//! Vantage heartbeat client.
//!
//! Posts POST /api/me/heartbeat on each VCP scan cycle so the Vantage
//! guild dashboard knows this node is alive and what devices are nearby.
//!
//! Auth: Bearer token from config.vantage.api_token.
//! Body: { work_state, intent, details: { node_name, node_did, ...vcp_summary } }

use serde_json::{json, Value};
use tracing::{debug, warn};

pub struct VantageClient {
    base_url:  String,
    api_token: String,
    client:    reqwest::Client,
}

impl VantageClient {
    pub fn new(base_url: impl Into<String>, api_token: impl Into<String>) -> Self {
        Self {
            base_url:  base_url.into(),
            api_token: api_token.into(),
            client:    reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
        }
    }

    /// POST /api/me/heartbeat with VCP device context.
    /// Non-fatal: logs a warning on failure and returns Ok(()).
    pub async fn post_heartbeat(
        &self,
        node_name: &str,
        node_did:  &str,
        vcp_summary: Value,
    ) {
        let body = json!({
            "work_state": "ALIVE",
            "intent":     "vcp_scan",
            "details": {
                "node_name":          node_name,
                "node_did":           node_did,
                "nearby_vcp_devices": vcp_summary["nearby_vcp_devices"],
                "device_count":       vcp_summary["device_count"],
            }
        });

        let url = format!("{}/api/me/heartbeat", self.base_url);
        match self.client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                debug!(url = %url, "Vantage heartbeat OK");
            }
            Ok(resp) => {
                warn!(
                    url    = %url,
                    status = %resp.status(),
                    "Vantage heartbeat non-2xx"
                );
            }
            Err(e) => {
                warn!(url = %url, error = %e, "Vantage heartbeat failed");
            }
        }
    }
}
