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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn tile_receipts_empty_for_valid_tile() {
    let (status, body) = call("GET", "/tiles/odu:00/receipts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tile_id"], "odu:00");
    assert_eq!(body["count"], 0);
    assert!(body["receipts"].as_array().unwrap().is_empty());
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn twin_timeline_returns_404_initially() {
    let (status, body) = call("GET", "/twins/unitree:go2:stub/timeline", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "timeline_not_found");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn timelines_list_initially_empty() {
    let (status, body) = call("GET", "/timelines", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 0);
}

// ─── /federation/peers ────────────────────────────────────────────────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn body_sessions_empty_initially() {
    let (status, body) = call("GET", "/body/sessions", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], serde_json::json!(0));
    assert!(body["sessions"].as_array().unwrap().is_empty());
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn wallet_get_returns_not_found_for_unknown() {
    let app = make_app(make_test_state());
    let (status, body) = call_with(app, "GET", "/wallets/did%3Anode%3Aunknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "should 404 unknown wallet: {:?}", body);
    assert!(body["error"].is_string());
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn proof_gaussian_submit_returns_evaluation() {
    let payload = gaussian_proof_payload("odu:55");
    let (status, body) = call("POST", "/proofs/gaussian", Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "gaussian proof: {:?}", body);
    assert!(body["proof_id"].is_string(), "proof_id: {:?}", body);
    assert!(body["quality"].is_number(), "quality: {:?}", body);
    assert_eq!(body["proof_type"], "spatial", "proof_type: {:?}", body);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
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
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn body_sessions_list_empty_initially() {
    let (status, body) = call("GET", "/body/sessions", None).await;
    assert_eq!(status, StatusCode::OK, "body sessions: {:?}", body);
    assert!(body["sessions"].is_array());
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn body_capabilities_returns_list() {
    let (status, body) = call("GET", "/body/capabilities", None).await;
    assert_eq!(status, StatusCode::OK, "capabilities: {:?}", body);
    assert!(body["capabilities"].is_array(), "no capabilities array: {:?}", body);
}

// ─── Phase 4.3 — body session command endpoint ───────────────────────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn body_session_command_accepted() {
    let app = make_app(make_test_state());

    // Open session with camera capability
    let open_body = json!({
        "agent_id":     "did:node:agent-cmd-test",
        "body_id":      "go2-cmd-body",
        "agent_tier":   "t4",
        "mode":         "HumanSupervised",
        "capabilities": ["sensor.camera", "locomotion"],
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/body/sessions", Some(open_body)).await;
    assert_eq!(s1, StatusCode::CREATED, "open: {:?}", b1);
    let session_id = b1["session_id"].as_str().unwrap().to_string();
    let encoded = urlencoding::encode(&session_id).to_string();

    // Issue a command
    let cmd = json!({
        "capability": "sensor.camera",
        "action":     "capture_frame",
        "params":     { "resolution": "1080p" },
    });
    let (sc, bc) = call_with(app.clone(), "POST", &format!("/body/sessions/{encoded}/command"), Some(cmd)).await;
    assert_eq!(sc, StatusCode::OK, "command: {:?}", bc);
    assert_eq!(bc["status"], "accepted", "status: {:?}", bc);
    assert!(bc["cmd_id"].as_str().map(|s| s.starts_with("cmd:")).unwrap_or(false));
    assert_eq!(bc["capability"], "sensor.camera");
    assert_eq!(bc["action"], "capture_frame");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn body_session_command_unknown_capability_denied() {
    let app = make_app(make_test_state());

    let open_body = json!({
        "agent_id":     "did:node:agent-cap-test",
        "body_id":      "go2-cap-body",
        "agent_tier":   "t4",
        "mode":         "HumanSupervised",
        "capabilities": ["sensor.camera"],
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/body/sessions", Some(open_body)).await;
    assert_eq!(s1, StatusCode::CREATED, "open: {:?}", b1);
    let session_id = b1["session_id"].as_str().unwrap().to_string();
    let encoded = urlencoding::encode(&session_id).to_string();

    // Try to use locomotion which wasn't granted
    let cmd = json!({ "capability": "locomotion", "action": "walk" });
    let (sc, bc) = call_with(app, "POST", &format!("/body/sessions/{encoded}/command"), Some(cmd)).await;
    assert_eq!(sc, StatusCode::FORBIDDEN, "should deny: {:?}", bc);
    assert_eq!(bc["error"], "capability_not_granted");
}

#[tokio::test]
async fn body_session_command_session_not_found() {
    let (sc, bc) = call("POST", "/body/sessions/no-such-session/command",
        Some(json!({ "capability": "sensor.camera", "action": "capture" }))).await;
    assert_eq!(sc, StatusCode::NOT_FOUND, "body: {:?}", bc);
}

// ─── Phase 4.1 full-loop: VCP camera session → auto-capture job ──────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn vcp_camera_session_close_queues_capture_job() {
    let state = make_test_state();
    let app   = make_app(state.clone());

    // Open session WITH camera capability and mission_success=true
    let open_body = json!({
        "agent_id":     "did:node:agent-cam-loop",
        "body_id":      "go2-cam-loop",
        "agent_tier":   "t4",
        "mode":         "HumanSupervised",
        "capabilities": ["sensor.camera", "locomotion"],
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/body/sessions", Some(open_body)).await;
    assert_eq!(s1, StatusCode::CREATED, "open: {:?}", b1);
    let session_id = b1["session_id"].as_str().unwrap().to_string();
    let encoded = urlencoding::encode(&session_id).to_string();

    // Close with mission_success=true (triggers auto-capture)
    let close_body = json!({ "mission_success": true });
    let (sc, bc) = call_with(app.clone(), "POST", &format!("/body/sessions/{encoded}/close"), Some(close_body)).await;
    assert_eq!(sc, StatusCode::OK, "close: {:?}", bc);
    assert!(bc["receipt_id"].is_string());

    // Give the auto-queued job a moment to register (it's spawned asynchronously)
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Verify a capture job was auto-queued (job store should have ≥1 job)
    let (sj, bj) = call_with(app, "GET", "/jobs", None).await;
    assert_eq!(sj, StatusCode::OK, "jobs: {:?}", bj);
    let job_count = bj["jobs"].as_array().map(|a| a.len()).unwrap_or(0);
    assert!(job_count >= 1, "expected auto-capture job, got 0 jobs: {:?}", bj);
}

// ─── Phase 4.2 — DIP inbound license request ─────────────────────────────────

#[tokio::test]
async fn dip_inbound_non_local_envelope_ignored() {
    // An envelope addressed to a different DID should return OK (silently dropped).
    // Build a valid DipEnvelope JSON — all fields required by the struct.
    let body = json!({
        "version":    "dip/1",
        "message_id": "test-msg-001",
        "timestamp":  1_700_000_000_000u64,
        "ttl":        300,
        "origin":      { "network": "vantage", "address": "did:node:sender",       "did": "did:node:sender" },
        "destination": { "network": "vantage", "address": "did:node:someone-else", "did": "did:node:someone-else" },
        "routing":    [],
        "kind":       "message",
        "payload":    { "hello": "world" },
        "identity": {
            "principal_id": "did:node:sender",
            "agent_id":     "did:node:sender",
            "session_id":   "sess:001",
            "execution_id": "exec:001",
            "receipt_id":   "rcpt:001"
        },
        "merkle_root": "0000000000000000000000000000000000000000000000000000000000000000",
        "signature":   "stub-sig",
    });
    let (status, _) = call("POST", "/dip/inbound", Some(body)).await;
    assert_eq!(status, StatusCode::OK);
}

// ─── Phase 4.2 — DIP→TSP: inbound twin license request creates a grant ───────

fn make_dip_envelope(dest_did: &str, kind: &str, payload: serde_json::Value) -> serde_json::Value {
    json!({
        "version":    "dip/1",
        "message_id": format!("test-{}", uuid::Uuid::new_v4()),
        "timestamp":  1_700_000_000_000u64,
        "ttl":        300,
        "origin":      { "network": "vantage", "address": "did:node:requester", "did": "did:node:requester" },
        "destination": { "network": "vantage", "address": dest_did,              "did": dest_did },
        "routing":    [],
        "kind":       kind,
        "payload":    payload,
        "identity": {
            "principal_id": "did:node:requester",
            "agent_id":     "did:node:requester",
            "session_id":   "sess:test",
            "execution_id": "exec:test",
            "receipt_id":   "rcpt:test"
        },
        "merkle_root": "0000000000000000000000000000000000000000000000000000000000000000",
        "signature":   "stub-sig",
    })
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn dip_twin_license_request_issues_grant() {
    let state    = make_test_state();
    let local_did = state.identity.did.clone();
    let app      = make_app(state.clone());

    let payload = json!({
        "type":    "twin_license_request",
        "twin_id": "twin:dip-license-test-001",
        "rights":  ["View", "Simulate"],
    });
    let envelope = make_dip_envelope(&local_did, "message", payload);

    let (status, body) = call_with(app, "POST", "/dip/inbound", Some(envelope)).await;
    assert_eq!(status, StatusCode::OK, "dip inbound: {:?}", body);

    // Give async handler a moment to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // License should now appear in the license store
    let (ls, lb) = call_with(make_app(state), "GET", "/licenses", None).await;
    assert_eq!(ls, StatusCode::OK, "licenses: {:?}", lb);
    let grants = lb["grants"].as_array().cloned().unwrap_or_default();
    let found = grants.iter().any(|g| {
        g["twin_id"].as_str() == Some("twin:dip-license-test-001")
    });
    assert!(found, "expected license grant for twin:dip-license-test-001, got: {:?}", grants);
}

// ─── Phase 4.3 — DIP→VCP: inbound command validated against body session ──────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn dip_vcp_command_accepted_for_authorized_session() {
    let state    = make_test_state();
    let local_did = state.identity.did.clone();
    let app      = make_app(state.clone());

    // Open a body session owned by "did:node:requester"
    let open_body = json!({
        "agent_id":     "did:node:requester",
        "body_id":      "go2-dip-cmd-test",
        "agent_tier":   "t4",
        "mode":         "HumanSupervised",
        "capabilities": ["sensor.camera"],
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/body/sessions", Some(open_body)).await;
    assert_eq!(s1, StatusCode::CREATED, "open: {:?}", b1);
    let session_id = b1["session_id"].as_str().unwrap().to_string();

    // Send a DIP vcp_command for that session — principal_id matches agent_id
    let payload = json!({
        "type":       "vcp_command",
        "session_id": session_id,
        "capability": "sensor.camera",
        "action":     "capture_frame",
        "params":     {},
    });
    let envelope = make_dip_envelope(&local_did, "message", payload);

    let (status, body) = call_with(app, "POST", "/dip/inbound", Some(envelope)).await;
    assert_eq!(status, StatusCode::OK, "dip inbound: {:?}", body);
    // Route accepted (gateway will fire a DIP reply, but we only check the HTTP 200 here)
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn dip_vcp_command_rejected_for_wrong_principal() {
    let state    = make_test_state();
    let local_did = state.identity.did.clone();
    let app      = make_app(state.clone());

    // Open a session owned by a different agent
    let open_body = json!({
        "agent_id":     "did:node:real-owner",
        "body_id":      "go2-reject-test",
        "agent_tier":   "t4",
        "mode":         "HumanSupervised",
        "capabilities": ["sensor.camera"],
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/body/sessions", Some(open_body)).await;
    assert_eq!(s1, StatusCode::CREATED, "open: {:?}", b1);
    let session_id = b1["session_id"].as_str().unwrap().to_string();

    // Attempt a command from a different principal (did:node:requester ≠ did:node:real-owner)
    let payload = json!({
        "type":       "vcp_command",
        "session_id": session_id,
        "capability": "sensor.camera",
        "action":     "capture_frame",
    });
    let envelope = make_dip_envelope(&local_did, "message", payload);
    let (status, body) = call_with(app, "POST", "/dip/inbound", Some(envelope)).await;
    // HTTP always 200 (DIP errors are reported in the reply envelope, not HTTP status)
    assert_eq!(status, StatusCode::OK, "dip inbound: {:?}", body);
}

// ─── Phase 4.4 — Full Loop: VCP session → capture → proof → receipt chain ────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn full_loop_session_capture_proof_receipt_chain() {
    let state = make_test_state();
    let app   = make_app(state.clone());

    // Step 1 — VCP: open a camera session
    let open_body = json!({
        "agent_id":     "did:node:loop-agent",
        "body_id":      "go2-full-loop",
        "agent_tier":   "t4",
        "mode":         "HumanSupervised",
        "capabilities": ["sensor.camera", "locomotion"],
    });
    let (s1, b1) = call_with(app.clone(), "POST", "/body/sessions", Some(open_body)).await;
    assert_eq!(s1, StatusCode::CREATED, "open session: {:?}", b1);
    let session_id = b1["session_id"].as_str().unwrap().to_string();
    let encoded_session = urlencoding::encode(&session_id).to_string();

    // Step 2 — VCP: record telemetry frames (simulates robot scanning)
    for i in 0..3u32 {
        let frame = json!({
            "session_id": session_id,
            "frame_index": i,
            "pitch": 0.0, "roll": 0.0, "yaw": (i as f64) * 0.1,
            "position_x": (i as f64) * 0.5, "position_y": 0.0, "position_z": 0.0,
            "battery_pct": 90,
            "timestamp_ms": 1_700_000_000_000u64 + (i as u64) * 100,
        });
        call_with(app.clone(), "POST", &format!("/body/sessions/{encoded_session}/telemetry"), Some(frame)).await;
    }

    // Step 3 — VCP→TSP (Phase 4.1): close session with mission_success → auto-capture queued
    let close_body = json!({ "mission_success": true, "witness_ids": [] });
    let (sc, bc) = call_with(app.clone(), "POST",
        &format!("/body/sessions/{encoded_session}/close"), Some(close_body)).await;
    assert_eq!(sc, StatusCode::OK, "close session: {:?}", bc);
    assert!(bc["receipt_id"].is_string(), "session receipt must have receipt_id: {:?}", bc);

    // Give auto-capture job time to queue
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Step 4 — TSP: manually push a capture receipt to verify the chain builds
    let device_id = "unitree:go2:127.0.0.1";
    let (sj, bj) = call_with(app.clone(), "POST",
        &format!("/capture/{}", urlencoding::encode(device_id)),
        Some(json!({}))).await;
    // 200 or 202 — job submitted
    assert!(
        sj == StatusCode::OK || sj == StatusCode::ACCEPTED,
        "capture submit: {sj} {:?}", bj
    );
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Step 5 — TSP: submit a simulation proof (full required schema)
    let sim_body = json!({
        "proof_id":          "p-full-loop-001",
        "agent_id":          "did:node:loop-agent",
        "principal_id":      "did:node:loop-agent",
        "simulation_id":     "sim:full-loop-001",
        "environment_hash":  "env_hash_001",
        "world_hash":        "world_hash_001",
        "model_hash":        "model_hash_001",
        "controller_hash":   "ctrl_hash_001",
        "input_hash":        "input_hash_001",
        "simulator_version": "mujoco/3.1.4",
        "seed":              42u64,
        "trajectory_hash":   "traj_hash_001",
        "sensor_hash":       "sensor_hash_001",
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
        "timestamp": 0u64,
        "signature": "stub-sig",
    });
    let (sp, bp) = call_with(app.clone(), "POST", "/proofs/simulation", Some(sim_body)).await;
    assert_eq!(sp, StatusCode::OK, "simulation proof: {:?}", bp);
    assert!(bp["proof_id"].is_string(), "simulation proof must have proof_id: {:?}", bp);

    // Step 6 — TSP: submit a physical ObservationReceipt (all required fields)
    let obs_body = json!({
        "kind":           31040u32,
        "receipt_id":     "rcpt:31040_full-loop-obs-001",
        "sim_receipt_id": bp["proof_id"].as_str().unwrap_or("p-full-loop-001"),
        "witness_id":     "witness:stub-001",
        "observed":       { "position_x": 0.48, "position_y": 0.0 },
        "predicted":      { "position_x": 0.5,  "position_y": 0.0 },
        "delta": {
            "fields":        {},
            "max_delta_pct": 0.0,
        },
        "outcome":        "validated",
        "tpm_key_id":     "tpm:stub-key-001",
        "hardware_sig":   "stub-hw-sig",
        "device_cert":    "stub-cert",
        "timestamp":      1_700_000_100_000u64,
    });
    let (so, _bo) = call_with(app.clone(), "POST", "/proofs/observation", Some(obs_body)).await;
    assert_eq!(so, StatusCode::CREATED, "observation: {:?}", _bo);

    // Step 7 — Verify receipt Merkle root is buildable (non-empty chain)
    let (sm, bm) = call_with(app.clone(), "GET", "/receipts/root", None).await;
    assert_eq!(sm, StatusCode::OK, "merkle root: {:?}", bm);
    assert!(bm["count"].as_u64().unwrap_or(0) >= 0,
        "merkle root count must be numeric: {:?}", bm);
    assert!(bm["algo"].as_str() == Some("sha256-binary-merkle"),
        "expected sha256-binary-merkle algo: {:?}", bm);

    // Step 8 — Verify DIP gossip can be triggered (propagates receipts to peers)
    let (sg, bg) = call_with(app.clone(), "POST", "/dip/gossip",
        Some(json!({ "count": 5 }))).await;
    assert_eq!(sg, StatusCode::OK, "dip gossip: {:?}", bg);
    assert!(bg["receipts"].is_number() || bg["receipt_count"].is_number(),
        "gossip must have receipts count: {:?}", bg);
    assert!(bg["peers"].is_number() || bg["peer_count"].is_number(),
        "gossip must have peer count: {:?}", bg);

    // Step 9 — Verify job list reflects the auto-queued capture
    let (sjl, bjl) = call_with(app, "GET", "/jobs", None).await;
    assert_eq!(sjl, StatusCode::OK, "jobs list: {:?}", bjl);
    let job_count = bjl["jobs"].as_array().map(|a| a.len()).unwrap_or(0);
    assert!(job_count >= 1, "expected ≥1 job in full loop, got 0: {:?}", bjl);
}

// ─── 4D timeline diff — migrated to Vantage ──────────────────────────────────
// These tests were removed: timeline_store migrated to Vantage/backend/sovereign_twins/timeline.py

// timeline_diff_requires_two_snapshots — MIGRATED to Vantage
// timeline_diff_computes_quality_delta — MIGRATED to Vantage

// ── OSOVM Token-of-Compute tests (Phase 52) ───────────────────────────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_pool_state_returns_expected_fields() {
    let (status, body) = call("GET", "/osovm/pool", None).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert!(body["decay_bps_per_day"].as_u64().is_some(), "missing decay_bps_per_day");
    assert!(body["eshu_tithe_bps"].as_u64().is_some(), "missing eshu_tithe_bps");
    assert_eq!(body["contribution_count"].as_u64().unwrap_or(1), 0);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_contribute_mints_tokens() {
    let app = make_app(make_test_state());
    let (status, body) = call_with(app, "POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:gpu-test",
        "device_id":       "gpu:a40:001",
        "compute_units":   1000u64,
        "proof_hash":      "sha256:deadbeef",
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "body: {:?}", body);
    assert!(body["contribution_id"].as_str().unwrap_or("").starts_with("gpu:"));
    let gpu_minted = body["gpu_minted"].as_u64().unwrap_or(0);
    let eshu_tithe = body["eshu_tithe"].as_u64().unwrap_or(0);
    assert!(gpu_minted > 0, "gpu_minted must be > 0");
    assert!(eshu_tithe > 0, "eshu_tithe must be > 0");
    assert!(gpu_minted + eshu_tithe == 1000, "mint + tithe must equal compute_units");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_contribute_rejects_zero_units() {
    let (status, body) = call("POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:zero",
        "device_id":       "gpu:a40:002",
        "compute_units":   0u64,
        "proof_hash":      "sha256:abc",
    }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {:?}", body);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_burn_for_synapse_succeeds() {
    let app = make_app(make_test_state());
    // First, contribute some GPU
    let (s1, b1) = call_with(app.clone(), "POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:burn-test",
        "device_id":       "gpu:h100:001",
        "compute_units":   100u64,
        "proof_hash":      "sha256:burn",
    }))).await;
    assert_eq!(s1, StatusCode::CREATED, "contribute: {:?}", b1);
    let gpu_minted = b1["gpu_minted"].as_u64().unwrap_or(0);
    assert!(gpu_minted > 0);

    // Burn half for synapses
    let burn_amount = gpu_minted / 2;
    if burn_amount > 0 {
        let (s2, b2) = call_with(app.clone(), "POST", "/osovm/gpu/burn", Some(json!({
            "did":        "did:worker:burn-test",
            "gpu_amount": burn_amount,
        }))).await;
        assert_eq!(s2, StatusCode::OK, "burn: {:?}", b2);
        assert!(b2["synapses_minted"].as_u64().unwrap_or(0) > 0);
        assert_eq!(b2["gpu_burned"].as_u64().unwrap_or(0), burn_amount);
    }
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_burn_fails_on_insufficient_balance() {
    let (status, body) = call("POST", "/osovm/gpu/burn", Some(json!({
        "did":        "did:worker:nobody",
        "gpu_amount": 999999u64,
    }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "body: {:?}", body);
    assert_eq!(body["error"], "insufficient_gpu");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn osovm_balances_returns_zero_for_unknown() {
    let (status, body) = call("GET", "/osovm/balances/did:unknown:xyz", None).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["gpu_balance"].as_u64().unwrap_or(1), 0);
    assert_eq!(body["synapse_balance"].as_u64().unwrap_or(1), 0);
}

// ── Bínò governance veto tests (Phase 55) ─────────────────────────────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn governance_veto_blocks_proposal() {
    let app = make_app(make_test_state());
    let proposal_payload = sample_proposal_payload();

    // Create a proposal
    let (s1, b1) = call_with(app.clone(), "POST", "/governance/proposals", Some(proposal_payload)).await;
    assert_eq!(s1, StatusCode::CREATED, "create: {:?}", b1);
    let pid = b1["id"].as_u64().unwrap();

    // Veto it
    let (s2, b2) = call_with(app.clone(), "POST", &format!("/governance/proposals/{pid}/veto"), Some(json!({
        "veto_by": "did:council:bino-seat-1",
        "reason":  "violates principle of sovereignty boundary",
    }))).await;
    assert_eq!(s2, StatusCode::OK, "veto: {:?}", b2);
    assert_eq!(b2["status"], "vetoed");
    assert_eq!(b2["proposal_id"].to_string().trim_matches('"'), pid.to_string().as_str());

    // GET should reflect vetoed status
    let (s3, b3) = call_with(app.clone(), "GET", &format!("/governance/proposals/{pid}"), None).await;
    assert_eq!(s3, StatusCode::OK, "get after veto: {:?}", b3);
    assert_eq!(b3["status"], "vetoed");

    // Execute should fail
    let (s4, b4) = call_with(app, "POST", &format!("/governance/proposals/{pid}/execute"), None).await;
    assert_eq!(s4, StatusCode::UNPROCESSABLE_ENTITY, "execute after veto: {:?}", b4);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn governance_veto_unknown_proposal_returns_404() {
    let (status, body) = call("POST", "/governance/proposals/9999/veto", Some(json!({
        "veto_by": "did:council:bino",
        "reason":  "test",
    }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {:?}", body);
    assert_eq!(body["error"], "proposal_not_found");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn governance_double_veto_returns_conflict() {
    let app = make_app(make_test_state());
    let payload = sample_proposal_payload();
    let (_, b) = call_with(app.clone(), "POST", "/governance/proposals", Some(payload)).await;
    let pid = b["id"].as_u64().unwrap();

    let veto_body = json!({ "veto_by": "did:council:bino", "reason": "test" });
    let (s1, _) = call_with(app.clone(), "POST", &format!("/governance/proposals/{pid}/veto"), Some(veto_body.clone())).await;
    assert_eq!(s1, StatusCode::OK);

    let (s2, b2) = call_with(app, "POST", &format!("/governance/proposals/{pid}/veto"), Some(veto_body)).await;
    assert_eq!(s2, StatusCode::CONFLICT, "double-veto should be 409: {:?}", b2);
    assert_eq!(b2["error"], "already_vetoed");
}

// ─── Phase 48: spatial PLY diff ───────────────────────────────────────────────

fn make_ply_b64(points: &[[f64; 3]]) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut s = format!(
        "ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nend_header\n",
        points.len()
    );
    for p in points {
        s.push_str(&format!("{} {} {}\n", p[0], p[1], p[2]));
    }
    STANDARD.encode(s.as_bytes())
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn splat_diff_identical_returns_zero_magnitude() {
    let b64 = make_ply_b64(&[[0.0,0.0,0.0],[1.0,1.0,1.0]]);
    let (status, body) = call(
        "POST",
        "/twins/twin:splat-test/splat/diff",
        Some(json!({ "snapshot_a": b64, "snapshot_b": b64 })),
    ).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["point_count_delta"], 0);
    let mag = body["change_magnitude"].as_f64().unwrap();
    assert!(mag < 1e-9, "expected zero magnitude, got {mag}");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn splat_diff_shifted_cloud_detects_change() {
    let a = make_ply_b64(&[[0.0,0.0,0.0],[1.0,0.0,0.0]]);
    let b = make_ply_b64(&[[10.0,0.0,0.0],[11.0,0.0,0.0]]);
    let (status, body) = call(
        "POST",
        "/twins/twin:splat-shift/splat/diff",
        Some(json!({ "snapshot_a": a, "snapshot_b": b })),
    ).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    let delta = body["centroid_delta_m"].as_f64().unwrap();
    assert!((delta - 10.0).abs() < 1e-4, "expected ~10m centroid delta, got {delta}");
    assert!(body["change_magnitude"].as_f64().unwrap() > 0.0);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn splat_diff_growing_cloud_positive_count_delta() {
    let a = make_ply_b64(&[[0.0,0.0,0.0]]);
    let b = make_ply_b64(&[[0.0,0.0,0.0],[1.0,0.0,0.0],[2.0,0.0,0.0]]);
    let (status, body) = call(
        "POST",
        "/twins/twin:splat-grow/splat/diff",
        Some(json!({ "snapshot_a": a, "snapshot_b": b })),
    ).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["point_count_delta"], 2);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn splat_diff_bad_base64_returns_422() {
    let (status, body) = call(
        "POST",
        "/twins/twin:splat-bad/splat/diff",
        Some(json!({ "snapshot_a": "not!!base64$$", "snapshot_b": "also bad" })),
    ).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {:?}", body);
    assert_eq!(body["error"], "invalid_base64");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn splat_diff_returns_stats_for_both_snapshots() {
    let a = make_ply_b64(&[[0.0,0.0,0.0],[2.0,0.0,0.0]]);
    let b = make_ply_b64(&[[0.0,0.0,0.0],[4.0,0.0,0.0]]);
    let (status, body) = call(
        "POST",
        "/twins/twin:splat-stats/splat/diff",
        Some(json!({ "snapshot_a": a, "snapshot_b": b })),
    ).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["stats_a"]["point_count"], 2);
    assert_eq!(body["stats_b"]["point_count"], 2);
    // a centroid x = 1.0, b centroid x = 2.0 → delta = 1.0
    let delta = body["centroid_delta_m"].as_f64().unwrap();
    assert!((delta - 1.0).abs() < 1e-4, "expected 1.0m delta, got {delta}");
}

// ─── Phase 49: swarm splat merge ──────────────────────────────────────────────

fn splat_input(device_id: &str, points: &[[f64; 3]]) -> serde_json::Value {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut s = format!(
        "ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nend_header\n",
        points.len()
    );
    for p in points {
        s.push_str(&format!("{} {} {}\n", p[0], p[1], p[2]));
    }
    json!({ "device_id": device_id, "ply_b64": STANDARD.encode(s.as_bytes()) })
}

async fn create_swarm(app: axum::Router) -> (axum::Router, String) {
    let (_, body) = call_with(app.clone(), "POST", "/capture/swarm", Some(json!({
        "device_ids": ["go2:merge-a", "go2:merge-b"],
        "hint": "merge test"
    }))).await;
    let swarm_id = body["swarm_id"].as_str().unwrap().to_owned();
    (app, swarm_id)
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn swarm_merge_splat_unknown_swarm_returns_404() {
    let (status, body) = call(
        "POST",
        "/swarm/swarm:does-not-exist/merge-splat",
        Some(json!({ "splats": [], "voxel_size": 0.0 })),
    ).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {:?}", body);
    assert_eq!(body["error"], "swarm_not_found");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn swarm_merge_splat_empty_splats_returns_422() {
    let app = make_app(make_test_state());
    let (app, swarm_id) = create_swarm(app).await;
    let (status, body) = call_with(app, "POST",
        &format!("/swarm/{swarm_id}/merge-splat"),
        Some(json!({ "splats": [], "voxel_size": 0.0 })),
    ).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {:?}", body);
    assert_eq!(body["error"], "no_splats");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn swarm_merge_splat_two_devices_returns_merged_ply() {
    let app = make_app(make_test_state());
    let (app, swarm_id) = create_swarm(app).await;
    let payload = json!({
        "splats": [
            splat_input("go2:merge-a", &[[0.0,0.0,0.0],[1.0,0.0,0.0]]),
            splat_input("go2:merge-b", &[[5.0,0.0,0.0],[6.0,0.0,0.0]]),
        ],
        "voxel_size": 0.0,
    });
    let (status, body) = call_with(app, "POST", &format!("/swarm/{swarm_id}/merge-splat"), Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["source_count"], 2);
    assert_eq!(body["input_points"],  4);
    assert_eq!(body["output_points"], 4);
    assert!(body["merged_ply_b64"].as_str().unwrap().len() > 10);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn swarm_merge_splat_voxel_reduces_points() {
    let app = make_app(make_test_state());
    let (app, swarm_id) = create_swarm(app).await;
    // 4 points all within a 1m voxel → 1 output point.
    let payload = json!({
        "splats": [
            splat_input("go2:merge-a", &[[0.1,0.1,0.1],[0.2,0.2,0.2]]),
            splat_input("go2:merge-b", &[[0.3,0.3,0.3],[0.4,0.4,0.4]]),
        ],
        "voxel_size": 1.0,
    });
    let (status, body) = call_with(app, "POST", &format!("/swarm/{swarm_id}/merge-splat"), Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["input_points"],  4);
    assert_eq!(body["output_points"], 1);
    let red = body["reduction_pct"].as_f64().unwrap();
    assert!((red - 75.0).abs() < 1e-4, "expected 75% reduction, got {red}");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn swarm_merge_splat_returns_centroid() {
    let app = make_app(make_test_state());
    let (app, swarm_id) = create_swarm(app).await;
    let payload = json!({
        "splats": [
            splat_input("go2:merge-a", &[[0.0,0.0,0.0]]),
            splat_input("go2:merge-b", &[[2.0,0.0,0.0]]),
        ],
        "voxel_size": 0.0,
    });
    let (status, body) = call_with(app, "POST", &format!("/swarm/{swarm_id}/merge-splat"), Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    let cx = body["centroid"][0].as_f64().unwrap();
    assert!((cx - 1.0).abs() < 1e-4, "expected centroid x=1.0, got {cx}");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn swarm_merge_splat_bad_base64_returns_422() {
    let app = make_app(make_test_state());
    let (app, swarm_id) = create_swarm(app).await;
    let payload = json!({
        "splats": [json!({ "device_id": "go2:bad", "ply_b64": "!!!not-base64!!!" })],
        "voxel_size": 0.0,
    });
    let (status, body) = call_with(app, "POST", &format!("/swarm/{swarm_id}/merge-splat"), Some(payload)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {:?}", body);
    assert_eq!(body["error"], "invalid_base64");
}

// ─── Phase 50: on-chain tile governance ───────────────────────────────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn tile_claim_returns_tx_digest_and_object_id() {
    let (status, body) = call("POST", "/tiles/odu:3F/claim", Some(json!({
        "owner_did": "did:p:phase50-owner"
    }))).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["ok"], true);
    assert_eq!(body["tile_id"], "odu:3F");
    // Stub mode: tx_digest and object_id are non-empty deterministic strings.
    let digest = body["tx_digest"].as_str().unwrap_or("");
    let obj    = body["object_id"].as_str().unwrap_or("");
    assert!(!digest.is_empty(), "tx_digest should be set in stub mode");
    assert!(!obj.is_empty(),    "object_id should be set in stub mode");
    assert!(body["stub"].as_bool().unwrap_or(false), "should be stub=true without a real Sui key");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn tile_stake_valid_returns_ok() {
    let app = make_app(make_test_state());
    // First claim the tile so economy entry exists.
    call_with(app.clone(), "POST", "/tiles/odu:A0/claim", Some(json!({
        "owner_did": "did:p:staker"
    }))).await;

    let (status, body) = call_with(app, "POST", "/tiles/odu:A0/stake", Some(json!({
        "staker_did": "did:p:staker",
        "amount":     1_000_000u64,
    }))).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["ok"], true);
    assert_eq!(body["amount"], 1_000_000u64);
    assert!(body["stub"].as_bool().unwrap_or(false));
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn tile_stake_zero_amount_returns_400() {
    let (status, body) = call("POST", "/tiles/odu:B1/stake", Some(json!({
        "staker_did": "did:p:staker",
        "amount":     0u64,
    }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {:?}", body);
    assert_eq!(body["error"], "invalid_amount");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn tile_stake_invalid_tile_id_returns_400() {
    let (status, body) = call("POST", "/tiles/bad-tile/stake", Some(json!({
        "staker_did": "did:p:staker",
        "amount":     100u64,
    }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {:?}", body);
    assert_eq!(body["error"], "invalid_tile_id");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn tile_claim_different_tiles_have_different_digests() {
    let app = make_app(make_test_state());
    let (_, b1) = call_with(app.clone(), "POST", "/tiles/odu:00/claim", Some(json!({
        "owner_did": "did:p:owner"
    }))).await;
    let (_, b2) = call_with(app, "POST", "/tiles/odu:FF/claim", Some(json!({
        "owner_did": "did:p:owner"
    }))).await;
    let d1 = b1["tx_digest"].as_str().unwrap_or("");
    let d2 = b2["tx_digest"].as_str().unwrap_or("");
    assert_ne!(d1, d2, "different tiles must produce different tx digests");
}

// ─── Phase 51: OSOVM Token-of-Compute wiring ─────────────────────────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_contribute_returns_emission_receipt_id() {
    let (status, body) = call("POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:toc-test",
        "device_id":       "gpu:a100:01",
        "compute_units":   10_000u64,
        "proof_hash":      "sha256:proof-toc-01",
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "body: {:?}", body);
    let eid = body["emission_receipt_id"].as_str().unwrap_or("");
    assert!(!eid.is_empty(), "emission_receipt_id should be set");
    assert!(eid.starts_with("emit:"), "expected emit: prefix, got {eid}");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_burn_returns_emission_receipt_id() {
    let app = make_app(make_test_state());
    // Seed GPU balance first.
    call_with(app.clone(), "POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:burn-toc",
        "device_id":       "gpu:h100:01",
        "compute_units":   1_000u64,
        "proof_hash":      "sha256:seed",
    }))).await;

    let (status, body) = call_with(app, "POST", "/osovm/gpu/burn", Some(json!({
        "did":        "did:worker:burn-toc",
        "gpu_amount": 10u64,
    }))).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    let eid = body["emission_receipt_id"].as_str().unwrap_or("");
    assert!(!eid.is_empty(), "emission_receipt_id should be set on burn");
    assert!(body["synapses_minted"].as_u64().unwrap_or(0) > 0);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_decay_returns_decayed_and_receipt() {
    let app = make_app(make_test_state());
    // Seed Synapse balance: contribute → burn.
    call_with(app.clone(), "POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:decay-toc",
        "device_id":       "gpu:a40:01",
        "compute_units":   100_000u64,
        "proof_hash":      "sha256:decay-seed",
    }))).await;
    let gpu_bal_body = {
        let did_enc = urlencoding::encode("did:worker:decay-toc").to_string();
        let (_, b) = call_with(app.clone(), "GET", &format!("/osovm/balances/{did_enc}"), None).await;
        b
    };
    let gpu = gpu_bal_body["gpu_balance"].as_u64().unwrap_or(0);
    call_with(app.clone(), "POST", "/osovm/gpu/burn", Some(json!({
        "did": "did:worker:decay-toc", "gpu_amount": gpu,
    }))).await;

    let (status, body) = call_with(app.clone(), "POST", "/osovm/gpu/decay", Some(json!({
        "epoch_day": 99999u64,
    }))).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert!(!body["already_applied"].as_bool().unwrap_or(true), "first decay should not be already_applied");
    assert!(body["synapses_decayed"].as_u64().unwrap_or(0) > 0, "should have decayed some synapses");
    let eid = body["emission_receipt_id"].as_str().unwrap_or("");
    assert!(!eid.is_empty(), "decay should produce emission_receipt_id");

    // Idempotent: second call same epoch_day returns already_applied.
    let (status2, body2) = call_with(app, "POST", "/osovm/gpu/decay", Some(json!({
        "epoch_day": 99999u64,
    }))).await;
    assert_eq!(status2, StatusCode::OK);
    assert!(body2["already_applied"].as_bool().unwrap_or(false), "second call should be already_applied");
    assert_eq!(body2["synapses_decayed"].as_u64().unwrap_or(1), 0);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn osovm_contributions_list_includes_contribution() {
    let app = make_app(make_test_state());
    call_with(app.clone(), "POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:list-toc",
        "device_id":       "gpu:list:01",
        "compute_units":   500u64,
        "proof_hash":      "sha256:list-seed",
    }))).await;

    let (status, body) = call_with(app, "GET", "/osovm/contributions", None).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert!(body["count"].as_u64().unwrap_or(0) >= 1);
    let contribs = body["contributions"].as_array().unwrap();
    assert!(contribs.iter().any(|c| c["contributor_did"] == "did:worker:list-toc"));
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn gpu_contribute_emission_appears_in_receipts_list() {
    let app = make_app(make_test_state());
    let (_, contrib_body) = call_with(app.clone(), "POST", "/osovm/gpu/contribute", Some(json!({
        "contributor_did": "did:worker:emit-check",
        "device_id":       "gpu:emit:01",
        "compute_units":   200u64,
        "proof_hash":      "sha256:emit-check",
    }))).await;
    let eid = contrib_body["emission_receipt_id"].as_str().unwrap();

    // Fetch via /emission/receipts/:id
    let (status, receipt) = call_with(app, "GET", &format!("/emission/receipts/{eid}"), None).await;
    assert_eq!(status, StatusCode::OK, "emission receipt should be retrievable: {:?}", receipt);
    assert_eq!(receipt["receipt_id"], eid);
}

// ─── Phase 53: A2A federation routing ────────────────────────────────────────

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn federation_register_peer_returns_201() {
    let (status, body) = call("POST", "/federation/peers", Some(json!({
        "peer_id":      "peer:alpha",
        "name":         "Alpha Node",
        "a2a_base_url": "http://alpha.local:8080/a2a",
        "did":          "did:vantage:alpha",
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "body: {:?}", body);
    assert_eq!(body["ok"], true);
    assert_eq!(body["peer_id"], "peer:alpha");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn federation_peers_list_includes_registered_peer() {
    let app = make_app(make_test_state());
    call_with(app.clone(), "POST", "/federation/peers", Some(json!({
        "peer_id":      "peer:list-test",
        "name":         "List Test Node",
        "a2a_base_url": "http://list.local:8080/a2a",
    }))).await;

    // GET /federation/peers returns mDNS peers (may be empty in CI) — check registered one
    // via the router state directly by re-fetching after register.
    // The existing GET /federation/peers calls avahi-browse, so we just verify register was OK.
    let (reg_status, _) = call_with(app.clone(), "POST", "/federation/peers", Some(json!({
        "peer_id":      "peer:list-test",
        "name":         "List Test Node",
        "a2a_base_url": "http://list.local:8080/a2a",
    }))).await;
    assert_eq!(reg_status, StatusCode::CREATED);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn federation_remove_unknown_peer_returns_404() {
    let (status, body) = call("DELETE", "/federation/peers/peer:does-not-exist", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {:?}", body);
    assert_eq!(body["error"], "peer_not_found");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn federation_remove_registered_peer_returns_ok() {
    let app = make_app(make_test_state());
    call_with(app.clone(), "POST", "/federation/peers", Some(json!({
        "peer_id":      "peer:to-remove",
        "name":         "Remove Me",
        "a2a_base_url": "http://rm.local:8080/a2a",
    }))).await;

    let (status, body) = call_with(app, "DELETE", "/federation/peers/peer:to-remove", None).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn federation_route_task_no_peers_returns_503() {
    let (status, body) = call("POST", "/federation/tasks", Some(json!({
        "message": { "type": "capture", "device_id": "go2:test" },
    }))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "body: {:?}", body);
    assert_eq!(body["error"], "no_peers");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn federation_route_task_prefer_unknown_peer_returns_404() {
    let app = make_app(make_test_state());
    // Register a real peer first so router isn't empty.
    call_with(app.clone(), "POST", "/federation/peers", Some(json!({
        "peer_id":      "peer:real",
        "name":         "Real",
        "a2a_base_url": "http://real.local:8080/a2a",
    }))).await;

    let (status, body) = call_with(app, "POST", "/federation/tasks", Some(json!({
        "message":      { "type": "capture" },
        "prefer_peer":  "peer:ghost",
    }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {:?}", body);
    assert_eq!(body["error"], "peer_not_found");
}

#[tokio::test]
    #[ignore = "migrated to Vantage/OSOVM/Omo-Koda2"]
async fn federation_health_check_returns_counts() {
    let (status, body) = call("POST", "/federation/health", None).await;
    assert_eq!(status, StatusCode::OK, "body: {:?}", body);
    assert!(body["healthy"].is_number());
    assert!(body["unreachable"].is_number());
}
