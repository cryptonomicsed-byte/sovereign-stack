//! DIP ↔ MCP adapter.
//!
//! MCP (Model Context Protocol) tool calls are wrapped in DIP Capability envelopes
//! so they can be routed across network boundaries (Vantage, Nostr, Meshtastic).
//!
//! Wire mapping:
//!   MCP tool call request  → DipKind::Capability, direction=Request
//!   MCP tool call response → DipKind::Capability, direction=Grant
//!   tool name stored in DipCapability.id, arguments in DipCapability.params

use serde::{Deserialize, Serialize};
use serde_json::Value;
use async_trait::async_trait;

use sovereign_types::{IdentityChain, crypto::{generate_keypair, did_from_pubkey}};
use crate::envelope::{DipEnvelope, DipKind, CapabilityPayload, CapabilityDirection, DipCapability};
use crate::address::{DipAddress, DipNetwork};
use crate::adapter::DipAdapter;
use crate::error::{DipError, DipResult};

/// A minimal MCP JSON-RPC tool call (request side).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,   // "tools/call"
    pub params: McpToolCallParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallParams {
    pub name: String,
    pub arguments: Value,
}

/// A minimal MCP JSON-RPC tool result (response side).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub jsonrpc: String,
    pub id: String,
    pub result: Value,
}

/// Wraps MCP messages in DIP envelopes and vice-versa.
pub struct McpAdapter {
    local_address: DipAddress,
    signing_key: String,
}

impl McpAdapter {
    pub fn new(local_did: impl Into<String>, signing_key: impl Into<String>) -> Self {
        let did = local_did.into();
        Self {
            local_address: DipAddress::vantage(did),
            signing_key: signing_key.into(),
        }
    }

    /// Wrap an MCP tool call into a DIP Capability(Request) envelope.
    pub fn wrap_tool_call(
        &self,
        call: &McpToolCall,
        destination_did: impl Into<String>,
        chain: IdentityChain,
    ) -> DipResult<DipEnvelope> {
        let dest = DipAddress::vantage(destination_did.into());
        let payload = serde_json::to_value(CapabilityPayload {
            direction: CapabilityDirection::Request,
            capabilities: vec![DipCapability {
                id: call.params.name.clone(),
                description: Some(format!("mcp:{}", call.method)),
                params: Some(call.params.arguments.clone()),
                constraints: None,
                expires_at: None,
            }],
            context: Some(call.id.clone()),
        })?;
        DipEnvelope::build(
            self.local_address.clone(),
            dest,
            chain,
            DipKind::Capability,
            payload,
            300,
            &self.signing_key,
        )
    }

    /// Wrap an MCP tool result into a DIP Capability(Grant) envelope.
    pub fn wrap_tool_result(
        &self,
        result: &McpToolResult,
        tool_name: impl Into<String>,
        destination_did: impl Into<String>,
        chain: IdentityChain,
    ) -> DipResult<DipEnvelope> {
        let dest = DipAddress::vantage(destination_did.into());
        let payload = serde_json::to_value(CapabilityPayload {
            direction: CapabilityDirection::Grant,
            capabilities: vec![DipCapability {
                id: tool_name.into(),
                description: Some("mcp:tools/call:result".into()),
                params: Some(result.result.clone()),
                constraints: None,
                expires_at: None,
            }],
            context: Some(result.id.clone()),
        })?;
        DipEnvelope::build(
            self.local_address.clone(),
            dest,
            chain,
            DipKind::Capability,
            payload,
            300,
            &self.signing_key,
        )
    }

    /// Extract an MCP tool call from a DIP Capability(Request) envelope.
    pub fn unwrap_tool_call(envelope: &DipEnvelope) -> DipResult<McpToolCall> {
        let cap: CapabilityPayload = serde_json::from_value(envelope.payload.clone())
            .map_err(|e| DipError::RoutingFailed(e.to_string()))?;
        if !matches!(cap.direction, CapabilityDirection::Request) {
            return Err(DipError::RoutingFailed("expected Request direction".into()));
        }
        let first = cap.capabilities.into_iter().next()
            .ok_or_else(|| DipError::RoutingFailed("no capabilities in payload".into()))?;
        Ok(McpToolCall {
            jsonrpc: "2.0".into(),
            id: cap.context.unwrap_or_else(|| envelope.message_id.clone()),
            method: first.description
                .and_then(|d| d.strip_prefix("mcp:").map(String::from))
                .unwrap_or_else(|| "tools/call".into()),
            params: McpToolCallParams {
                name: first.id,
                arguments: first.params.unwrap_or(Value::Null),
            },
        })
    }

    /// Extract an MCP tool result from a DIP Capability(Grant) envelope.
    pub fn unwrap_tool_result(envelope: &DipEnvelope) -> DipResult<McpToolResult> {
        let cap: CapabilityPayload = serde_json::from_value(envelope.payload.clone())
            .map_err(|e| DipError::RoutingFailed(e.to_string()))?;
        if !matches!(cap.direction, CapabilityDirection::Grant) {
            return Err(DipError::RoutingFailed("expected Grant direction".into()));
        }
        let first = cap.capabilities.into_iter().next()
            .ok_or_else(|| DipError::RoutingFailed("no capabilities in payload".into()))?;
        Ok(McpToolResult {
            jsonrpc: "2.0".into(),
            id: cap.context.unwrap_or_else(|| envelope.message_id.clone()),
            result: first.params.unwrap_or(Value::Null),
        })
    }
}

#[async_trait]
impl DipAdapter for McpAdapter {
    fn name(&self) -> &'static str { "mcp" }

    async fn to_dip(&self, native: &[u8]) -> DipResult<DipEnvelope> {
        let call: McpToolCall = serde_json::from_slice(native)
            .map_err(|e| DipError::Serialization(e))?;
        let chain = IdentityChain::new(
            self.local_address.address.clone(),
            "anon-agent".into(),
        );
        self.wrap_tool_call(&call, self.local_address.address.clone(), chain)
    }

    async fn from_dip(&self, envelope: &DipEnvelope) -> DipResult<Vec<u8>> {
        let call = Self::unwrap_tool_call(envelope)?;
        serde_json::to_vec(&call).map_err(DipError::Serialization)
    }

    async fn send(&self, _envelope: &DipEnvelope) -> DipResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::crypto::{generate_keypair, did_from_pubkey};

    fn make_adapter() -> (McpAdapter, String) {
        let (priv_key, pub_key) = generate_keypair();
        let did = did_from_pubkey(&pub_key, "agent");
        (McpAdapter::new(did.clone(), priv_key), did)
    }

    fn make_chain(did: &str) -> IdentityChain {
        IdentityChain::new(did.into(), "agent-1".into())
    }

    #[test]
    fn wrap_unwrap_tool_call_roundtrip() {
        let (adapter, did) = make_adapter();
        let call = McpToolCall {
            jsonrpc: "2.0".into(),
            id: "req-1".into(),
            method: "tools/call".into(),
            params: McpToolCallParams {
                name: "vcp_nearby_devices".into(),
                arguments: serde_json::json!({"radius_m": 50}),
            },
        };
        let envelope = adapter.wrap_tool_call(&call, did.clone(), make_chain(&did)).unwrap();
        assert_eq!(envelope.kind, DipKind::Capability);

        let recovered = McpAdapter::unwrap_tool_call(&envelope).unwrap();
        assert_eq!(recovered.params.name, "vcp_nearby_devices");
        assert_eq!(recovered.params.arguments["radius_m"], 50);
        assert_eq!(recovered.id, "req-1");
    }

    #[test]
    fn wrap_unwrap_tool_result_roundtrip() {
        let (adapter, did) = make_adapter();
        let result = McpToolResult {
            jsonrpc: "2.0".into(),
            id: "req-1".into(),
            result: serde_json::json!([{"id": "device-abc", "capabilities": ["camera"]}]),
        };
        let envelope = adapter.wrap_tool_result(&result, "vcp_nearby_devices", did.clone(), make_chain(&did)).unwrap();
        let recovered = McpAdapter::unwrap_tool_result(&envelope).unwrap();
        assert_eq!(recovered.result[0]["id"], "device-abc");
        assert_eq!(recovered.id, "req-1");
    }

    #[test]
    fn wrong_direction_rejected() {
        let (adapter, did) = make_adapter();
        let result = McpToolResult {
            jsonrpc: "2.0".into(),
            id: "req-1".into(),
            result: serde_json::json!({}),
        };
        let envelope = adapter.wrap_tool_result(&result, "some_tool", did.clone(), make_chain(&did)).unwrap();
        // Grant direction should be rejected when unwrapping as a call
        let err = McpAdapter::unwrap_tool_call(&envelope);
        assert!(err.is_err());
    }
}
