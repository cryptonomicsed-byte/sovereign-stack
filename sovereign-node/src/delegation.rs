//! Node-to-node capture delegation via A2A v1.0.
//!
//! When a capture request names a device_id that this node cannot reach
//! directly, it can delegate the task to a peer node configured under
//! `[peers]` in config.toml.
//!
//! Flow:
//!   1. POST /capture/delegate  { device_id, hint?, peer? }
//!   2. Pick a peer (by name if specified, else first available)
//!   3. Submit A2A task to peer's /a2a/tasks endpoint
//!   4. Return task_id + poll URL so caller can track progress

use serde::{Deserialize, Serialize};

use sovereign_a2a::{A2aClient, A2aTask};

use crate::config::PeerNodeConfig;

/// POST /capture/delegate request body.
#[derive(Debug, Deserialize)]
pub struct DelegateRequest {
    pub device_id: String,
    #[serde(default)]
    pub hint: String,
    /// Prefer a specific peer by name; if omitted, first configured peer is used.
    pub peer: Option<String>,
}

/// Result of a successful delegation.
#[derive(Debug, Serialize)]
pub struct DelegateResult {
    pub peer_name: String,
    pub peer_url:  String,
    pub task_id:   String,
    pub poll_url:  String,
}

/// Delegate a capture task to the best available peer node.
pub async fn delegate_capture(
    peers:     &[PeerNodeConfig],
    device_id: &str,
    hint:      &str,
    prefer:    Option<&str>,
) -> Result<DelegateResult, String> {
    let peer = match prefer {
        Some(name) => peers.iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("peer '{name}' not found in config"))?,
        None => peers.first()
            .ok_or_else(|| "no peers configured — add [[peers.nodes]] to config.toml".to_string())?,
    };

    let client = A2aClient::new(&peer.a2a_base_url);

    let text = if hint.is_empty() {
        format!("capture {device_id}")
    } else {
        format!("capture {device_id} {hint}")
    };

    let task: A2aTask = client.submit_task(text)
        .await.map_err(|e| format!("A2A submit failed: {e}"))?;

    Ok(DelegateResult {
        poll_url:  format!("{}/a2a/tasks/{}", peer.a2a_base_url, task.id),
        peer_name: peer.name.clone(),
        peer_url:  peer.a2a_base_url.clone(),
        task_id:   task.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PeerNodeConfig;

    #[test]
    fn delegate_request_deserialises() {
        let json = r#"{"device_id":"unitree:go2:1","hint":"","peer":null}"#;
        let req: DelegateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.device_id, "unitree:go2:1");
        assert!(req.peer.is_none());
    }

    #[test]
    fn pick_peer_by_name() {
        let peers = vec![
            PeerNodeConfig { name: "alpha".into(), a2a_base_url: "http://alpha:7779".into() },
            PeerNodeConfig { name: "beta".into(),  a2a_base_url: "http://beta:7779".into() },
        ];
        let found = peers.iter().find(|p| p.name == "beta").unwrap();
        assert_eq!(found.a2a_base_url, "http://beta:7779");
    }

    #[test]
    fn delegate_text_combines_device_hint() {
        let hint = "panoramic";
        let device_id = "unitree:go2:192.168.1.10";
        let text = format!("capture {device_id} {hint}");
        assert!(text.starts_with("capture unitree:go2:"));
        assert!(text.contains("panoramic"));
    }

    #[tokio::test]
    async fn delegate_fails_gracefully_with_no_peers() {
        let result = delegate_capture(&[], "go2:1", "", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no peers configured"));
    }

    #[tokio::test]
    async fn delegate_fails_gracefully_with_unknown_peer() {
        let peers = vec![
            PeerNodeConfig { name: "alpha".into(), a2a_base_url: "http://alpha:7779".into() },
        ];
        let result = delegate_capture(&peers, "go2:1", "", Some("gamma")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }
}
