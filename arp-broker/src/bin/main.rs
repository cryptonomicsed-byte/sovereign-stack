//! ARP Broker — Action Receipt Protocol v1 HTTP service.
//!
//! Stores ActionReceipts in memory, verifies SHA-256 hash chains,
//! and exposes them for query by Zàngbétò and other verifiers.
//!
//! Port: 7795 (ARP_PORT env override)
//!
//! Routes:
//!   GET  /health
//!   POST /api/receipts          — submit an ActionReceipt
//!   GET  /api/receipts/:id      — fetch by receipt_id
//!   GET  /api/receipts/agent/:agent_id — list receipts for an agent (ordered by timestamp)
//!   POST /api/receipts/verify-chain    — verify a list of receipts form a valid chain

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use arp_types::ActionReceipt;
use chrono::Utc;

type AppState = Arc<BrokerState>;

struct BrokerState {
    /// receipt_id → ActionReceipt
    receipts: RwLock<HashMap<String, ActionReceipt>>,
    /// agent_id → ordered list of receipt_ids (by timestamp)
    by_agent: RwLock<HashMap<String, Vec<String>>>,
}

impl BrokerState {
    fn new() -> Self {
        Self {
            receipts: RwLock::new(HashMap::new()),
            by_agent: RwLock::new(HashMap::new()),
        }
    }
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("ARP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7795);

    let state = Arc::new(BrokerState::new());

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/receipts", post(submit_receipt))
        .route("/api/receipts/verify-chain", post(verify_chain))
        .route("/api/receipts/:id", get(get_receipt))
        .route("/api/receipts/agent/:agent_id", get(list_agent_receipts))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    println!("ARP broker listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── handlers ─────────────────────────────────────────────────────────────────

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status":  "ok",
        "service": "arp-broker",
        "version": "0.1.0",
        "timestamp": Utc::now().timestamp(),
    }))
}

async fn submit_receipt(
    State(s): State<AppState>,
    Json(receipt): Json<ActionReceipt>,
) -> (StatusCode, Json<serde_json::Value>) {
    let agent_id   = receipt.principal.agent_id.clone();
    let receipt_id = receipt.receipt_id.to_string();
    let hash       = receipt.hash();

    // Verify chain link if previous_hash is set
    if let Some(prev_hash) = &receipt.previous_hash {
        let rmap = s.receipts.read().unwrap();
        let agent_receipts = s.by_agent.read().unwrap();
        let chain_ok = agent_receipts
            .get(&agent_id)
            .and_then(|ids| ids.last())
            .and_then(|last_id| rmap.get(last_id))
            .map(|prev_receipt| &prev_receipt.hash() == prev_hash)
            .unwrap_or(false);

        if !chain_ok {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "chain_broken",
                    "detail": "previous_hash does not match last stored receipt for this agent",
                    "receipt_id": receipt_id,
                })),
            );
        }
    }

    // Store receipt
    {
        let mut rmap  = s.receipts.write().unwrap();
        let mut amap  = s.by_agent.write().unwrap();
        rmap.insert(receipt_id.clone(), receipt);
        amap.entry(agent_id.clone())
            .or_default()
            .push(receipt_id.clone());
    }

    (StatusCode::CREATED, Json(serde_json::json!({
        "receipt_id": receipt_id,
        "hash":       hash,
        "agent_id":   agent_id,
    })))
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

async fn list_agent_receipts(
    State(s): State<AppState>,
    Path(agent_id): Path<String>,
) -> Json<serde_json::Value> {
    let rmap = s.receipts.read().unwrap();
    let amap = s.by_agent.read().unwrap();
    let ids  = amap.get(&agent_id).cloned().unwrap_or_default();

    let receipts: Vec<serde_json::Value> = ids.iter()
        .filter_map(|id| rmap.get(id))
        .map(|r| serde_json::to_value(r).unwrap_or_default())
        .collect();

    let count = receipts.len();
    Json(serde_json::json!({ "agent_id": agent_id, "receipts": receipts, "count": count }))
}

/// Verify a submitted ordered slice of receipts forms an unbroken hash chain.
async fn verify_chain(
    Json(receipts): Json<Vec<ActionReceipt>>,
) -> Json<serde_json::Value> {
    if receipts.is_empty() {
        return Json(serde_json::json!({"valid": true, "length": 0}));
    }

    for (i, window) in receipts.windows(2).enumerate() {
        let prev = &window[0];
        let curr = &window[1];
        if !curr.chain_valid(prev) {
            return Json(serde_json::json!({
                "valid":        false,
                "broken_at":    i + 1,
                "expected_hash": prev.hash(),
                "got_hash":      curr.previous_hash,
            }));
        }
    }

    Json(serde_json::json!({
        "valid":  true,
        "length": receipts.len(),
        "tip_hash": receipts.last().map(|r| r.hash()),
    }))
}
