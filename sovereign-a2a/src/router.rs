//! Axum router for A2A v1.0 endpoints.
//!
//! Mount with:
//!   router.nest("/a2a", a2a_router(state))
//!
//! Routes:
//!   GET  /agent           — AgentCard
//!   POST /tasks           — submit Task (TaskSendParams → A2aTask)
//!   GET  /tasks/:id       — poll Task
//!   POST /tasks/:id/cancel — cancel Task

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json,
};
use tracing::{info, warn};

use crate::types::{
    A2aConfig, A2aError, A2aMessage, A2aTask, AgentCard,
    Artifact, Part, TaskSendParams, TaskState, TaskStatus,
};

/// A2A skill dispatch request sent from the router to the host application.
#[derive(Debug, Clone)]
pub struct A2aDispatchRequest {
    pub task_id:  String,
    pub skill:    String,       // "capture" | "receipt" | ...
    pub text:     String,       // raw user message text
}

/// Shared state for the A2A router.
#[derive(Clone)]
pub struct A2aState {
    pub config:     A2aConfig,
    pub agent_card: AgentCard,
    tasks:          Arc<RwLock<HashMap<String, A2aTask>>>,
    /// Optional channel to forward skill dispatch requests to the host node.
    /// If None, the router handles requests with built-in stubs.
    pub dispatch_tx: Option<mpsc::Sender<A2aDispatchRequest>>,
}

impl A2aState {
    pub fn new(config: A2aConfig) -> Self {
        let agent_card = config.to_agent_card();
        Self {
            config,
            agent_card,
            tasks:       Arc::new(RwLock::new(HashMap::new())),
            dispatch_tx: None,
        }
    }

    /// Attach a dispatch channel so skill requests are forwarded to the host.
    pub fn with_dispatch(mut self, tx: mpsc::Sender<A2aDispatchRequest>) -> Self {
        self.dispatch_tx = Some(tx);
        self
    }

    pub async fn get_task(&self, id: &str) -> Option<A2aTask> {
        self.tasks.read().await.get(id).cloned()
    }

    pub async fn insert_task(&self, task: A2aTask) {
        self.tasks.write().await.insert(task.id.clone(), task);
    }

    pub async fn update_task_state(&self, id: &str, state: TaskState, message: Option<String>) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(id) {
            task.status = TaskStatus {
                state,
                updated: now_iso(),
                message: message.map(|m| A2aMessage {
                    role:  "agent".into(),
                    parts: vec![Part::Text { text: m }],
                }),
            };
        }
    }

    pub async fn complete_task(&self, id: &str, artifacts: Vec<Artifact>) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(id) {
            task.status = TaskStatus {
                state:   TaskState::Completed,
                updated: now_iso(),
                message: None,
            };
            task.artifacts = artifacts;
        }
    }
}

/// Build the A2A sub-router (mount at /a2a in parent Router).
///
/// Generic over the parent state `S` so it can be nested directly into any axum
/// `Router<S>` as long as `S` implements `FromRef<A2aState>` (i.e. `NodeState`
/// has an `A2aState` field reachable via `axum::extract::FromRef`).
pub fn a2a_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    A2aState: axum::extract::FromRef<S>,
{
    Router::new()
        .route("/agent",             get(handle_agent_card))
        .route("/tasks",             post(handle_task_submit))
        .route("/tasks/:id",         get(handle_task_get))
        .route("/tasks/:id/cancel",  post(handle_task_cancel))
}

/// GET /a2a/agent — return AgentCard
async fn handle_agent_card(State(s): State<A2aState>) -> Json<AgentCard> {
    Json(s.agent_card.clone())
}

/// POST /a2a/tasks — submit a new task
async fn handle_task_submit(
    State(s): State<A2aState>,
    Json(params): Json<TaskSendParams>,
) -> impl IntoResponse {
    let task_id = params.id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let task = A2aTask {
        id:         task_id.clone(),
        session_id: params.session_id,
        status: TaskStatus {
            state:   TaskState::Submitted,
            updated: now_iso(),
            message: None,
        },
        history:   vec![params.message.clone()],
        artifacts: vec![],
        metadata:  params.metadata,
    };

    s.insert_task(task.clone()).await;
    info!(task_id = %task_id, "A2A task submitted");

    // Kick off async processing
    let state_clone = s.clone();
    let msg = params.message;
    tokio::spawn(async move {
        dispatch_task(state_clone, task_id, msg).await;
    });

    Json(task)
}

/// GET /a2a/tasks/:id — poll task status
async fn handle_task_get(
    State(s): State<A2aState>,
    Path(id):  Path<String>,
) -> impl IntoResponse {
    match s.get_task(&id).await {
        Some(task) => (StatusCode::OK, Json(serde_json::to_value(task).unwrap())).into_response(),
        None => {
            let err = A2aError::task_not_found(&id);
            (StatusCode::NOT_FOUND, Json(serde_json::to_value(err).unwrap())).into_response()
        }
    }
}

/// POST /a2a/tasks/:id/cancel — cancel a task
async fn handle_task_cancel(
    State(s): State<A2aState>,
    Path(id):  Path<String>,
) -> impl IntoResponse {
    match s.get_task(&id).await {
        Some(task) if task.status.state == TaskState::Working
                   || task.status.state == TaskState::Submitted => {
            s.update_task_state(&id, TaskState::Canceled, Some("Canceled by requester".into())).await;
            info!(task_id = %id, "A2A task canceled");
            (StatusCode::OK, Json(serde_json::json!({ "id": id, "state": "canceled" }))).into_response()
        }
        Some(_) => {
            let err = A2aError { code: -32002, message: "task not cancelable".into(), data: None };
            (StatusCode::CONFLICT, Json(serde_json::to_value(err).unwrap())).into_response()
        }
        None => {
            let err = A2aError::task_not_found(&id);
            (StatusCode::NOT_FOUND, Json(serde_json::to_value(err).unwrap())).into_response()
        }
    }
}

/// Dispatch: determine task type from message text and route to handler.
///
/// If `dispatch_tx` is set, skill requests are forwarded to the host node
/// (sovereign-node) which runs the actual pipeline. Otherwise uses built-in stubs.
///
/// Skill routing:
///   - "capture" in text → twin capture pipeline
///   - "receipt" in text → receipt query
///   - anything else     → unsupported skill error
async fn dispatch_task(state: A2aState, task_id: String, message: A2aMessage) {
    state.update_task_state(&task_id, TaskState::Working, None).await;

    let text = message.parts.iter()
        .find_map(|p| if let Part::Text { text } = p { Some(text.as_str()) } else { None })
        .unwrap_or("");

    let text_lower = text.to_lowercase();

    let skill = if text_lower.contains("capture") {
        "capture"
    } else if text_lower.contains("receipt") {
        "receipt"
    } else {
        state.update_task_state(
            &task_id,
            TaskState::Failed,
            Some(format!("Unrecognized skill request: '{text}'")),
        ).await;
        return;
    };

    // Forward to host node if dispatch channel is available
    if let Some(tx) = &state.dispatch_tx {
        let req = A2aDispatchRequest {
            task_id:  task_id.clone(),
            skill:    skill.into(),
            text:     text.into(),
        };
        if tx.send(req).await.is_err() {
            warn!(task_id = %task_id, "A2A dispatch channel closed — using stub");
        } else {
            // Host takes ownership of completing the task
            return;
        }
    }

    // Built-in stubs (no host dispatch channel)
    match skill {
        "capture" => {
            warn!(task_id = %task_id, "A2A capture: stub (no pipeline channel)");
            state.complete_task(&task_id, vec![Artifact {
                name:  "notice".into(),
                parts: vec![Part::Text {
                    text: "Capture acknowledged. Wire A2A dispatch_tx to sovereign-node for live execution.".into()
                }],
                index: vec![0],
            }]).await;
        }
        "receipt" => {
            warn!(task_id = %task_id, "A2A receipt: stub (no pipeline channel)");
            state.complete_task(&task_id, vec![Artifact {
                name:  "receipts".into(),
                parts: vec![Part::Text {
                    text: "Receipt query acknowledged. Wire to receipt_store for live results.".into()
                }],
                index: vec![0],
            }]).await;
        }
        _ => unreachable!(),
    }
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // ISO-8601 seconds precision — sufficient for A2A task timestamps
    format!("{ms}Z")
}
