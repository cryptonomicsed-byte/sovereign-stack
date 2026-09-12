use serde::{Deserialize, Serialize};

/// Canonical numeric kind codes for all twin-protocol receipts.
/// These are Nostr-adjacent event kinds in the 31000-series range.
///
/// Witness-firmware (ESP32/LoRa) publishes ReceiptKind::Observation.
/// sovereign_witness.py constructs CanonicalReceipts with kind = 31040.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum ReceiptKind {
    /// Physical twin capture — sensor telemetry + trajectory proof.
    Capture     = 31020,
    /// Scene receipt — Gaussian splat or point-cloud spatial anchor.
    Scene       = 31030,
    /// Observation receipt — hardware-witnessed physical signal (LoRa/TPM).
    /// Produced by Witness-firmware (ESP32 + SX1278 LoRa nodes).
    Observation = 31040,
    /// Simulation receipt — OSOVM deterministic run output.
    Simulation  = 31050,
}

impl ReceiptKind {
    pub fn as_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            31020 => Some(Self::Capture),
            31030 => Some(Self::Scene),
            31040 => Some(Self::Observation),
            31050 => Some(Self::Simulation),
            _     => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capture     => "capture",
            Self::Scene       => "scene",
            Self::Observation => "observation",
            Self::Simulation  => "simulation",
        }
    }
}

impl std::fmt::Display for ReceiptKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<ReceiptKind> for u32 {
    fn from(k: ReceiptKind) -> u32 {
        k as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for (n, k) in [(31020, ReceiptKind::Capture), (31030, ReceiptKind::Scene),
                       (31040, ReceiptKind::Observation), (31050, ReceiptKind::Simulation)] {
            assert_eq!(k.as_u32(), n);
            assert_eq!(ReceiptKind::from_u32(n), Some(k));
        }
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(ReceiptKind::from_u32(99999), None);
    }

    #[test]
    fn observation_is_31040() {
        assert_eq!(ReceiptKind::Observation.as_u32(), 31040);
        assert_eq!(ReceiptKind::Observation.as_str(), "observation");
    }
}
