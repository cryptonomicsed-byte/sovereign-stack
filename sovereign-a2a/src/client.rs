//! HTTP client for sending tasks to a remote A2A agent.
//!
//! Usage:
//!   let client = A2aClient::new("http://peer-node:7779");
//!   let task = client.submit_task("Capture unitree:go2:192.168.1.10").await?;
//!   let done = client.poll_until_done(&task.id, 60).await?;

use tracing::{debug, warn};

use crate::types::{A2aMessage, A2aTask, AgentCard, Part, TaskSendParams, TaskState};

pub struct A2aClient {
    base_url: String,
    client:   reqwest::Client,
}

impl A2aClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    /// GET /a2a/agent — fetch the remote AgentCard.
    pub async fn agent_card(&self) -> Result<AgentCard, String> {
        let url = format!("{}/a2a/agent", self.base_url);
        self.client.get(&url).send().await
            .map_err(|e| e.to_string())?
            .json::<AgentCard>().await
            .map_err(|e| e.to_string())
    }

    /// POST /a2a/tasks — submit a text task to the remote agent.
    pub async fn submit_task(&self, text: impl Into<String>) -> Result<A2aTask, String> {
        let params = TaskSendParams {
            id:         None,
            session_id: None,
            metadata:   Default::default(),
            message: A2aMessage {
                role:  "user".into(),
                parts: vec![Part::Text { text: text.into() }],
            },
        };
        let url = format!("{}/a2a/tasks", self.base_url);
        self.client.post(&url).json(&params).send().await
            .map_err(|e| e.to_string())?
            .json::<A2aTask>().await
            .map_err(|e| e.to_string())
    }

    /// GET /a2a/tasks/:id — poll current task state.
    pub async fn get_task(&self, task_id: &str) -> Result<A2aTask, String> {
        let url = format!("{}/a2a/tasks/{task_id}", self.base_url);
        self.client.get(&url).send().await
            .map_err(|e| e.to_string())?
            .json::<A2aTask>().await
            .map_err(|e| e.to_string())
    }

    /// POST /a2a/tasks/:id/cancel — cancel a task.
    pub async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        let url = format!("{}/a2a/tasks/{task_id}/cancel", self.base_url);
        let resp = self.client.post(&url).send().await
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() { Ok(()) } else {
            Err(format!("cancel returned {}", resp.status()))
        }
    }

    /// Poll until the task reaches a terminal state or `timeout_secs` elapses.
    /// Returns the final task. Polls every 2 seconds.
    pub async fn poll_until_done(&self, task_id: &str, timeout_secs: u64) -> Result<A2aTask, String> {
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(timeout_secs);

        loop {
            let task = self.get_task(task_id).await?;
            match task.status.state {
                TaskState::Completed | TaskState::Failed | TaskState::Canceled => {
                    debug!(task_id = %task_id, state = ?task.status.state, "A2A task terminal");
                    return Ok(task);
                }
                _ => {}
            }
            if std::time::Instant::now() >= deadline {
                warn!(task_id = %task_id, "A2A poll timeout after {timeout_secs}s");
                return Err(format!("poll timeout after {timeout_secs}s"));
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }
}
