pub mod attestation;
pub mod error;
pub mod firmware;
pub mod observation;

pub use attestation::{AttestationKind, AttestationStatus, WitnessAttestation};
pub use error::WitnessError;
pub use firmware::{FirmwareKind, FirmwareManifest};
pub use observation::{ObservationBundle, PhysicalObservation, SensorReading};

/// Nostr kind for physical witness attestations (matches vcp-types WitnessAttestation).
pub const NOSTR_KIND_WITNESS: u32 = 31020;
/// Nostr kind for twin provenance anchors.
pub const NOSTR_KIND_TWIN_ANCHOR: u32 = 31030;
