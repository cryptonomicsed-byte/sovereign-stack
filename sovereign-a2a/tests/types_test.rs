use sovereign_a2a::{
    A2aConfig, A2aError, A2aMessage, Artifact, Part,
    TaskSendParams, TaskState,
};
use serde_json::json;
use std::collections::HashMap;

fn text_part(s: &str) -> Part { Part::Text { text: s.into() } }

fn user_msg(s: &str) -> A2aMessage {
    A2aMessage { role: "user".into(), parts: vec![text_part(s)] }
}

#[test]
fn task_state_serde_roundtrip() {
    for state in &[TaskState::Submitted, TaskState::Working, TaskState::Completed,
                   TaskState::Failed, TaskState::Canceled, TaskState::InputRequired] {
        let s = serde_json::to_string(state).unwrap();
        let d: TaskState = serde_json::from_str(&s).unwrap();
        assert_eq!(*state, d);
    }
}

#[test]
fn task_state_lowercase_wire() {
    assert_eq!(serde_json::to_string(&TaskState::Submitted).unwrap(), "\"submitted\"");
    assert_eq!(serde_json::to_string(&TaskState::Completed).unwrap(), "\"completed\"");
}

#[test]
fn part_text_serde() {
    let p = text_part("hello");
    let s = serde_json::to_string(&p).unwrap();
    assert!(s.contains("\"type\":\"text\""));
    let d: Part = serde_json::from_str(&s).unwrap();
    assert!(matches!(d, Part::Text { text } if text == "hello"));
}

#[test]
fn part_data_serde() {
    let p = Part::Data { data: json!({"key": "value"}) };
    let s = serde_json::to_string(&p).unwrap();
    let d: Part = serde_json::from_str(&s).unwrap();
    assert!(matches!(d, Part::Data { .. }));
}

#[test]
fn message_role_and_parts() {
    let m = user_msg("do something");
    assert_eq!(m.role, "user");
    assert_eq!(m.parts.len(), 1);
}

#[test]
fn task_send_params_serde_roundtrip() {
    let p = TaskSendParams {
        id:         Some("task-001".into()),
        session_id: None,
        message:    user_msg("capture lab"),
        metadata:   HashMap::new(),
    };
    let s = serde_json::to_string(&p).unwrap();
    let d: TaskSendParams = serde_json::from_str(&s).unwrap();
    assert_eq!(d.id.as_deref(), Some("task-001"));
}

#[test]
fn default_config_to_agent_card() {
    let card = A2aConfig::default().to_agent_card();
    assert_eq!(card.name, "sovereign-node");
    assert!(card.url.ends_with("/a2a/agent"));
    assert!(!card.skills.is_empty());
    assert!(card.capabilities.state_transition_history);
}

#[test]
fn agent_card_url_uses_base_url() {
    let cfg = A2aConfig {
        name:        "test".into(),
        description: "d".into(),
        base_url:    "http://10.0.0.1:7779".into(),
        version:     "0.1".into(),
        skills:      vec![],
        provider:    None,
    };
    assert_eq!(cfg.to_agent_card().url, "http://10.0.0.1:7779/a2a/agent");
}

#[test]
fn agent_card_top_level_camel_case() {
    // AgentCard uses #[serde(rename_all = "camelCase")] — check top-level fields
    let card = A2aConfig::default().to_agent_card();
    let s = serde_json::to_string(&card).unwrap();
    // AgentCard's `capabilities` field serializes as-is (the struct name, not contents)
    assert!(s.contains("\"capabilities\""));
    // AgentCapabilities fields use default snake_case
    assert!(s.contains("\"state_transition_history\""));
    assert!(s.contains("\"push_notifications\""));
}

#[test]
fn error_task_not_found() {
    let e = A2aError::task_not_found("abc-123");
    assert_eq!(e.code, -32001);
    assert!(e.message.contains("abc-123"));
}

#[test]
fn error_internal() {
    let e = A2aError::internal("disk full");
    assert_eq!(e.code, -32603);
    assert!(e.message.contains("disk full"));
}

#[test]
fn artifact_serde_without_empty_index() {
    let a = Artifact { name: "splat.ply".into(), parts: vec![], index: vec![] };
    let s = serde_json::to_string(&a).unwrap();
    assert!(!s.contains("\"index\""), "empty index should be skipped");
}
