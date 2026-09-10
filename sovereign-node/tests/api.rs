//! Integration tests for sovereign-node HTTP API.
//!
//! Spins up the axum Router in-process (no socket binding) and exercises all
//! REST endpoints using Tower's oneshot helper.

use axum::body::Body;
use http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use sovereign_node::node::{build_router, make_test_state, NodeState};

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Build a test app from a shared state (needed when tests make two calls to the same state).
fn make_app(state: NodeState) -> axum::Router {
    build_router(state)
}

async fn call_with(app: axum::Router, method: &str, uri: &str, body: Option<Value>)
    -> (StatusCode, Value)
{
    let req = if let Some(payload) = body {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap()
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    };

    let resp   = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes  = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let val    = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, val)
}

async fn call(method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    call_with(make_app(make_test_state()), method, uri, body).await
}

// ─── /health ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_returns_ok() {
    let (status, body) = call("GET", "/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true, "body: {:?}", body);
}

// ─── /status ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn status_returns_node_info() {
    let (status, body) = call("GET", "/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["did"].is_string(),        "did must be present: {:?}", body);
    assert!(body["uptime_secs"].is_number(), "uptime_secs must be present: {:?}", body);
    assert!(body["node"].is_string(),        "node name must be present: {:?}", body);
}

// ─── /devices ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn devices_returns_list() {
    let (status, body) = call("GET", "/devices", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["devices"].is_array(), "devices array expected: {:?}", body);
    assert_eq!(body["count"], 0);
}

// ─── /devices/register ───────────────────────────────────────────────────────

#[tokio::test]
async fn device_register_accepts_valid_manifest() {
    // Build a valid AgentDeviceManifest using the VCP adapter helper
    use vcp::adapters::{Go2Adapter, Go2ConnectionMode};
    let adapter  = Go2Adapter::new("unitree:go2:192.168.1.99", Go2ConnectionMode::default());
    let manifest = adapter.manifest("base64url:fakepub");

    let (status, body) = call("POST", "/devices/register",
        Some(serde_json::to_value(&manifest).unwrap())).await;
    assert_eq!(status, StatusCode::OK, "registration should succeed: {:?}", body);
    assert_eq!(body["ok"], true);
    assert_eq!(body["device_id"], "unitree:go2:192.168.1.99");
}

#[tokio::test]
async fn device_register_rejects_invalid_json() {
    let state = make_test_state();
    let app   = make_app(state);

    let req = Request::builder()
        .method("POST")
        .uri("/devices/register")
        .header("content-type", "application/json")
        .body(Body::from(b"not-json".as_ref()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::UNPROCESSABLE_ENTITY
        || resp.status() == StatusCode::BAD_REQUEST,
        "expected 422 or 400, got {}", resp.status()
    );
}

// ─── /jobs ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn jobs_list_initially_empty() {
    let (status, body) = call("GET", "/jobs", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["jobs"].is_array());
    assert_eq!(body["count"], 0);
}

// ─── /jobs/:id (missing) ─────────────────────────────────────────────────────

#[tokio::test]
async fn job_get_returns_404_for_unknown_id() {
    let (status, _) = call("GET", "/jobs/nonexistent-job-id", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── /receipts ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn receipts_returns_empty_list() {
    let (status, body) = call("GET", "/receipts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["receipts"].is_array());
    assert_eq!(body["count"], 0);
}

// ─── /receipts/:twin_id (missing) ────────────────────────────────────────────

#[tokio::test]
async fn receipt_get_returns_404_for_unknown_twin() {
    let (status, _) = call("GET", "/receipts/twin:sha256:unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── /mcp (JSON-RPC 2.0) ─────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_tools_list_returns_tool_array() {
    let payload = json!({
        "jsonrpc": "2.0",
        "id":      1,
        "method":  "tools/list",
        "params":  {}
    });
    let (status, body) = call("POST", "/mcp", Some(payload)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["jsonrpc"], "2.0");
    assert!(body["result"]["tools"].is_array(), "tools array expected: {:?}", body);
    assert!(!body["result"]["tools"].as_array().unwrap().is_empty(), "at least one MCP tool expected");
}

#[tokio::test]
async fn mcp_unknown_method_returns_error() {
    let payload = json!({
        "jsonrpc": "2.0",
        "id":      1,
        "method":  "nonexistent/method",
        "params":  {}
    });
    let (status, body) = call("POST", "/mcp", Some(payload)).await;
    assert_eq!(status, StatusCode::OK); // JSON-RPC errors return 200
    assert!(body["error"].is_object(), "error object expected for unknown method: {:?}", body);
}

// ─── /a2a/agent ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a2a_agent_card_has_required_fields() {
    let (status, body) = call("GET", "/a2a/agent", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["name"].is_string(),         "name must be present: {:?}", body);
    assert!(body["url"].is_string(),          "url must be present: {:?}", body);
    assert!(body["version"].is_string(),      "version must be present: {:?}", body);
    assert!(body["capabilities"].is_object(), "capabilities must be present: {:?}", body);
    assert!(body["skills"].is_array(),        "skills must be present: {:?}", body);
    assert!(!body["skills"].as_array().unwrap().is_empty(), "at least one skill required");
}

// ─── /a2a/tasks ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a2a_task_submission_returns_task() {
    let payload = json!({
        "message": {
            "role":  "user",
            "parts": [{ "type": "text", "text": "Hello from test" }]
        }
    });
    let (status, body) = call("POST", "/a2a/tasks", Some(payload)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["id"].is_string(),     "task id must be present: {:?}", body);
    assert!(body["status"].is_object(), "status must be present: {:?}", body);
}

#[tokio::test]
async fn a2a_task_submit_and_poll() {
    // Shared state across both requests
    let state = make_test_state();

    let submit_payload = json!({
        "message": {
            "role":  "user",
            "parts": [{ "type": "text", "text": "capture unitree:go2:test" }]
        }
    });

    let (_, submit_body) = call_with(make_app(state.clone()), "POST", "/a2a/tasks",
        Some(submit_payload)).await;
    let task_id = submit_body["id"].as_str().expect("task id missing");

    // Poll from same state
    let (status, body) = call_with(make_app(state.clone()), "GET",
        &format!("/a2a/tasks/{task_id}"), None).await;
    // A2aState is Clone — shared Arc internally — so the task is visible in both apps
    assert_eq!(status, StatusCode::OK, "poll failed: {:?}", body);
    assert_eq!(body["id"], task_id);
}

#[tokio::test]
async fn a2a_task_poll_unknown_id_returns_404() {
    let (status, _) = call("GET", "/a2a/tasks/nonexistent-task", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── /dip/inbound ────────────────────────────────────────────────────────────

#[tokio::test]
async fn dip_inbound_accepts_valid_envelope() {
    use dip::{DipEnvelope, DipKind};
    use dip::address::DipAddress;
    use sovereign_types::{IdentityChain, crypto::{generate_keypair, did_from_pubkey}};

    let (key, pub_key) = generate_keypair();
    let did = did_from_pubkey(&pub_key, "test");
    let identity = IdentityChain::new(did.clone(), did.clone());

    let envelope = DipEnvelope::build(
        DipAddress::vantage(&did),
        DipAddress::vantage("did:vantage:node:test"),
        identity,
        DipKind::Message,
        json!({ "text": "hello" }),
        3600,
        &key,
    ).expect("envelope build");

    let (status, body) = call("POST", "/dip/inbound", Some(
        serde_json::to_value(&envelope).unwrap()
    )).await;
    assert_eq!(status, StatusCode::OK, "DIP inbound should accept envelope: {:?}", body);
    assert_eq!(body["ok"], true);
    assert!(body["message_id"].is_string());
}

// ─── /ws/twin — WebSocket upgrade ────────────────────────────────────────────

#[tokio::test]
async fn ws_twin_route_exists() {
    // Tower oneshot can't complete a WebSocket upgrade (no real TCP connection),
    // so the handler returns 426 Upgrade Required — but NOT 404.
    // 426 confirms the route is wired and the WS handler was reached.
    let app = make_app(make_test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/ws/twin/test-twin")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("sec-websocket-version", "13")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // 426 = handler found but can't upgrade in-process test; 404 would mean missing route.
    assert_ne!(resp.status(), StatusCode::NOT_FOUND, "WS route must be registered");
    assert_eq!(resp.status().as_u16(), 426, "in-process WS returns 426 (not a real socket)");
}

#[tokio::test]
async fn ws_twin_wildcard_route_exists() {
    let app = make_app(make_test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/ws/twin/all")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("sec-websocket-version", "13")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(resp.status().as_u16(), 426);
}

// ─── /ws/splat ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ws_splat_route_exists() {
    // Same as twin WS test: 426 = route found, can't upgrade in-process.
    let app = make_app(make_test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/ws/splat/test-twin")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("sec-websocket-version", "13")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(resp.status(), StatusCode::NOT_FOUND, "splat WS route must be registered");
    assert_eq!(resp.status().as_u16(), 426, "in-process WS returns 426");
}

// ─── /capture/delegate ────────────────────────────────────────────────────────

#[tokio::test]
async fn capture_delegate_no_peers_returns_502() {
    // Default test state has no peers — delegation should fail with 502 Bad Gateway.
    let (status, body) = call("POST", "/capture/delegate", Some(json!({
        "device_id": "unitree:go2:192.168.1.10",
        "hint": "",
    }))).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "no peers → 502: {:?}", body);
    assert_eq!(body["error"], "delegation_failed");
}

#[tokio::test]
async fn capture_delegate_unknown_peer_returns_502() {
    // No peers configured + named peer requested → "not found" error
    let (status, body) = call("POST", "/capture/delegate", Some(json!({
        "device_id": "unitree:go2:192.168.1.10",
        "peer": "nonexistent-peer",
    }))).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let reason = body["reason"].as_str().unwrap_or("");
    // Either "not found in config" (named peer) or "no peers configured" (fallback)
    assert!(
        reason.contains("not found") || reason.contains("no peers"),
        "unexpected reason: {reason}"
    );
}

// ─── /tiles — Odù spatial tile endpoints ─────────────────────────────────────

#[tokio::test]
async fn tiles_list_returns_256_tiles() {
    let (status, body) = call("GET", "/tiles", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 256);
    let tiles = body["tiles"].as_array().unwrap();
    assert_eq!(tiles.len(), 256);
    // First tile is odu:00 at (0,0)
    assert_eq!(tiles[0]["tile_id"], "odu:00");
    assert_eq!(tiles[0]["x"], 0);
    assert_eq!(tiles[0]["y"], 0);
    // Last tile is odu:ff at (15,15)
    assert_eq!(tiles[255]["tile_id"], "odu:ff");
}

#[tokio::test]
async fn tile_receipts_empty_for_valid_tile() {
    let (status, body) = call("GET", "/tiles/odu:00/receipts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tile_id"], "odu:00");
    assert_eq!(body["count"], 0);
    assert!(body["receipts"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn tile_receipts_rejects_invalid_tile_id() {
    let (status, body) = call("GET", "/tiles/bad-tile/receipts", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_tile_id");
}

// ─── /events/receipts — SSE endpoint ─────────────────────────────────────────

#[tokio::test]
async fn sse_receipts_route_exists() {
    // GET /events/receipts should return 200 with text/event-stream content-type.
    // We can't consume the stream in oneshot (it never ends), so just check headers.
    let app = make_app(make_test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/events/receipts")
        .header("accept", "text/event-stream")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("text/event-stream"), "expected SSE content-type, got: {ct}");
}

// ─── /events/jobs — SSE endpoint ─────────────────────────────────────────────

#[tokio::test]
async fn sse_jobs_route_exists() {
    // GET /events/jobs should return 200 with text/event-stream content-type.
    // We can't consume the stream in oneshot (it never ends), so just check headers.
    let app = make_app(make_test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/events/jobs")
        .header("accept", "text/event-stream")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("text/event-stream"), "expected SSE content-type, got: {ct}");
}

// ─── /receipts/root — Merkle tree ────────────────────────────────────────────

#[tokio::test]
async fn receipt_merkle_root_empty() {
    let (status, body) = call("GET", "/receipts/root", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["root"].as_str().unwrap_or("").starts_with("sha256:"));
    assert_eq!(body["count"], 0);
    assert_eq!(body["algo"], "sha256-binary-merkle");
}

// ─── /capture/swarm — swarm capture ──────────────────────────────────────────

#[tokio::test]
async fn swarm_capture_requires_device_ids() {
    let (status, body) = call("POST", "/capture/swarm", Some(json!({
        "device_ids": []
    }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap_or("").contains("no device_ids"));
}

#[tokio::test]
async fn swarm_capture_creates_swarm_job() {
    let state = make_test_state();
    let app   = make_app(state.clone());
    let (status, body) = call_with(app, "POST", "/capture/swarm", Some(json!({
        "device_ids": ["unitree:go2:1", "unitree:go2:2"]
    }))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body["swarm_id"].as_str().unwrap_or("").starts_with("swarm:"));
    assert_eq!(body["device_ids"].as_array().unwrap().len(), 2);
    assert_eq!(body["child_jobs"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn swarm_get_returns_404_for_unknown() {
    let (status, body) = call("GET", "/swarm/swarm:unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "swarm_not_found");
}

#[tokio::test]
async fn swarm_list_initially_empty() {
    let (status, body) = call("GET", "/swarm", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 0);
}

#[tokio::test]
async fn swarm_poll_after_submit() {
    let state = make_test_state();
    let (_, submit_body) = call_with(make_app(state.clone()), "POST", "/capture/swarm",
        Some(json!({ "device_ids": ["unitree:go2:stub"] }))).await;
    let swarm_id = submit_body["swarm_id"].as_str().unwrap();

    let (status, body) = call_with(make_app(state), "GET",
        &format!("/swarm/{swarm_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["swarm_id"], swarm_id);
}

// ─── /twins/:id/timeline — 4D provenance ─────────────────────────────────────

#[tokio::test]
async fn twin_timeline_returns_404_initially() {
    let (status, body) = call("GET", "/twins/unitree:go2:stub/timeline", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "timeline_not_found");
}

#[tokio::test]
async fn timelines_list_initially_empty() {
    let (status, body) = call("GET", "/timelines", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 0);
}

// ─── /federation/peers ────────────────────────────────────────────────────────

#[tokio::test]
async fn federation_peers_returns_empty_list_without_avahi() {
    // avahi-browse is unlikely to be available in CI; the handler should still
    // return 200 with an empty peers array rather than erroring.
    let (status, body) = call("GET", "/federation/peers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["peers"].is_array());
    // count may be 0 (no avahi) or N (avahi found peers) — both are valid
    assert!(body["count"].as_u64().is_some());
}

// ─── /receipts/export ────────────────────────────────────────────────────────

#[tokio::test]
async fn receipt_export_returns_ndjson() {
    let app = make_app(make_test_state());
    let req = Request::builder()
        .method("GET")
        .uri("/receipts/export")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("ndjson") || ct.contains("json"), "expected ndjson content-type, got: {ct}");
}

// ─── /tiles/:tile_id/economy ─────────────────────────────────────────────────

#[tokio::test]
async fn tile_economy_returns_404_initially() {
    let (status, body) = call("GET", "/tiles/odu:00/economy", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "tile_economy_not_found");
}

// ─── /dip/gossip ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn dip_gossip_returns_ok_with_no_peers() {
    let (status, body) = call("POST", "/dip/gossip", Some(json!({ "count": 5 }))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["pushed"].is_number());
    assert_eq!(body["peers"], 0); // no peers configured in test state
}

// ─── /receipts/verify/:id ────────────────────────────────────────────────────

#[tokio::test]
async fn receipt_verify_returns_404_for_unknown() {
    let (status, body) = call("GET", "/receipts/verify/nonexistent-receipt", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["verified"], false);
    assert!(body["merkle_root"].is_string());
}

// ─── /jobs/:id/retry ─────────────────────────────────────────────────────────

#[tokio::test]
async fn job_retry_returns_404_for_unknown_job() {
    let (status, body) = call("POST", "/jobs/nonexistent-job/retry", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "job_not_found");
}

// ─── /tiles/:tile_id/claim ───────────────────────────────────────────────────

#[tokio::test]
async fn tile_claim_sets_owner_and_returns_ok() {
    let state = make_test_state();
    let app   = make_app(state);
    let (status, body) = call_with(app, "POST", "/tiles/odu:00/claim",
        Some(json!({ "owner_did": "did:vantage:test123" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["owner_did"], "did:vantage:test123");
    assert_eq!(body["stub"], true);
}

#[tokio::test]
async fn tile_claim_rejects_invalid_tile_id() {
    let (status, body) = call("POST", "/tiles/invalid/claim",
        Some(json!({ "owner_did": "did:test" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_tile_id");
}

// ─── /config/check ───────────────────────────────────────────────────────────

#[tokio::test]
async fn config_check_returns_node_info() {
    let (status, body) = call("GET", "/config/check", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["did"].is_string());
    assert!(body["node"].is_string());
    assert!(body["warnings"].is_array());
    assert!(body["warning_count"].as_u64().is_some());
}

// ─── /ip/root — IP provenance ─────────────────────────────────────────────────

#[tokio::test]
async fn ip_root_returns_404_without_nsec() {
    let (status, _body) = call("GET", "/ip/root", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ip_receipt_returns_404_without_nsec() {
    let (status, _body) = call("GET", "/ip/receipt/twin%3Aabc123", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn receipt_verify_unknown_returns_not_in_tree() {
    let (status, body) = call("GET", "/receipts/verify/unknown-receipt-id", None).await;
    // Should be 200 with verified=false, or 404 — both are acceptable
    assert!(status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "expected 200 or 404, got {status}");
    if status == StatusCode::OK {
        assert_eq!(body["verified"], serde_json::json!(false));
    }
}

// ─── /agent/receipts ─────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_receipts_initially_empty() {
    let (status, body) = call("GET", "/agent/receipts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"],    serde_json::json!(0));
    assert_eq!(body["verified"], serde_json::json!(true));
    assert!(body["receipts"].is_array(), "receipts must be an array: {:?}", body);
    assert_eq!(body["receipts"].as_array().unwrap().len(), 0);
}

// ─── /proofs/simulation ───────────────────────────────────────────────────────

#[tokio::test]
async fn proof_simulation_submit_clean_run() {
    let proof = json!({
        "proof_id":          "p-test-1",
        "agent_id":          "agent:demo",
        "principal_id":      "did:vantage:demo",
        "simulation_id":     "sim:scarab-001",
        "environment_hash":  "e_alpha_001",
        "world_hash":        "w_001",
        "model_hash":        "m_001",
        "controller_hash":   "c_001",
        "input_hash":        "i_001",
        "simulator_version": "0.1.0",
        "seed":              42,
        "trajectory_hash":   "traj_sha256_001",
        "sensor_hash":       "sensor_sha256_001",
        "checkpoint_root":   "ckpt_root_001",
        "metrics": {
            "execution_time_ms":    12400,
            "energy_estimate":      87.2,
            "gates_cleared":        5,
            "gates_total":          5,
            "crashes":              0,
            "collision_margin_m":   0.84,
            "controller_stability": 0.91
        },
        "outcome": "success",
        "timestamp": 0,
        "signature": "test_sig"
    });
    let (status, body) = call("POST", "/proofs/simulation", Some(proof)).await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["proof_id"], "p-test-1");
    assert!(body["proof_value"].as_f64().unwrap_or(0.0) > 0.0);
    assert_eq!(body["proof_type"], "simulation");
    // Clean run with all hashes → mint_eligible should be true
    assert_eq!(body["mint_eligible"], serde_json::json!(true));
}

#[tokio::test]
async fn proof_simulation_submit_crashed_not_eligible() {
    let proof = json!({
        "proof_id":          "p-test-crash",
        "agent_id":          "agent:demo",
        "principal_id":      "did:vantage:demo",
        "simulation_id":     "sim:crash-001",
        "environment_hash":  "e_crash_001",
        "world_hash":        "w_001",
        "model_hash":        "m_001",
        "controller_hash":   "c_001",
        "input_hash":        "i_001",
        "simulator_version": "0.1.0",
        "seed":              1,
        "trajectory_hash":   "traj_001",
        "sensor_hash":       "sensor_001",
        "checkpoint_root":   "ckpt_001",
        "metrics": {
            "execution_time_ms":    5000,
            "energy_estimate":      40.0,
            "gates_cleared":        2,
            "gates_total":          5,
            "crashes":              1,
            "collision_margin_m":   0.0,
            "controller_stability": 0.3
        },
        "outcome": "failure",
        "timestamp": 0,
        "signature": ""
    });
    let (status, body) = call("POST", "/proofs/simulation", Some(proof)).await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["mint_eligible"], serde_json::json!(false));
}

#[tokio::test]
async fn proof_simulation_get_unknown_returns_404() {
    let (status, _) = call("GET", "/proofs/simulation/nonexistent-proof", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── /body — body sessions & capabilities ────────────────────────────────────

#[tokio::test]
async fn body_capabilities_returns_stampfly_catalogue() {
    let (status, body) = call("GET", "/body/capabilities", None).await;
    assert_eq!(status, StatusCode::OK, "body: {body:?}");
    assert_eq!(body["body"], "stampfly_v1_1");
    let caps = body["capabilities"].as_array().expect("capabilities array");
    assert!(caps.len() >= 8, "expected >= 8 StampFly capabilities, got {}", caps.len());
    // emergency_stop must always be present
    assert!(caps.iter().any(|c| c["id"] == "flight.emergency_stop"));
}

#[tokio::test]
async fn body_sessions_empty_initially() {
    let (status, body) = call("GET", "/body/sessions", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], serde_json::json!(0));
    assert!(body["sessions"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn body_session_open_t4_supervised() {
    let state = make_test_state();
    let app   = make_app(state);
    let (status, body) = call_with(app, "POST", "/body/sessions", Some(json!({
        "agent_id":    "agent:test",
        "agent_tier":  "t4",
        "body_id":     "stampfly:001",
        "mode":        "human_supervised",
        "capabilities": ["sensor.imu", "flight.arm"],
        "sim_proof_id": "proof:sim:001"
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "body: {body:?}");
    assert!(body["session_id"].as_str().unwrap().starts_with("body:"));
    assert_eq!(body["agent_tier"], "t4");
    assert_eq!(body["mode"], "human_supervised");
}

#[tokio::test]
async fn body_session_open_t3_autonomous_rejected() {
    let (status, body) = call("POST", "/body/sessions", Some(json!({
        "agent_id":   "agent:low-tier",
        "agent_tier": "t3",
        "body_id":    "stampfly:001",
        "mode":       "autonomous"
    }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
    assert!(body["error"].as_str().unwrap().contains("t3")
        || body["error"].as_str().unwrap().contains("T3"));
}

#[tokio::test]
async fn body_session_get_unknown_returns_404() {
    let (status, _) = call("GET", "/body/sessions/nonexistent-session", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn body_receipts_empty_for_unknown_body() {
    let (status, body) = call("GET", "/body/stampfly:unknown/receipts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], serde_json::json!(0));
}
