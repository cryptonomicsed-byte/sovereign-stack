//! Action Receipt Protocol v1 (ARP)
//!
//! One canonical receipt format spanning all 5 primitives of the sovereign
//! ecosystem:  Principal → Capability → Action → Evidence → Receipt
//!
//! ALL consequential system operations MUST produce an ActionReceipt.
//! The `kind` discriminant routes each receipt to the right verifier:
//!
//!   "compute"          → UCX ComputeReceipt wrapper
//!   "vcp_session"      → VCP VcpReceipt wrapper
//!   "emission"         → OSOVM ASE emission event
//!   "twin_capture"     → Nostr kind 31020
//!   "twin_scene"       → Nostr kind 31030
//!   "simulation"       → OSOVM simulation run receipt
//!   "governance"       → Council vote / Bínò veto
//!   "economic"         → Trade, settlement, ASE transfer
//!   "agent_lifecycle"  → Birth, death, tier change
//!   "witness"          → Physical observation by Witness firmware
//!   "mesh_event"       → Agent join/leave/signal on Vantage mesh
//!   "custom"           → Extension point; kind_ext identifies the sub-type
//!
//! The hash chain (`previous_hash`) links consecutive receipts from the same
//! agent into a tamper-evident timeline.

pub mod receipt;
pub mod principal;
pub mod error;

pub use receipt::{
    ActionReceipt, ActionSpec, EvidenceRef, WitnessAttestation,
    ConsensusReceipt, PhysicalAttestation, ReceiptKind,
};
pub use principal::{Principal, PrincipalKind, CapabilityRef};
pub use error::ArpError;
