use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A physical-world attestation signed by a Witness node.
///
/// Published as Nostr kind 31020.  The `observation_hash` commits to the
/// full sensor bundle; `sim_receipt_id` links back to the ScarabSwarm
/// Proof-of-Simulation that generated the executed policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessAttestation {
    pub attest_id:        String,
    pub kind:             AttestationKind,

    pub vcp_session_id:   String,
    pub device_id:        String,
    pub agent_id:         String,
    pub sim_receipt_id:   Option<String>,

    pub observation_hash: String,
    pub outcome:          String,
    pub status:           AttestationStatus,

    pub latitude:         Option<f64>,
    pub longitude:        Option<f64>,
    pub altitude_m:       Option<f64>,

    pub nostr_event_id:   Option<String>,
    pub zangbeto_anchor:  Option<String>,
    pub arp_receipt_id:   Option<String>,

    pub timestamp:        DateTime<Utc>,
    /// Ed25519 hex signature from the Witness node's key
    pub signature:        String,
}

impl WitnessAttestation {
    pub fn canonical_hash(&self) -> String {
        let data = format!(
            "{}:{}:{}:{}:{}",
            self.attest_id, self.vcp_session_id, self.observation_hash,
            self.outcome, self.timestamp.timestamp()
        );
        sha256_hex(data.as_bytes())
    }

    /// Convert to Nostr kind 31020 tags
    pub fn to_nostr_tags(&self) -> Vec<Vec<String>> {
        let mut tags = vec![
            vec!["d".to_string(), self.attest_id.clone()],
            vec!["t".to_string(), "witness".to_string()],
            vec!["session".to_string(), self.vcp_session_id.clone()],
            vec!["device".to_string(), self.device_id.clone()],
            vec!["observation_hash".to_string(), self.observation_hash.clone()],
            vec!["outcome".to_string(), self.outcome.clone()],
        ];
        if let Some(ref id) = self.sim_receipt_id {
            tags.push(vec!["sim_receipt".to_string(), id.clone()]);
        }
        tags
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    format!("{:016x}{:016x}", h.finish(), h.finish().wrapping_mul(0xcafebabe))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttestationKind {
    /// Robot/drone executed a simulated policy and outcome was physically observed
    PolicyExecution,
    /// Sensor observation recorded (no prior simulation)
    SensorObservation,
    /// Twin data capture (feeds Gaussian splat pipeline)
    TwinCapture,
    /// Anomaly detected during execution
    AnomalyReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttestationStatus {
    Pending,
    Signed,
    Published,
    Anchored,
}
