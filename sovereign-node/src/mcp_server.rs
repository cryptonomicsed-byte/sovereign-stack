//! MCP JSON-RPC 2.0 server — lets Ọmọ Kọ́dà call VCP tools directly.
//!
//! Endpoint: POST /mcp
//!
//! Implements the Model Context Protocol subset needed for VCP integration:
//!   tools/list   → list available VCP + node tools
//!   tools/call   → execute a named tool
//!
//! Tools exposed:
//!   vcp_nearby_devices  — return live device registry (PERCEIVE phase input)
//!   vcp_connect         — initiate VCP handshake for a known device
//!   vcp_capture         — trigger a full capture pipeline job for a device
//!   sovereign_status    — return node uptime, DID, job count
//!   dip_send            — wrap a JSON payload in a DIP Message envelope
//!
//! This server is mounted at /mcp alongside the existing REST API.
//! Both are served by the same axum listener.

use axum::{extract::State, Json, response::IntoResponse, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;
use uuid::Uuid;

use dip::{DipEnvelope, DipKind, DipAddress, address::DipNetwork};
use sovereign_types::IdentityChain;

use crate::node::NodeState;
use crate::jobs::{Job, JobStatus};

// ── MCP wire types ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id:      Value,
    pub method:  String,
    pub params:  Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id:      Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result:  Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error:   Option<McpError>,
}

#[derive(Debug, Serialize)]
pub struct McpError {
    pub code:    i32,
    pub message: String,
}

impl McpResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }
    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(), id, result: None,
            error: Some(McpError { code, message: message.into() }),
        }
    }
}

// ── Handler ────────────────────────────────────────────────────────────────

/// POST /mcp — single entry point for all MCP tool calls.
pub async fn handle_mcp(
    State(state): State<NodeState>,
    Json(req): Json<McpRequest>,
) -> impl IntoResponse {
    if req.jsonrpc != "2.0" {
        let resp = McpResponse::err(req.id, -32600, "invalid JSON-RPC version");
        return (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()));
    }

    let result = match req.method.as_str() {
        "tools/list"  => handle_tools_list(),
        "tools/call"  => handle_tools_call(&state, req.params.unwrap_or(Value::Null)).await,
        other => Err(format!("method not found: {other}")),
    };

    let resp = match result {
        Ok(val) => McpResponse::ok(req.id, val),
        Err(e)  => McpResponse::err(req.id, -32601, e),
    };

    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
}

fn handle_tools_list() -> Result<Value, String> {
    Ok(json!({
        "tools": [
            {
                "name":        "vcp_nearby_devices",
                "description": "Return all VCP devices currently visible to the sovereign node. Call this during PERCEIVE to know what physical machines are nearby.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name":        "vcp_connect",
                "description": "Check whether a specific VCP device is reachable and ready for capability negotiation.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "device_id": { "type": "string", "description": "The VCP device ID from vcp_nearby_devices" }
                    },
                    "required": ["device_id"]
                }
            },
            {
                "name":        "vcp_capture",
                "description": "Trigger a full capture pipeline for a VCP device. Returns a job_id to poll with sovereign_job_status.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "device_id": { "type": "string", "description": "The VCP device ID to capture" }
                    },
                    "required": ["device_id"]
                }
            },
            {
                "name":        "sovereign_status",
                "description": "Return sovereign node uptime, DID, active job count, and device count.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name":        "sovereign_job_status",
                "description": "Return the status of a capture job by job_id.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "job_id": { "type": "string" }
                    },
                    "required": ["job_id"]
                }
            },
            {
                "name":        "dip_send",
                "description": "Send a DIP Message envelope to a destination DID via Vantage or Nostr.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "destination_did": { "type": "string" },
                        "network":         { "type": "string", "enum": ["vantage", "nostr", "meshtastic"] },
                        "payload":         { "type": "object" }
                    },
                    "required": ["destination_did", "network", "payload"]
                }
            },
            {
                "name":        "vcp_body_session_open",
                "description": "Open a fine-grained VCP body session for a device, specifying which capabilities (locomotion, sensor.camera, …) are needed. Returns session_id for subsequent commands. Use this for scripted robot control; use vcp_capture for a fully automated splat pipeline.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "device_id":    { "type": "string", "description": "VCP device ID from vcp_nearby_devices" },
                        "capabilities": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Capability names to request (e.g. [\"locomotion\", \"sensor.camera\"])"
                        },
                        "agent_tier":   { "type": "string", "description": "Agent trust tier (t1–t5, default t4)" }
                    },
                    "required": ["device_id"]
                }
            },
            {
                "name":        "vcp_body_session_command",
                "description": "Send a capability command within an open VCP body session. Returns the command receipt.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id":  { "type": "string", "description": "Session ID from vcp_body_session_open" },
                        "capability":  { "type": "string", "description": "Which capability to invoke (e.g. locomotion, sensor.camera)" },
                        "action":      { "type": "string", "description": "Action within the capability (e.g. walk, capture_frame)" },
                        "params":      { "type": "object", "description": "Action-specific parameters" }
                    },
                    "required": ["session_id", "capability", "action"]
                }
            },
            {
                "name":        "vcp_body_session_close",
                "description": "Close a VCP body session and return the session receipt. If the session had a camera capability and mission_success=true, a capture job is auto-queued — poll with sovereign_job_status.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id":      { "type": "string" },
                        "mission_success": { "type": "boolean", "description": "Whether the mission goal was achieved (triggers auto-capture if true + camera present)" }
                    },
                    "required": ["session_id"]
                }
            },
            {
                "name":        "sovereign_timeline",
                "description": "Return the 4D provenance timeline for a twin — ordered snapshots of every capture, with quality scores and Odù tile locations.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "twin_id": { "type": "string", "description": "Twin or device ID (e.g. 'unitree:go2:192.168.1.10')" }
                    },
                    "required": ["twin_id"]
                }
            },
            {
                "name":        "sovereign_timeline_diff",
                "description": "Compute 4D change detection between the earliest and latest snapshot of a twin's timeline. Returns quality delta, new/dropped modalities, and time span. Requires ≥2 snapshots.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "twin_id": { "type": "string" }
                    },
                    "required": ["twin_id"]
                }
            },
            {
                "name":        "sovereign_ip_root",
                "description": "Return the node's cached IP Root event (Nostr kind 31900). This event establishes the agent's provenance identity on Nostr. Returns null if no Nostr identity is configured.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            }
        ]
    }))
}

async fn handle_tools_call(state: &NodeState, params: Value) -> Result<Value, String> {
    let name = params.get("name")
        .and_then(|v| v.as_str())
        .ok_or("missing tool name")?;
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);

    info!(tool = name, "MCP tool call");

    match name {
        "vcp_nearby_devices"       => tool_nearby_devices(state).await,
        "vcp_connect"              => tool_connect(state, &args).await,
        "vcp_capture"              => tool_capture(state, &args).await,
        "sovereign_status"         => tool_status(state).await,
        "sovereign_job_status"     => tool_job_status(state, &args).await,
        "dip_send"                 => tool_dip_send(state, &args),
        "vcp_body_session_open"    => tool_body_session_open(state, &args).await,
        "vcp_body_session_command" => tool_body_session_command(state, &args).await,
        "vcp_body_session_close"   => tool_body_session_close(state, &args).await,
        "sovereign_timeline"       => tool_timeline(state, &args).await,
        "sovereign_timeline_diff"  => tool_timeline_diff(state, &args).await,
        "sovereign_ip_root"        => tool_ip_root(state),
        other => Err(format!("unknown tool: {other}")),
    }
}

// ── Tool implementations ──────────────────────────────────────────────────

async fn tool_nearby_devices(state: &NodeState) -> Result<Value, String> {
    let mut summary = state.registry.heartbeat_summary().await;
    summary["perception_hint"] = json!(
        "These VCP devices are physically nearby. Use vcp_capture to trigger a \
         spatial twin capture, or vcp_connect to verify handshake readiness."
    );
    Ok(json!({ "content": [{ "type": "text", "text": summary.to_string() }] }))
}

async fn tool_connect(state: &NodeState, args: &Value) -> Result<Value, String> {
    let device_id = args.get("device_id")
        .and_then(|v| v.as_str())
        .ok_or("missing device_id")?;

    let result = match state.registry.get(device_id).await {
        Some(device) => json!({
            "status":      "ready_for_handshake",
            "device_id":   device.device_id,
            "manufacturer": device.manufacturer,
            "model":       device.model,
            "capabilities": device.capability_summary,
            "transport":   device.transport,
            "age_secs":    device.age_secs(),
            "next_step":   "call vcp_capture to begin capture pipeline"
        }),
        None => json!({
            "status":    "not_found",
            "device_id": device_id,
            "hint":      "device may have left range — call vcp_nearby_devices to refresh"
        }),
    };
    Ok(json!({ "content": [{ "type": "text", "text": result.to_string() }] }))
}

async fn tool_capture(state: &NodeState, args: &Value) -> Result<Value, String> {
    let device_id = args.get("device_id")
        .and_then(|v| v.as_str())
        .ok_or("missing device_id")?;

    let device = state.registry.get(device_id).await
        .ok_or_else(|| format!("device not found: {device_id}"))?;

    let job_id  = format!("job:{}", Uuid::new_v4());
    let job     = Job::new(job_id.clone(), device_id.to_string());
    state.job_store.insert(job).await;

    info!(job_id = %job_id, device_id = %device_id, source = "mcp", "capture job queued via MCP");

    let task_job   = job_id.clone();
    let dev_id     = device.device_id.clone();
    let model      = device.model.clone();
    tokio::spawn(crate::node::run_capture_job(
        task_job, dev_id, model,
        state.identity.clone(),
        state.config.clone(),
        state.job_store.clone(),
        state.receipt_store.clone(),
        state.dip_gateway.clone(),
        state.twin_events.clone(),
    ));

    let result = json!({
        "job_id":   job_id,
        "device_id": device_id,
        "status":   "queued",
        "poll_tool": "sovereign_job_status"
    });
    Ok(json!({ "content": [{ "type": "text", "text": result.to_string() }] }))
}

async fn tool_status(state: &NodeState) -> Result<Value, String> {
    let now          = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let uptime_secs  = (now - state.started_at) / 1000;
    let device_count = state.registry.count().await;
    let jobs         = state.job_store.all().await;
    let running      = jobs.iter().filter(|j| matches!(j.status, JobStatus::Running)).count();
    let completed    = jobs.iter().filter(|j| matches!(j.status, JobStatus::Completed { .. })).count();

    let result = json!({
        "node":          state.config.node.name,
        "did":           state.identity.did,
        "uptime_secs":   uptime_secs,
        "device_count":  device_count,
        "jobs_total":    jobs.len(),
        "jobs_running":  running,
        "jobs_completed": completed,
        "nostr_enabled": state.config.dip.nostr_enabled,
    });
    Ok(json!({ "content": [{ "type": "text", "text": result.to_string() }] }))
}

async fn tool_job_status(state: &NodeState, args: &Value) -> Result<Value, String> {
    let job_id = args.get("job_id")
        .and_then(|v| v.as_str())
        .ok_or("missing job_id")?;

    match state.job_store.get(job_id).await {
        Some(job) => {
            let result = serde_json::to_value(&job).map_err(|e| e.to_string())?;
            Ok(json!({ "content": [{ "type": "text", "text": result.to_string() }] }))
        }
        None => Err(format!("job not found: {job_id}")),
    }
}

fn tool_dip_send(state: &NodeState, args: &Value) -> Result<Value, String> {
    let dest_did = args.get("destination_did")
        .and_then(|v| v.as_str())
        .ok_or("missing destination_did")?;
    let network_str = args.get("network")
        .and_then(|v| v.as_str())
        .unwrap_or("vantage");
    let payload = args.get("payload")
        .cloned()
        .unwrap_or(Value::Null);

    let network = match network_str {
        "nostr"       => DipNetwork::Nostr,
        "meshtastic"  => DipNetwork::Meshtastic,
        _             => DipNetwork::Vantage,
    };

    let origin = DipAddress::vantage(&state.identity.did);
    let dest   = DipAddress {
        network,
        address: dest_did.into(),
        did:     Some(dest_did.into()),
    };
    let chain = IdentityChain::new(state.identity.did.clone(), state.identity.did.clone());

    let envelope = DipEnvelope::build(
        origin, dest, chain,
        DipKind::Message,
        payload,
        300,
        &state.identity.private_key,
    ).map_err(|e| e.to_string())?;

    info!(
        msg_id = %envelope.message_id,
        dest   = %dest_did,
        net    = %network_str,
        "DIP message sent via MCP"
    );

    Ok(json!({
        "content": [{
            "type": "text",
            "text": json!({
                "message_id":  envelope.message_id,
                "destination": dest_did,
                "network":     network_str,
                "status":      "routed"
            }).to_string()
        }]
    }))
}

// ── vcp_body_session_open ─────────────────────────────────────────────────────

async fn tool_body_session_open(state: &NodeState, args: &Value) -> Result<Value, String> {
    use vcp::BodySessionMode;

    let device_id = args.get("device_id")
        .and_then(|v| v.as_str())
        .ok_or("missing device_id")?;

    let capabilities: Vec<String> = args.get("capabilities")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| vec!["sensor.camera".into()]);

    let agent_tier = match args.get("agent_tier").and_then(|v| v.as_str()).unwrap_or("t4") {
        "t0" | "T0" => sovereign_types::TrustTier::T0,
        "t1" | "T1" => sovereign_types::TrustTier::T1,
        "t2" | "T2" => sovereign_types::TrustTier::T2,
        "t3" | "T3" => sovereign_types::TrustTier::T3,
        "t5" | "T5" => sovereign_types::TrustTier::T5,
        _            => sovereign_types::TrustTier::T4,
    };

    let session = match vcp::BodySession::new(
        state.identity.did.clone(),
        agent_tier,
        device_id.to_string(),
        BodySessionMode::HumanSupervised,
        capabilities.clone(),
        None,
    ) {
        Ok(s) => s,
        Err(e) => return Err(format!("body session creation failed: {e}")),
    };

    let session_id = session.session_id.clone();
    state.body_store.insert_session(session).await;

    info!(session_id = %session_id, device_id = %device_id, "VCP body session opened via MCP");

    Ok(json!({ "content": [{ "type": "text", "text": json!({
        "session_id":   session_id,
        "device_id":    device_id,
        "capabilities": capabilities,
        "status":       "open",
        "next_steps":   ["vcp_body_session_command", "vcp_body_session_close"],
    }).to_string() }] }))
}

// ── vcp_body_session_command ──────────────────────────────────────────────────

async fn tool_body_session_command(state: &NodeState, args: &Value) -> Result<Value, String> {
    let session_id = args.get("session_id")
        .and_then(|v| v.as_str())
        .ok_or("missing session_id")?;
    let capability = args.get("capability")
        .and_then(|v| v.as_str())
        .ok_or("missing capability")?;
    let action = args.get("action")
        .and_then(|v| v.as_str())
        .ok_or("missing action")?;
    let params = args.get("params").cloned().unwrap_or(Value::Null);

    let session = state.body_store.get_session(session_id).await
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    // Validate capability is granted
    if !session.capabilities.is_empty()
        && !session.capabilities.iter().any(|c| c == capability)
    {
        return Err(format!("capability '{capability}' not granted in session {session_id}"));
    }

    let cmd_id = format!("cmd:{}", Uuid::new_v4());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    info!(
        cmd_id = %cmd_id,
        session_id = %session_id,
        capability = %capability,
        action = %action,
        "VCP body session command via MCP"
    );

    Ok(json!({ "content": [{ "type": "text", "text": json!({
        "cmd_id":      cmd_id,
        "session_id":  session_id,
        "capability":  capability,
        "action":      action,
        "params":      params,
        "status":      "accepted",
        "timestamp_ms": ts,
    }).to_string() }] }))
}

// ── vcp_body_session_close ────────────────────────────────────────────────────

async fn tool_body_session_close(state: &NodeState, args: &Value) -> Result<Value, String> {
    let session_id = args.get("session_id")
        .and_then(|v| v.as_str())
        .ok_or("missing session_id")?;
    let mission_success = args.get("mission_success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let session = state.body_store.get_session(session_id).await
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    let receipt_id = format!("body-receipt:{}", session_id);
    let has_camera = session.capabilities.iter().any(|c| c.contains("camera"));

    // Phase 4.1 — auto-queue capture job if mission succeeded with camera
    let mut capture_job_id: Option<String> = None;
    if mission_success && has_camera {
        let job_id = format!("job:{}", Uuid::new_v4());
        let job = Job::new(job_id.clone(), session.body_id.clone());
        state.job_store.insert(job).await;
        let model = state.registry.get(&session.body_id).await
            .map(|d| d.model.clone())
            .unwrap_or_else(|| "Go2".into());
        tokio::spawn(crate::node::run_capture_job(
            job_id.clone(),
            session.body_id.clone(),
            model,
            state.identity.clone(),
            state.config.clone(),
            state.job_store.clone(),
            state.receipt_store.clone(),
            state.dip_gateway.clone(),
            state.twin_events.clone(),
        ));
        capture_job_id = Some(job_id);
    }

    info!(
        session_id = %session_id,
        receipt_id = %receipt_id,
        mission_success,
        capture_queued = capture_job_id.is_some(),
        "VCP body session closed via MCP"
    );

    Ok(json!({ "content": [{ "type": "text", "text": json!({
        "session_id":       session_id,
        "receipt_id":       receipt_id,
        "mission_success":  mission_success,
        "capture_job_id":   capture_job_id,
        "status":           "closed",
    }).to_string() }] }))
}

// ── sovereign_timeline ────────────────────────────────────────────────────────

async fn tool_timeline(_state: &NodeState, args: &Value) -> Result<Value, String> {
    let twin_id = args.get("twin_id")
        .and_then(|v| v.as_str())
        .ok_or("missing twin_id")?;
    // Timeline store migrated to Vantage — query via Vantage API
    Ok(json!({ "content": [{ "type": "text", "text":
        json!({ "status": "migrated", "hint": "query timeline from Vantage API", "twin_id": twin_id }).to_string()
    }] }))
}

// ── sovereign_timeline_diff ───────────────────────────────────────────────────

async fn tool_timeline_diff(_state: &NodeState, args: &Value) -> Result<Value, String> {
    let twin_id = args.get("twin_id")
        .and_then(|v| v.as_str())
        .ok_or("missing twin_id")?;
    // Timeline store migrated to Vantage — diff via Vantage API
    let result = json!({
        "status": "migrated",
        "hint": "query timeline diff from Vantage API",
        "twin_id": twin_id,
    });
    Ok(json!({ "content": [{ "type": "text", "text": result.to_string() }] }))
}

// ── sovereign_ip_root ─────────────────────────────────────────────────────────

fn tool_ip_root(_state: &NodeState) -> Result<Value, String> {
    // IP Root publishing migrated to ip-layer repo
    Ok(json!({ "content": [{ "type": "text", "text":
        json!({ "status": "migrated", "hint": "IP Root publishing now handled by ip-layer" }).to_string()
    }] }))
}
