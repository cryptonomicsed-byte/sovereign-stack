//! Vantage HTTP client.
//!
//! post_heartbeat: POST /api/me/heartbeat on each VCP scan cycle.
//! post_dip:       POST /api/dip/inbound — forward DIP envelopes to Vantage routing.
//! post_receipt:   POST /api/receipts    — publish scene receipts for explorer indexing.
//!
//! Auth: Bearer token from config.vantage.api_token.
//! All methods are non-fatal: log warn on failure, return immediately.

use serde_json::{json, Value};
use tracing::{debug, info, warn};

use dip::DipEnvelope;

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

    /// POST /api/dip/inbound — forward a DIP envelope to Vantage for destination routing.
    pub async fn post_dip(&self, envelope: &DipEnvelope) {
        let url = format!("{}/api/dip/inbound", self.base_url);
        match self.client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(envelope)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                debug!(msg_id = %envelope.message_id, "DIP envelope forwarded to Vantage");
            }
            Ok(resp) => {
                warn!(
                    msg_id = %envelope.message_id,
                    status = %resp.status(),
                    "Vantage DIP ingest non-2xx"
                );
            }
            Err(e) => {
                warn!(msg_id = %envelope.message_id, error = %e, "Vantage DIP ingest failed");
            }
        }
    }

    /// POST /api/receipts — publish a scene receipt for explorer indexing.
    pub async fn post_receipt(
        &self,
        twin_id:    &str,
        receipt_id: &str,
        device_id:  &str,
        kind:       u32,
        sui_object_id: Option<&str>,
    ) {
        let body = json!({
            "receipt_id":    receipt_id,
            "twin_id":       twin_id,
            "device_id":     device_id,
            "kind":          kind,
            "sui_object_id": sui_object_id,
        });
        let url = format!("{}/api/receipts", self.base_url);
        match self.client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                info!(receipt_id = %receipt_id, twin_id = %twin_id, "receipt published to Vantage");
            }
            Ok(resp) => {
                warn!(receipt_id = %receipt_id, status = %resp.status(), "Vantage receipt non-2xx");
            }
            Err(e) => {
                warn!(receipt_id = %receipt_id, error = %e, "Vantage receipt publish failed");
            }
        }
    }

    /// POST /api/me/heartbeat with VCP device context.
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
