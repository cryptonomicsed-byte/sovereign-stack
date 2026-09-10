//! A2A v1.0 wire types.
//!
//! Follows the Google A2A spec (https://google.github.io/A2A/specification/) with
//! Sovereign-specific extensions in `SovereignExtensions`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Task status lifecycle per A2A v1.0 §4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Submitted,
    Working,
    Completed,
    Failed,
    Canceled,
    InputRequired,
}

/// A2A Task — created by POST /a2a/tasks, polled via GET /a2a/tasks/:id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aTask {
    /// Unique task ID (UUID v4).
    pub id:          String,
    /// Session ID grouping related tasks (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id:  Option<String>,
    pub status:      TaskStatus,
    /// Input message(s) submitted with the task.
    pub history:     Vec<A2aMessage>,
    /// Artifacts produced by the task (outputs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts:   Vec<Artifact>,
    /// Agent-defined metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata:    HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub state:   TaskState,
    /// ISO-8601 timestamp of last state change.
    pub updated: String,
    /// Human-readable message (error detail on Failed state).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<A2aMessage>,
}

/// A2A Message — role + multipart content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aMessage {
    /// "user" | "agent"
    pub role:  String,
    pub parts: Vec<Part>,
}

/// A2A Part — text, data, or file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Part {
    Text { text: String },
    Data { data: serde_json::Value },
    File { file: FileContent },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub name:      String,
    pub mime_type: String,
    /// Base64-encoded bytes or URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes:     Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri:       Option<String>,
}

/// Artifact produced by a completed task (e.g. PLY bytes, receipt JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub name:  String,
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index: Vec<u32>,
}

/// POST /a2a/tasks request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSendParams {
    pub id:          Option<String>,
    pub session_id:  Option<String>,
    pub message:     A2aMessage,
    #[serde(default)]
    pub metadata:    HashMap<String, serde_json::Value>,
}

/// AgentCard — returned by GET /a2a/agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name:         String,
    pub description:  String,
    pub url:          String,
    pub version:      String,
    pub capabilities: AgentCapabilities,
    pub skills:       Vec<AgentSkill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider:     Option<AgentProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub streaming:          bool,
    pub push_notifications: bool,
    pub state_transition_history: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkill {
    pub id:          String,
    pub name:        String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples:    Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags:        Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProvider {
    pub organization: String,
    pub url:          String,
}

/// Configuration used to build the AgentCard.
#[derive(Debug, Clone)]
pub struct A2aConfig {
    pub name:        String,
    pub description: String,
    /// Public base URL where /a2a/* routes are mounted (e.g. "http://node.example.com").
    pub base_url:    String,
    pub version:     String,
    pub skills:      Vec<AgentSkill>,
    pub provider:    Option<AgentProvider>,
}

impl A2aConfig {
    pub fn to_agent_card(&self) -> AgentCard {
        AgentCard {
            name:        self.name.clone(),
            description: self.description.clone(),
            url:         format!("{}/a2a/agent", self.base_url),
            version:     self.version.clone(),
            capabilities: AgentCapabilities {
                streaming:                    false,
                push_notifications:           false,
                state_transition_history:     true,
            },
            skills:   self.skills.clone(),
            provider: self.provider.clone(),
        }
    }
}

impl Default for A2aConfig {
    fn default() -> Self {
        Self {
            name:        "sovereign-node".into(),
            description: "Sovereign Node — physical twin capture and provenance".into(),
            base_url:    "http://127.0.0.1:7779".into(),
            version:     "0.1.0".into(),
            provider:    None,
            skills: vec![
                AgentSkill {
                    id:          "twin_capture".into(),
                    name:        "Twin Capture".into(),
                    description: "Capture a physical environment as a Gaussian splat twin with provenance receipt".into(),
                    examples:    vec!["Capture the lab at unitree:go2:192.168.1.10".into()],
                    tags:        vec!["capture".into(), "twin".into(), "3dgs".into()],
                },
                AgentSkill {
                    id:          "receipt_query".into(),
                    name:        "Receipt Query".into(),
                    description: "Query capture or scene receipts anchored on Sui".into(),
                    examples:    vec!["Get receipt for twin:sha256:abc123".into()],
                    tags:        vec!["receipt".into(), "provenance".into()],
                },
            ],
        }
    }
}

/// A2A error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aError {
    pub code:    i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data:    Option<serde_json::Value>,
}

impl A2aError {
    pub fn task_not_found(id: &str) -> Self {
        Self { code: -32001, message: format!("task not found: {id}"), data: None }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self { code: -32603, message: msg.into(), data: None }
    }
}

