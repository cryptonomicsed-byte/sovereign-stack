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
        "vcp_nearby_devices"  => tool_nearby_devices(state).await,
        "vcp_connect"         => tool_connect(state, &args).await,
        "vcp_capture"         => tool_capture(state, &args).await,
        "sovereign_status"    => tool_status(state).await,
        "sovereign_job_status"=> tool_job_status(state, &args).await,
        "dip_send"            => tool_dip_send(state, &args),
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

    // Clone for the background task
    let identity   = state.identity.clone();
    let config     = state.config.clone();
    let job_store  = state.job_store.clone();
    let nostr      = state.nostr_relay.clone();
    let task_job   = job_id.clone();
    let dev_id     = device.device_id.clone();
    let model      = device.model.clone();

    let receipts  = state.receipt_store.clone();
    let witnesses = state.witnesses.clone();
    tokio::spawn(crate::node::run_capture_job(
        task_job, dev_id, model, identity, config, job_store, receipts, witnesses, nostr,
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
