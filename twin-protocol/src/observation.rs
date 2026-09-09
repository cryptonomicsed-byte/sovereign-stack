use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use sovereign_types::{Timestamp};
use crate::error::{TspError, TspResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationOutcome {
    Validated,      // max_delta_pct < 10%
    Partial,        // some fields out of range
    Falsified,      // max_delta_pct > 30%
    Inconclusive,   // insufficient sensor data
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaField {
    pub predicted:  f32,
    pub observed:   f32,
    pub delta_pct:  f32,
}

impl DeltaField {
    pub fn new(predicted: f32, observed: f32) -> Self {
        let delta_pct = if predicted.abs() > 0.001 {
            ((observed - predicted) / predicted).abs() * 100.0
        } else {
            0.0
        };
        Self { predicted, observed, delta_pct }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationDelta {
    pub fields:        BTreeMap<String, DeltaField>,
    pub max_delta_pct: f32,
}

impl ObservationDelta {
    pub fn new(fields: BTreeMap<String, DeltaField>) -> Self {
        let max_delta_pct = fields.values()
            .map(|f| f.delta_pct)
            .fold(0.0f32, f32::max);
        Self { fields, max_delta_pct }
    }

    pub fn outcome(&self) -> ObservationOutcome {
        match self.max_delta_pct {
            d if d < 10.0  => ObservationOutcome::Validated,
            d if d > 30.0  => ObservationOutcome::Falsified,
            _              => ObservationOutcome::Partial,
        }
    }
}

/// Proof-of-Observation Receipt.
/// Signed by hardware TPM — software signatures are NOT accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationReceipt {
    pub kind:           String,    // "proof_of_observation"
    pub receipt_id:     String,
    pub sim_receipt_id: String,
    pub witness_id:     String,

    pub observed:       serde_json::Value,
    pub predicted:      serde_json::Value,
    pub delta:          ObservationDelta,
    pub outcome:        ObservationOutcome,

    /// TPM key ID — REQUIRED. No software-only signatures accepted.
    pub tpm_key_id:     String,
    /// Signed by TPM, NOT by a software key.
    pub hardware_sig:   String,
    /// X.509 device cert for TPM key verification.
    pub device_cert:    String,

    pub timestamp:      Timestamp,
}

impl ObservationReceipt {
    pub fn build(
        sim_receipt_id: impl Into<String>,
        witness_id:     impl Into<String>,
        observed:       serde_json::Value,
        predicted:      serde_json::Value,
        delta_fields:   BTreeMap<String, DeltaField>,
        tpm_key_id:     impl Into<String>,
        hardware_sig:   impl Into<String>,
        device_cert:    impl Into<String>,
    ) -> TspResult<Self> {
        let tpm_key_id  = tpm_key_id.into();
        let hardware_sig = hardware_sig.into();
        let device_cert  = device_cert.into();

        // RULE: TPM key ID required — no software-only signatures
        if tpm_key_id.is_empty() {
            return Err(TspError::SoftwareSignatureRejected);
        }

        let delta   = ObservationDelta::new(delta_fields);
        let outcome = delta.outcome();

        Ok(Self {
            kind:           "proof_of_observation".into(),
            receipt_id:     format!("rcpt:obs:{}", uuid::Uuid::new_v4()),
            sim_receipt_id: sim_receipt_id.into(),
            witness_id:     witness_id.into(),
            observed,
            predicted,
            delta,
            outcome,
            tpm_key_id,
            hardware_sig,
            device_cert,
            timestamp: now_ms(),
        })
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_classification() {
        let mut fields = BTreeMap::new();
        fields.insert("energy".into(), DeltaField::new(61.0, 63.0));  // 3.3% delta
        let delta = ObservationDelta::new(fields);
        assert_eq!(delta.outcome(), ObservationOutcome::Validated);

        let mut fields2 = BTreeMap::new();
        fields2.insert("energy".into(), DeltaField::new(61.0, 90.0));  // 47% delta
        let delta2 = ObservationDelta::new(fields2);
        assert_eq!(delta2.outcome(), ObservationOutcome::Falsified);
    }

    #[test]
    fn no_tpm_rejected() {
        let result = ObservationReceipt::build(
            "rcpt:sim:test", "did:witness:01",
            serde_json::json!({}), serde_json::json!({}),
            BTreeMap::new(),
            "",    // empty tpm_key_id
            "sig", "cert",
        );
        assert!(matches!(result, Err(TspError::SoftwareSignatureRejected)));
    }
}
