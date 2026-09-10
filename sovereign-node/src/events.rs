use serde::{Deserialize, Serialize};

/// Events broadcast over the twin_events channel.
/// Subscribers on /ws/twin/:id receive these as JSON text frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TwinEvent {
    /// Capture pipeline completed successfully.
    CaptureComplete {
        twin_id:    String,
        device_id:  String,
        receipt_id: String,
        job_id:     String,
    },
    /// Capture pipeline failed.
    CaptureFailed {
        job_id:    String,
        device_id: String,
        reason:    String,
    },
    /// Intermediate status update (e.g. splat training started).
    StatusUpdate {
        job_id:  String,
        message: String,
    },
    /// Àṣẹ tokens minted after proof evaluation cleared the eligibility gate.
    /// The 3.69% Éṣù tithe routes through the Elegbára router → 8 sub-wallets.
    MintApproved {
        proof_id:      String,
        proof_domain:  String,  // "simulation" | "spatial" | "physical"
        tile_id:       String,
        minter_did:    String,
        tokens_minted: u64,     // gross micro-Àṣẹ
        net_minted:    u64,     // after 3.69% Éṣù tithe
        owner_fee:     u64,     // to tile owner
        eshu_tithe:    u64,     // total tithe routed to Elegbára router
        tx_digest:     Option<String>,
        stub:          bool,
    },
}

impl TwinEvent {
    /// Return the twin_id for filter matching, if applicable.
    pub fn twin_id(&self) -> Option<&str> {
        match self {
            TwinEvent::CaptureComplete { twin_id, .. } => Some(twin_id),
            TwinEvent::CaptureFailed { .. }
            | TwinEvent::StatusUpdate { .. }
            | TwinEvent::MintApproved { .. } => None,
        }
    }

    pub fn job_id(&self) -> &str {
        match self {
            TwinEvent::CaptureComplete { job_id, .. } => job_id,
            TwinEvent::CaptureFailed { job_id, .. } => job_id,
            TwinEvent::StatusUpdate { job_id, .. } => job_id,
            TwinEvent::MintApproved { proof_id, .. } => proof_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_has_twin_id() {
        let ev = TwinEvent::CaptureComplete {
            twin_id:   "twin:abc".into(),
            device_id: "go2:1".into(),
            receipt_id: "rec:1".into(),
            job_id:    "job:1".into(),
        };
        assert_eq!(ev.twin_id(), Some("twin:abc"));
        assert_eq!(ev.job_id(), "job:1");
    }

    #[test]
    fn failed_has_no_twin_id() {
        let ev = TwinEvent::CaptureFailed {
            job_id: "job:2".into(), device_id: "go2:1".into(), reason: "err".into(),
        };
        assert!(ev.twin_id().is_none());
        assert_eq!(ev.job_id(), "job:2");
    }

    #[test]
    fn serialises_with_event_tag() {
        let ev = TwinEvent::CaptureComplete {
            twin_id: "t".into(), device_id: "d".into(),
            receipt_id: "r".into(), job_id: "j".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains(r#""event":"capture_complete""#));
    }
}
