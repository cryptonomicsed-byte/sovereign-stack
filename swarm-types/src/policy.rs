use serde::{Deserialize, Serialize};

/// A policy is the winning trajectory turned into executable instructions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub policy_id:       String,
    pub traj_id:         String,
    pub twin_id:         String,
    pub kind:            PolicyKind,
    pub steps:           Vec<PolicyStep>,
    pub proof_of_sim:    String,
    pub vcp_session_id:  Option<String>,
    pub executed:        bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    Navigation,
    Manipulation,
    Inspection,
    Swarm,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStep {
    pub seq:      u32,
    pub command:  String,
    pub params:   serde_json::Value,
    pub expected_duration_ms: u32,
}

/// Result of selecting the best policy from N trajectory candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySelection {
    pub winner_traj_id: String,
    pub score:          f64,
    pub n_candidates:   u32,
    pub selection_hash: String,
}
