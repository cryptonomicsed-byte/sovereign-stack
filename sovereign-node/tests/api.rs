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

// ─── /governance/proposals ────────────────────────────────────────────────────

fn sample_proposal_payload() -> serde_json::Value {
    json!({
        "proposer":           "0xPROPOSER",
        "recipient":          "0xRECIPIENT",
        "amount_micro_ase":   1_000_000u64,
        "purpose":            "fund open-source tooling",
        "veil_id":            0u64
    })
}

#[tokio::test]
async fn governance_create_and_list() {
    let state = make_test_state();
    let app   = make_app(state);

    // POST a new proposal
    let (create_status, created) = call_with(
        app.clone(),
        "POST",
        "/governance/proposals",
        Some(sample_proposal_payload()),
    ).await;
    assert_eq!(create_status, StatusCode::CREATED, "create: {:?}", created);
    assert_eq!(created["id"], 1, "first proposal should have id=1: {:?}", created);

    // GET the full list — expect count=1
    let (list_status, list) = call_with(app, "GET", "/governance/proposals", None).await;
    assert_eq!(list_status, StatusCode::OK, "list: {:?}", list);
    assert_eq!(list["count"], 1, "list should have count=1: {:?}", list);
    assert!(list["proposals"].is_array());
    assert_eq!(list["proposals"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn governance_vote_for() {
    let state = make_test_state();
    let app   = make_app(state);

    // Create a proposal first
    let (_, _) = call_with(
        app.clone(),
        "POST",
        "/governance/proposals",
        Some(sample_proposal_payload()),
    ).await;

    // Cast a vote_for on proposal id=1
    let (vote_status, voted) = call_with(
        app.clone(),
        "POST",
        "/governance/proposals/1/vote_for",
        None,
    ).await;
    assert_eq!(vote_status, StatusCode::OK, "vote_for: {:?}", voted);
    let votes_for = voted["votes_for"].as_u64().unwrap_or(0);
    assert!(votes_for > 0, "votes_for should be > 0 after vote_for: {:?}", voted);
    // Exactly one bit should be set → count_ones == 1
    assert_eq!(votes_for.count_ones(), 1, "one bit set: {:?}", voted);
}

#[tokio::test]
async fn governance_execute_fails_before_quorum() {
    let state = make_test_state();
    let app   = make_app(state);

    // Create a proposal — 0 votes, timelock not yet elapsed
    let (_, _) = call_with(
        app.clone(),
        "POST",
        "/governance/proposals",
        Some(sample_proposal_payload()),
    ).await;

    // Attempt to execute immediately — should fail (no quorum + timelock)
    let (exec_status, exec_body) = call_with(
        app,
        "POST",
        "/governance/proposals/1/execute",
        None,
    ).await;
    assert_eq!(
        exec_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "execute before quorum should return 422: {:?}", exec_body,
    );
    assert!(
        exec_body["error"].is_string(),
        "error field expected: {:?}", exec_body,
    );
}

// ── Oracle + Emission API tests (Phase 45) ────────────────────────────────────

#[tokio::test]
async fn oracle_today_returns_tile_and_emission() {
    let app = make_app(make_test_state());
    let (status, body) = call_with(app, "GET", "/oracle/today", None).await;
    assert_eq!(status, StatusCode::OK, "oracle/today: {:?}", body);
    assert!(body["day"].is_u64(),       "day field: {:?}", body);
    assert!(body["tile_id"].is_string(), "tile_id: {:?}", body);
    assert!(body["tile_index"].is_u64(),"tile_index: {:?}", body);
    assert!(body["seed_hash"].is_string(),"seed_hash: {:?}", body);
    assert!(body["emission_cap"].is_u64(),"emission_cap: {:?}", body);
}

#[tokio::test]
async fn oracle_day_returns_deterministic_result() {
    let app = make_app(make_test_state());
    let (s1, b1) = call_with(app.clone(), "GET", "/oracle/day/10000", None).await;
    let (s2, b2) = call_with(app,         "GET", "/oracle/day/10000", None).await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b1["tile_id"], b2["tile_id"], "oracle must be deterministic");
    assert_eq!(b1["seed_hash"], b2["seed_hash"]);
}

#[tokio::test]
async fn emission_status_has_required_fields() {
    let app = make_app(make_test_state());
    let (status, body) = call_with(app, "GET", "/emission/status", None).await;
    assert_eq!(status, StatusCode::OK, "emission/status: {:?}", body);
    assert!(body["epoch_minute"].is_u64(),         "epoch_minute: {:?}", body);
    assert!(body["micro_ase_per_minute"].is_u64(), "micro_ase_per_minute: {:?}", body);
    assert!(body["current_difficulty"].is_f64() || body["current_difficulty"].is_number(),
        "current_difficulty: {:?}", body);
    assert!(body["is_sabbath"].is_boolean(),        "is_sabbath: {:?}", body);
    assert!(body["utxos_claimed"].is_u64(),         "utxos_claimed: {:?}", body);
    assert!(body["pending_claims"].is_u64(),        "pending_claims: {:?}", body);
}

#[tokio::test]
async fn emission_claim_queues_and_returns_accepted() {
    let app = make_app(make_test_state());
    let payload = serde_json::json!({
        "proof_id":        "test-claim-001",
        "worker_did":      "did:node:worker-1",
        "veil_id":         42,
        "trajectory_hash": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
        "f1_score":        0.92,
        "proof_value":     0.88,
        "env_hash":        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
    });
    let (status, body) = call_with(app, "POST", "/emission/claim", Some(payload)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "emission/claim: {:?}", body);
    assert_eq!(body["queued"], true);
    assert_eq!(body["proof_id"], "test-claim-001");
    assert!(body["epoch_minute"].is_u64());
}

// ── Sovereign Wallet API tests (Phase 46) ─────────────────────────────────────

#[tokio::test]
async fn wallet_credit_creates_and_returns_balance() {
    let app = make_app(make_test_state());
    let credit_body = serde_json::json!({
        "amount_micro_ase": 1_000_000u64,
        "reason":           "test credit",
    });
    let (status, body) = call_with(app, "POST", "/wallets/did%3Anode%3Aalice/credit", Some(credit_body)).await;
    assert_eq!(status, StatusCode::OK, "wallet credit: {:?}", body);
    assert_eq!(body["credited"], 1_000_000u64, "credited amount: {:?}", body);
    assert_eq!(body["balance_micro_ase"], 1_000_000u64, "balance: {:?}", body);
}

#[tokio::test]
async fn wallet_get_returns_not_found_for_unknown() {
    let app = make_app(make_test_state());
    let (status, body) = call_with(app, "GET", "/wallets/did%3Anode%3Aunknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "should 404 unknown wallet: {:?}", body);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn wallets_list_empty_initially() {
    let app = make_app(make_test_state());
    let (status, body) = call_with(app, "GET", "/wallets", None).await;
    assert_eq!(status, StatusCode::OK, "wallets list: {:?}", body);
    assert!(body["wallets"].is_array());
    assert_eq!(body["count"], 0u64);
}

// ── Proof submission tests (Phase 50) ─────────────────────────────────────────

fn sim_proof_payload(env_hash: &str) -> serde_json::Value {
    json!({
        "proof_id":          format!("sim-{env_hash}"),
        "agent_id":          "did:node:agent-test",
        "principal_id":      "did:node:principal-test",
        "simulation_id":     "sim-session-001",
        "environment_hash":  env_hash,
        "world_hash":        "0000000000000000000000000000000000000000000000000000000000000000",
        "model_hash":        "1111111111111111111111111111111111111111111111111111111111111111",
        "controller_hash":   "2222222222222222222222222222222222222222222222222222222222222222",
        "input_hash":        "3333333333333333333333333333333333333333333333333333333333333333",
        "simulator_version": "test-v1.0",
        "seed":              42u64,
        "trajectory_hash":   "4444444444444444444444444444444444444444444444444444444444444444",
        "sensor_hash":       "5555555555555555555555555555555555555555555555555555555555555555",
        "checkpoint_root":   "6666666666666666666666666666666666666666666666666666666666666666",
        "metrics": {
            "execution_time_ms":    1000u64,
            "energy_estimate":      0.5,
            "gates_cleared":        8u32,
            "gates_total":          10u32,
            "crashes":              0u32,
            "collision_margin_m":   0.3,
            "controller_stability": 0.9,
        },
        "outcome": "success",
        "timestamp":  1_700_000_000_000u64,
        "signature":  "deadbeefdeadbeef",
    })
}

fn gaussian_proof_payload(odu_tile: &str) -> serde_json::Value {
    json!({
        "proof_id":      format!("gauss-{odu_tile}"),
        "agent_id":      "did:node:agent-gauss",
        "principal_id":  "did:node:principal-gauss",
        "capture_hash":  "aaaa000000000000000000000000000000000000000000000000000000000000",
        "splat_hash":    "bbbb000000000000000000000000000000000000000000000000000000000000",
        "storage_ref":   null,
        "odu_tile":      odu_tile,
        "quality": {
            "capture_completeness": 0.90,
            "pose_quality":         0.05,
            "geometric_consistency": 0.88,
            "photometric_quality":  0.85,
            "novel_view_quality":   0.82,
            "semantic_accuracy":    0.80,
            "area_m2":              50.0,
            "witness_count":        1u32,
        },
        "timestamp": 1_700_000_000_000u64,
        "signature": "cafebabecafebabe",
    })
}

#[tokio::test]
async fn proof_simulation_submit_returns_evaluation() {
    let payload = sim_proof_payload("aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd");
    let (status, body) = call("POST", "/proofs/simulation", Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "sim proof: {:?}", body);
    assert!(body["proof_id"].is_string(), "proof_id missing: {:?}", body);
    assert!(body["quality"].is_number(), "quality missing: {:?}", body);
    assert!(body["novelty"].is_number(), "novelty missing: {:?}", body);
}

#[tokio::test]
async fn proof_simulation_get_returns_not_found() {
    // Proofs are not persisted in the MVP (in-memory node only stores the receipt).
    let (status, body) = call("GET", "/proofs/simulation/nonexistent-id", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "should 404 for unknown proof: {:?}", body);
}

#[tokio::test]
async fn proof_gaussian_submit_returns_evaluation() {
    let payload = gaussian_proof_payload("odu:55");
    let (status, body) = call("POST", "/proofs/gaussian", Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "gaussian proof: {:?}", body);
    assert!(body["proof_id"].is_string(), "proof_id: {:?}", body);
    assert!(body["quality"].is_number(), "quality: {:?}", body);
    assert_eq!(body["proof_type"], "spatial", "proof_type: {:?}", body);
}

#[tokio::test]
async fn proof_physical_below_threshold_returns_error() {
    // RTS score below MIN_RTS_FOR_PROOF (0.6) with physical_proof_eligible=false
    let payload = json!({
        "session_id":            "flight-session-low",
        "sim_proof_id":          null,
        "flight_receipt_id":     "receipt-flight-low",
        "position_accuracy":     0.20,
        "orientation_accuracy":  0.15,
        "altitude_accuracy":     0.10,
        "energy_accuracy":       0.12,
        "collision_accuracy":    0.08,
        "mission_transfer":      0.10,
        "rts":                   0.13,
        "physical_proof_eligible": false,
    });
    let (status, body) = call("POST", "/proofs/physical", Some(payload)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY,
        "expected 422 for non-eligible RTS: {:?}", body);
}

// ── Governance tests (Phase 50) ───────────────────────────────────────────────

#[tokio::test]
async fn governance_p50_create_and_list() {
    let app = make_app(make_test_state());
    let payload = json!({
        "proposer":          "did:node:council-1",
        "recipient":         "did:node:grantee-1",
        "amount_micro_ase":  500_000u64,
        "purpose":           "sensor array for Odù tile 0xAB",
        "veil_id":           1u64,
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/governance/proposals", Some(payload)).await;
    assert_eq!(s1, StatusCode::CREATED, "create proposal: {:?}", b1);
    assert!(b1["id"].is_u64(), "id missing: {:?}", b1);

    let (s2, b2) = call_with(app, "GET", "/governance/proposals", None).await;
    assert_eq!(s2, StatusCode::OK, "list proposals: {:?}", b2);
    assert!(b2["proposals"].as_array().map(|a| a.len()).unwrap_or(0) >= 1);
}

#[tokio::test]
async fn governance_vote_for_increments() {
    let app = make_app(make_test_state());
    let payload = json!({
        "proposer": "did:node:c1", "recipient": "did:node:r1",
        "amount_micro_ase": 1000u64, "purpose": "test vote", "veil_id": 1u64,
    });
    let (_, b) = call_with(app.clone(), "POST", "/governance/proposals", Some(payload)).await;
    let pid = b["id"].as_u64().unwrap();

    let (sv, bv) = call_with(app.clone(), "POST", &format!("/governance/proposals/{pid}/vote_for"), None).await;
    assert_eq!(sv, StatusCode::OK, "vote_for: {:?}", bv);

    let (sg, bg) = call_with(app, "GET", &format!("/governance/proposals/{pid}"), None).await;
    assert_eq!(sg, StatusCode::OK, "get proposal: {:?}", bg);
    assert!(bg["votes_for"].as_u64().unwrap_or(0) >= 1);
}

// ── License marketplace tests (Phase 50) ─────────────────────────────────────

#[tokio::test]
async fn license_issue_and_list() {
    let app = make_app(make_test_state());
    // IssueLicenseBody: grantee_did (required), rights (default []), expires_at, fee_mist, constraints
    let payload = json!({
        "grantee_did": "did:node:grantee-lic-1",
        "rights":      [],
        "expires_at":  null,
        "fee_mist":    null,
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/twins/twin-abc/licenses", Some(payload)).await;
    assert_eq!(s1, StatusCode::CREATED, "issue license: {:?}", b1);
    assert!(b1["grant_id"].is_string(), "grant_id missing: {:?}", b1);

    let grant_id = b1["grant_id"].as_str().unwrap().to_string();
    let encoded = urlencoding::encode(&grant_id).to_string();
    let (s2, b2) = call_with(app.clone(), "GET", &format!("/licenses/{encoded}"), None).await;
    assert_eq!(s2, StatusCode::OK, "get license: {:?}", b2);
    assert_eq!(b2["grant_id"], grant_id);

    let (s3, b3) = call_with(app, "GET", "/licenses", None).await;
    assert_eq!(s3, StatusCode::OK, "list licenses: {:?}", b3);
    assert!(b3["grants"].as_array().map(|a| a.len()).unwrap_or(0) >= 1);
}

#[tokio::test]
async fn license_accept_counter_signs() {
    let app = make_app(make_test_state());
    let payload = json!({
        "grantee_did": "did:node:grantee-acc-1",
        "rights":      [],
        "expires_at":  null,
        "fee_mist":    null,
    });
    let (sc, b) = call_with(app.clone(), "POST", "/twins/twin-xyz/licenses", Some(payload)).await;
    assert_eq!(sc, StatusCode::CREATED, "issue: {:?}", b);
    let grant_id = b["grant_id"].as_str().unwrap().to_string();
    let encoded = urlencoding::encode(&grant_id).to_string();

    let accept_body = json!({ "grantee_sig": "sig-placeholder-abc123" });
    let (sa, ba) = call_with(app, "POST", &format!("/licenses/{encoded}/accept"), Some(accept_body)).await;
    assert_eq!(sa, StatusCode::OK, "accept license: {:?}", ba);
    assert!(ba["grantee_sig"].is_string(), "grantee_sig missing: {:?}", ba);
}

// ── Body / VCP session tests (Phase 50) ──────────────────────────────────────

#[tokio::test]
async fn body_session_lifecycle() {
    let app = make_app(make_test_state());

    // Open a T4 supervised session (agent_id + body_id are required)
    // TrustTier is serde snake_case: "t4", BodySessionMode is: "HumanSupervised"
    let open_body = json!({
        "agent_id":   "did:node:agent-go2-1",
        "body_id":    "go2-body-lifecycle",
        "agent_tier": "t4",
        "mode":       "HumanSupervised",
        "capabilities": ["sensor.camera", "sensor.imu"],
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/body/sessions", Some(open_body)).await;
    assert_eq!(s1, StatusCode::CREATED, "open session: {:?}", b1);
    let session_id = b1["session_id"].as_str().unwrap().to_string();

    // Push a telemetry frame (FlightTelemetry: timestamp_ms, position [x,y,z], orientation [q], altitude_m, battery_pct, velocity_ms)
    let telem = json!({
        "timestamp_ms":  1_700_000_000_000u64,
        "position":      [0.0, 0.0, 5.0],
        "orientation":   [0.0, 0.0, 0.0, 1.0],
        "altitude_m":    5.0,
        "battery_pct":   87.0,
        "velocity_ms":   1.5,
        "obstacle_dist_m": null,
    });
    let encoded_sid = urlencoding::encode(&session_id).to_string();
    let (st, bt) = call_with(app.clone(), "POST", &format!("/body/sessions/{encoded_sid}/telemetry"), Some(telem)).await;
    assert_eq!(st, StatusCode::OK, "push telemetry: {:?}", bt);

    // Close session (mission_success required; witness_ids optional)
    let close_body = json!({ "mission_success": false, "witness_ids": [] });
    let (sc, bc) = call_with(app.clone(), "POST", &format!("/body/sessions/{encoded_sid}/close"), Some(close_body)).await;
    assert_eq!(sc, StatusCode::OK, "close session: {:?}", bc);
    assert!(bc["receipt_id"].is_string(), "receipt_id: {:?}", bc);
    assert!(bc["trajectory_hash"].is_string(), "trajectory_hash: {:?}", bc);

    // Receipts for body
    let encoded_body = urlencoding::encode("go2-body-lifecycle").to_string();
    let (sr, br) = call_with(app, "GET", &format!("/body/{encoded_body}/receipts"), None).await;
    assert_eq!(sr, StatusCode::OK, "body receipts: {:?}", br);
    assert!(br["receipts"].as_array().map(|a| a.len()).unwrap_or(0) >= 1);
}

#[tokio::test]
async fn body_sessions_list_empty_initially() {
    let (status, body) = call("GET", "/body/sessions", None).await;
    assert_eq!(status, StatusCode::OK, "body sessions: {:?}", body);
    assert!(body["sessions"].is_array());
}

#[tokio::test]
async fn body_capabilities_returns_list() {
    let (status, body) = call("GET", "/body/capabilities", None).await;
    assert_eq!(status, StatusCode::OK, "capabilities: {:?}", body);
    assert!(body["capabilities"].is_array(), "no capabilities array: {:?}", body);
}
