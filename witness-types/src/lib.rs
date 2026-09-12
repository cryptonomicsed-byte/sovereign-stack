pub mod attestation;
pub mod observation;
pub mod firmware;
pub mod error;

pub use attestation::{WitnessAttestation, AttestationKind, AttestationStatus};
pub use observation::{PhysicalObservation, SensorReading, ObservationBundle};
pub use firmware::{FirmwareManifest, FirmwareKind};
pub use error::WitnessError;

/// Nostr kind for physical witness attestations (matches vcp-types WitnessAttestation).
pub const NOSTR_KIND_WITNESS: u32 = 31020;
/// Nostr kind for twin provenance anchors.
pub const NOSTR_KIND_TWIN_ANCHOR: u32 = 31030;
