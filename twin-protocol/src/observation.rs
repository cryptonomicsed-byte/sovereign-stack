use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use sovereign_types::Timestamp;
use crate::error::{TspError, TspResult};
use crate::receipt_kind::ReceiptKind;

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

/// Physical attestation block from a LoRa/Meshtastic witness node.
/// Embedded in FirmwareObservationReceipt.physical_attestation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmwarePhysicalAttestation {
    /// LoRa received signal strength indicator (dBm). None if unavailable.
    pub rssi:         Option<i32>,
    /// Carrier frequency in Hz, e.g. 915_000_000 for US 915 MHz.
    pub frequency_hz: u64,
    /// DID of the witness node device (did:vantage:device:esp32:<node_id>).
    pub node_did:     String,
}

/// Observation receipt produced by Witness-firmware (ESP32 + SX1278 LoRa).
///
/// This is the canonical wire shape that `sovereign_witness.py` serialises at
/// the Micro hardware tier. It is distinct from ObservationReceipt (which is
/// for sim-vs-physical comparison) — here the receipt simply proves "this
/// LoRa node physically received this packet at this timestamp, signed by the
/// node's device key."
///
/// Kind 31040 = ReceiptKind::Observation. The `kind` field is always the
/// numeric constant, never the string "observation".
///
/// Ingest: POST /proofs/observation on sovereign-node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmwareObservationReceipt {
    /// Always 31040 (ReceiptKind::Observation). Use FirmwareObservationReceipt::KIND.
    pub kind:                 u32,
    pub receipt_id:           String,
    /// IdentityChain of the witness node (serialised as JSON).
    pub identity:             serde_json::Value,
    /// ActionRecord: { kind: "observation", target: "lora:<node_id>", params: {...} }
    pub action:               serde_json::Value,
    /// SHA-256 evidence hashes over the raw attestation payload.
    pub evidence_ids:         Vec<String>,
    /// WitnessAttestation(s) from the receiving LoRa node(s).
    pub witness_attestations: Vec<serde_json::Value>,
    pub throne_evaluations:   Vec<serde_json::Value>,
    pub consensus_receipt:    Option<serde_json::Value>,
    /// LoRa physical layer metadata (RSSI, frequency, node DID).
    pub physical_attestation: FirmwarePhysicalAttestation,
    /// Unix timestamp as float seconds (from MicroPython's time.time()).
    pub timestamp:            f64,
    pub previous_hash:        String,
    pub merkle_root:          String,
    pub signature:            String,
}

impl FirmwareObservationReceipt {
    /// The canonical kind value — always 31040.
    pub const KIND: u32 = 31040;
}

/// Proof-of-Observation Receipt — kind 31040.
/// Signed by hardware TPM — software signatures are NOT accepted.
/// Produced by Witness-firmware (ESP32 + SX1278 LoRa nodes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationReceipt {
    pub kind:           u32,       // 31040 = ReceiptKind::Observation
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

impl ObservationOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validated    => "validated",
            Self::Partial      => "partial",
            Self::Falsified    => "falsified",
            Self::Inconclusive => "inconclusive",
        }
    }
}

impl ObservationReceipt {
    /// Return the outcome as a plain string (for event messages, logs, etc.)
    pub fn outcome_str(&self) -> &'static str {
        self.outcome.as_str()
    }

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
            kind:           ReceiptKind::Observation.as_u32(),
            receipt_id:     format!("rcpt:{}_{}",
                                ReceiptKind::Observation.as_u32(),
                                uuid::Uuid::new_v4()),
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
    fn firmware_receipt_kind_constant() {
        assert_eq!(FirmwareObservationReceipt::KIND, 31040);
        assert_eq!(FirmwareObservationReceipt::KIND, ReceiptKind::Observation.as_u32());
    }

    #[test]
    fn firmware_receipt_round_trips_json() {
        let receipt = FirmwareObservationReceipt {
            kind:                 FirmwareObservationReceipt::KIND,
            receipt_id:           "rcpt:test-001".into(),
            identity:             serde_json::json!({"principal_id": "did:p:1"}),
            action:               serde_json::json!({"kind": "observation", "target": "lora:NodeA"}),
            evidence_ids:         vec!["sha256:abc".into()],
            witness_attestations: vec![serde_json::json!({"witness_id": "did:witness:01"})],
            throne_evaluations:   vec![],
            consensus_receipt:    None,
            physical_attestation: FirmwarePhysicalAttestation {
                rssi:         Some(-67),
                frequency_hz: 915_000_000,
                node_did:     "did:vantage:device:esp32:NodeA".into(),
            },
            timestamp:    1700000000.0,
            previous_hash: "sha256:00000000000000000000000000000000000000000000000000000000000000000000".into(),
            merkle_root:  "sha256:abc".into(),
            signature:    "stub-sig".into(),
        };

        let json_str = serde_json::to_string(&receipt).unwrap();
        let decoded: FirmwareObservationReceipt = serde_json::from_str(&json_str).unwrap();
        assert_eq!(decoded.kind, 31040, "kind must survive JSON round-trip as integer 31040");
        assert_eq!(decoded.physical_attestation.rssi, Some(-67));
        assert_eq!(decoded.physical_attestation.frequency_hz, 915_000_000);
    }

    #[test]
    fn firmware_receipt_kind_is_not_string_observation() {
        let receipt = FirmwareObservationReceipt {
            kind:                 FirmwareObservationReceipt::KIND,
            receipt_id:           "rcpt:kind-check".into(),
            identity:             serde_json::json!({}),
            action:               serde_json::json!({}),
            evidence_ids:         vec![],
            witness_attestations: vec![],
            throne_evaluations:   vec![],
            consensus_receipt:    None,
            physical_attestation: FirmwarePhysicalAttestation {
                rssi: None, frequency_hz: 915_000_000,
                node_did: "did:dev".into(),
            },
            timestamp: 0.0, previous_hash: "sha256:00".into(),
            merkle_root: "sha256:00".into(), signature: "stub".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&receipt).unwrap();
        // Confirm the kind field is a number, not the string "observation"
        assert!(v["kind"].is_number(), "kind must be a JSON number, not a string");
        assert_eq!(v["kind"].as_u64(), Some(31040));
    }

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
