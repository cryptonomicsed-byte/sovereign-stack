use std::sync::Arc;
use std::collections::HashMap;
use std::sync::RwLock;
use axum::{Router, Json, extract::{State, Path}, routing::{get, post}, http::StatusCode};
use witness_types::{
    WitnessAttestation, AttestationKind, AttestationStatus,
    ObservationBundle, SensorReading,
    NOSTR_KIND_WITNESS,
};
use uuid::Uuid;
use chrono::Utc;

type AppState = Arc<BrokerState>;

struct BrokerState {
    attestations: RwLock<HashMap<String, WitnessAttestation>>,
    bundles:      RwLock<HashMap<String, ObservationBundle>>,
}

impl BrokerState {
    fn new() -> Self {
        Self {
            attestations: RwLock::new(HashMap::new()),
            bundles:      RwLock::new(HashMap::new()),
        }
    }
}

#[tokio::main]
async fn main() {
    let state = Arc::new(BrokerState::new());
    let port: u16 = std::env::var("WITNESS_PORT")
        .ok().and_then(|p| p.parse().ok())
        .unwrap_or(7794);

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/observations",          post(submit_observation))
        .route("/api/observations/:id",      get(get_observation))
        .route("/api/attestations",          get(list_attestations).post(create_attestation))
        .route("/api/attestations/:id",      get(get_attestation))
        .route("/api/attestations/:id/sign", post(sign_attestation))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    println!("Witness broker listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "witness-broker"}))
}

async fn submit_observation(
    State(s): State<AppState>,
    Json(bundle): Json<ObservationBundle>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut b = bundle;
    b.hash = b.compute_hash();
    let id = b.bundle_id.clone();
    let hash = b.hash.clone();
    s.bundles.write().unwrap().insert(id.clone(), b);
    (StatusCode::CREATED, Json(serde_json::json!({"bundle_id": id, "hash": hash})))
}

async fn get_observation(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match s.bundles.read().unwrap().get(&id).cloned() {
        Some(b) => (StatusCode::OK, Json(serde_json::to_value(b).unwrap_or_default())),
        None    => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))),
    }
}

#[derive(serde::Deserialize)]
struct AttestationRequest {
    vcp_session_id:  String,
    device_id:       String,
    agent_id:        String,
    bundle_id:       String,
    outcome:         String,
    kind:            Option<String>,
    sim_receipt_id:  Option<String>,
    latitude:        Option<f64>,
    longitude:       Option<f64>,
    altitude_m:      Option<f64>,
}

async fn create_attestation(
    State(s): State<AppState>,
    Json(req): Json<AttestationRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let bundle_hash = s.bundles.read().unwrap()
        .get(&req.bundle_id)
        .map(|b| b.hash.clone())
        .unwrap_or_else(|| format!("bundle_unknown:{}", req.bundle_id));

    let attest_id = Uuid::new_v4().to_string();
    let attestation = WitnessAttestation {
        attest_id:        attest_id.clone(),
        kind:             AttestationKind::PolicyExecution,
        vcp_session_id:   req.vcp_session_id,
        device_id:        req.device_id,
        agent_id:         req.agent_id,
        sim_receipt_id:   req.sim_receipt_id,
        observation_hash: bundle_hash,
        outcome:          req.outcome,
        status:           AttestationStatus::Pending,
        latitude:         req.latitude,
        longitude:        req.longitude,
        altitude_m:       req.altitude_m,
        nostr_event_id:   None,
        zangbeto_anchor:  None,
        arp_receipt_id:   None,
        timestamp:        Utc::now(),
        signature:        String::new(),
    };

    let hash = attestation.canonical_hash();
    let tags = attestation.to_nostr_tags();
    s.attestations.write().unwrap().insert(attest_id.clone(), attestation);

    (StatusCode::CREATED, Json(serde_json::json!({
        "attest_id":      attest_id,
        "canonical_hash": hash,
        "nostr_kind":     NOSTR_KIND_WITNESS,
        "nostr_tags":     tags,
        "status":         "pending",
    })))
}

async fn list_attestations(State(s): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let lock = s.attestations.read().unwrap();
    Json(lock.values().map(|a| serde_json::to_value(a).unwrap_or_default()).collect())
}

async fn get_attestation(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match s.attestations.read().unwrap().get(&id).cloned() {
        Some(a) => (StatusCode::OK, Json(serde_json::to_value(a).unwrap_or_default())),
        None    => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))),
    }
}

#[derive(serde::Deserialize)]
struct SignRequest { signature: String }

async fn sign_attestation(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SignRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut attestations = s.attestations.write().unwrap();
    match attestations.get_mut(&id) {
        Some(a) => {
            a.signature = req.signature;
            a.status    = AttestationStatus::Signed;
            let hash = a.canonical_hash();
            (StatusCode::OK, Json(serde_json::json!({"ok": true, "canonical_hash": hash, "status": "signed"})))
        },
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))),
    }
}
