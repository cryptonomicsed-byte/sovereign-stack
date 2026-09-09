use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::{IdentityChain, Hash, Signature, Timestamp};
use crate::grant::VcpCapabilityGrant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VcpSessionOutcome {
    Completed,
    Revoked,
    Expired,
    DeviceFault,
    NetworkLoss,
}

/// A VCP session receipt — issued on session close.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcpSessionReceipt {
    pub receipt_id:        String,
    pub session_id:        String,
    pub grant_id:          String,
    pub identity:          IdentityChain,
    pub device_id:         String,
    pub capabilities_used: Vec<String>,
    pub commands_issued:   u32,
    pub commands_success:  u32,
    pub commands_failed:   u32,
    pub telemetry_summary: Option<Value>,
    pub started_at:        Timestamp,
    pub ended_at:          Timestamp,
    pub duration_ms:       u64,
    pub outcome:           VcpSessionOutcome,
    pub evidence_ids:      Vec<String>,  // links to TSP 31020 receipts if scanning
    pub merkle_root:       Hash,
    pub signature:         Signature,
}

/// In-memory session tracker.
pub struct VcpSession {
    pub grant:            VcpCapabilityGrant,
    pub started_at:       Timestamp,
    pub commands_issued:  u32,
    pub commands_success: u32,
    pub commands_failed:  u32,
    pub capabilities_used: std::collections::HashSet<String>,
    pub evidence_ids:     Vec<String>,
}

impl VcpSession {
    pub fn new(grant: VcpCapabilityGrant) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            grant,
            started_at:       now,
            commands_issued:  0,
            commands_success: 0,
            commands_failed:  0,
            capabilities_used: Default::default(),
            evidence_ids:     vec![],
        }
    }

    pub fn record_command(&mut self, capability: &str, success: bool) {
        self.commands_issued += 1;
        self.capabilities_used.insert(capability.to_string());
        if success { self.commands_success += 1; } else { self.commands_failed += 1; }
    }

    pub fn add_evidence(&mut self, receipt_id: impl Into<String>) {
        self.evidence_ids.push(receipt_id.into());
    }

    pub fn close(self, outcome: VcpSessionOutcome, device_key: &str) -> VcpSessionReceipt {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let receipt_id = format!("rcpt:vcp:{}", uuid::Uuid::new_v4());
        VcpSessionReceipt {
            receipt_id,
            session_id:        self.grant.session_id.clone(),
            grant_id:          self.grant.grant_id.clone(),
            identity:          self.grant.identity.clone(),
            device_id:         self.grant.device_id.clone(),
            capabilities_used: self.capabilities_used.into_iter().collect(),
            commands_issued:   self.commands_issued,
            commands_success:  self.commands_success,
            commands_failed:   self.commands_failed,
            telemetry_summary: None,
            started_at:        self.started_at,
            ended_at:          now,
            duration_ms:       now - self.started_at,
            outcome,
            evidence_ids:      self.evidence_ids,
            merkle_root:       String::new(),
            signature:         String::new(),
        }
    }
}
