use std::sync::Arc;
use std::collections::HashMap;
use std::sync::RwLock;
use axum::{Router, Json, extract::{State, Path}, routing::{get, post}, http::StatusCode};
use swarm_types::{SimReceipt, SimOutcome};
use uuid::Uuid;
use chrono::Utc;

type AppState = Arc<BrokerState>;

struct BrokerState {
    receipts: RwLock<HashMap<String, SimReceipt>>,
}

impl BrokerState {
    fn new() -> Self {
        Self { receipts: RwLock::new(HashMap::new()) }
    }
}

#[tokio::main]
async fn main() {
    let state = Arc::new(BrokerState::new());
    let port: u16 = std::env::var("SWARM_PORT")
        .ok().and_then(|p| p.parse().ok())
        .unwrap_or(7793);

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/sim/run",           post(run_simulation))
        .route("/api/sim/receipts",      get(list_receipts))
        .route("/api/sim/receipts/:id",  get(get_receipt))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    println!("ScarabSwarm broker listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "swarm-broker"}))
}

#[derive(serde::Deserialize)]
struct SimRequest {
    twin_id:   String,
    agent_id:  String,
    session_id: Option<String>,
    n_candidates: Option<u32>,
}

/// Stub simulation: generates N candidate trajectories and selects the best.
/// Real implementation calls OSOVM's veilsim_engine over HTTP.
async fn run_simulation(
    State(s): State<AppState>,
    Json(req): Json<SimRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let n = req.n_candidates.unwrap_or(100);
    let receipt_id = Uuid::new_v4().to_string();
    let winning_id = Uuid::new_v4().to_string();

    // Stub Merkle root — real impl hashes all trajectory sim_hash values
    let merkle_root = format!("merkle_{}", &receipt_id[..8]);
    let policy_hash = format!("policy_{}", &winning_id[..8]);
    let proof_of_sim = format!("pos_{}_{}", &merkle_root[..8], &policy_hash[..8]);

    let receipt = SimReceipt {
        receipt_id: receipt_id.clone(),
        twin_id:    req.twin_id.clone(),
        agent_id:   req.agent_id.clone(),
        session_id: req.session_id.clone(),
        n_trajectories: n,
        n_feasible:      n.saturating_sub(n / 10),
        winning_traj_id: winning_id.clone(),
        winner_score:    0.87,
        merkle_root,
        policy_hash,
        proof_of_sim: proof_of_sim.clone(),
        outcome:     SimOutcome::PolicySelected,
        zangbeto_anchor:  None,
        witness_event_id: None,
        created_at:  Utc::now(),
        signature:   String::new(),
    };

    let hash = receipt.canonical_hash();
    s.receipts.write().unwrap().insert(receipt_id.clone(), receipt.clone());

    (StatusCode::CREATED, Json(serde_json::json!({
        "receipt_id":    receipt_id,
        "winning_traj":  winning_id,
        "proof_of_sim":  proof_of_sim,
        "canonical_hash": hash,
        "n_trajectories": n,
    })))
}

async fn list_receipts(State(s): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let receipts = s.receipts.read().unwrap();
    Json(receipts.values().map(|r| serde_json::to_value(r).unwrap_or_default()).collect())
}

async fn get_receipt(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match s.receipts.read().unwrap().get(&id).cloned() {
        Some(r) => (StatusCode::OK, Json(serde_json::to_value(r).unwrap_or_default())),
        None    => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))),
    }
}
