//! OsoMeshEnvelope — compact binary control-plane message for LoRa/Meshtastic.
//!
//! Budget: ≤ 136 bytes serialized (Meshtastic raw payload cap minus headers).
//!
//! Wire format (big-endian, fixed header + variable body):
//!   [0]       version  u8      protocol version (currently 1)
//!   [1]       kind     u8      OsoMeshKind discriminant (5 values)
//!   [2..5]    ts_secs  u32     Unix timestamp truncated to u32 (valid until 2106)
//!   [6..37]   pubkey   [u8;32] sender Ed25519 public key
//!   [38..101] sig      [u8;64] Ed25519 signature over bytes [0..38] + body
//!   [102..]   body     varies  kind-specific payload (≤ 34 bytes)
//!
//! Total header = 102 bytes. Body budget = 34 bytes.
//!
//! The envelope is NOT serde-based on the wire — use `OsoMeshEnvelope::encode`
//! / `::decode` for the compact binary path.  The serde derive is provided for
//! debugging and JSON logging only.

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

pub const OSO_MESH_VERSION: u8 = 1;
/// Maximum total wire bytes (LoRa budget).
pub const OSO_MESH_MAX_BYTES: usize = 136;
pub const OSO_MESH_HEADER_BYTES: usize = 102;
pub const OSO_MESH_BODY_BUDGET: usize = OSO_MESH_MAX_BYTES - OSO_MESH_HEADER_BYTES;

/// The 5 control-plane message kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OsoMeshKind {
    /// Agent is alive — beacon every N seconds.
    Heartbeat   = 0x01,
    /// Request a lifecycle transition (hibernate / wake / terminate).
    LifecycleCmd = 0x02,
    /// Announce presence on the mesh after boot or migration.
    MeshJoin    = 0x03,
    /// Graceful departure — peers should stop expecting heartbeats.
    MeshLeave   = 0x04,
    /// Relay a small OSO-encapsulated upstream message (gateway bridge).
    OrcaRelay   = 0x05,
}

impl OsoMeshKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Heartbeat),
            0x02 => Some(Self::LifecycleCmd),
            0x03 => Some(Self::MeshJoin),
            0x04 => Some(Self::MeshLeave),
            0x05 => Some(Self::OrcaRelay),
            _    => None,
        }
    }
}

/// Kind-specific payload bodies (must serialise within OSO_MESH_BODY_BUDGET).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OsoMeshBody {
    Heartbeat {
        /// Lifecycle stage as a 1-byte discriminant (matches AgentLifecycleStage).
        stage: u8,
        /// Battery level 0–100 (0xFF = not applicable / powered).
        battery: u8,
    },
    LifecycleCmd {
        /// Target transition discriminant (matches TransitionKind byte).
        transition: u8,
        /// Optional short reason code (truncated at 31 bytes).
        reason_code: u8,
    },
    MeshJoin {
        /// Short agent ID fingerprint — first 4 bytes of SHA-256(agent_id).
        agent_id_hint: [u8; 4],
        /// Trust tier 0..=5.
        tier: u8,
    },
    MeshLeave {
        /// 0=graceful, 1=migration, 2=revoked.
        reason: u8,
    },
    OrcaRelay {
        /// Relay payload (up to OSO_MESH_BODY_BUDGET bytes, truncated silently).
        payload: Vec<u8>,
    },
}

impl OsoMeshBody {
    pub fn kind(&self) -> OsoMeshKind {
        match self {
            Self::Heartbeat { .. }    => OsoMeshKind::Heartbeat,
            Self::LifecycleCmd { .. } => OsoMeshKind::LifecycleCmd,
            Self::MeshJoin { .. }     => OsoMeshKind::MeshJoin,
            Self::MeshLeave { .. }    => OsoMeshKind::MeshLeave,
            Self::OrcaRelay { .. }    => OsoMeshKind::OrcaRelay,
        }
    }

    /// Serialize body to bytes (compact, no tag byte — kind is in envelope header).
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Heartbeat { stage, battery } => vec![*stage, *battery],
            Self::LifecycleCmd { transition, reason_code } => vec![*transition, *reason_code],
            Self::MeshJoin { agent_id_hint, tier } => {
                let mut v = agent_id_hint.to_vec();
                v.push(*tier);
                v
            }
            Self::MeshLeave { reason } => vec![*reason],
            Self::OrcaRelay { payload } => {
                // Truncate to body budget.
                payload[..payload.len().min(OSO_MESH_BODY_BUDGET)].to_vec()
            }
        }
    }

    /// Deserialize body from bytes given the kind.
    pub fn decode(kind: OsoMeshKind, data: &[u8]) -> Result<Self, MeshError> {
        match kind {
            OsoMeshKind::Heartbeat => {
                if data.len() < 2 { return Err(MeshError::BodyTooShort); }
                Ok(Self::Heartbeat { stage: data[0], battery: data[1] })
            }
            OsoMeshKind::LifecycleCmd => {
                if data.len() < 2 { return Err(MeshError::BodyTooShort); }
                Ok(Self::LifecycleCmd { transition: data[0], reason_code: data[1] })
            }
            OsoMeshKind::MeshJoin => {
                if data.len() < 5 { return Err(MeshError::BodyTooShort); }
                let mut hint = [0u8; 4];
                hint.copy_from_slice(&data[..4]);
                Ok(Self::MeshJoin { agent_id_hint: hint, tier: data[4] })
            }
            OsoMeshKind::MeshLeave => {
                if data.is_empty() { return Err(MeshError::BodyTooShort); }
                Ok(Self::MeshLeave { reason: data[0] })
            }
            OsoMeshKind::OrcaRelay => {
                Ok(Self::OrcaRelay { payload: data.to_vec() })
            }
        }
    }
}

/// The complete signed mesh envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsoMeshEnvelope {
    pub version:    u8,
    pub kind:       OsoMeshKind,
    /// Unix seconds (u32 — valid until 2106).
    pub ts_secs:    u32,
    /// Sender's Ed25519 public key (32 bytes).
    pub pubkey:     [u8; 32],
    /// Ed25519 signature over `signing_bytes()` (64 bytes, stored as Vec for serde compat).
    pub signature:  Vec<u8>,
    pub body:       OsoMeshBody,
}

impl OsoMeshEnvelope {
    fn sig64(&self) -> [u8; 64] {
        let mut arr = [0u8; 64];
        let n = self.signature.len().min(64);
        arr[..n].copy_from_slice(&self.signature[..n]);
        arr
    }
}

impl OsoMeshEnvelope {
    /// Bytes that are signed: header fields + encoded body (excludes the signature slot).
    pub fn signing_bytes(
        version: u8,
        kind: OsoMeshKind,
        ts_secs: u32,
        pubkey: &[u8; 32],
        body_bytes: &[u8],
    ) -> Vec<u8> {
        let mut v = Vec::with_capacity(OSO_MESH_HEADER_BYTES + body_bytes.len());
        v.push(version);
        v.push(kind as u8);
        v.extend_from_slice(&ts_secs.to_be_bytes());
        v.extend_from_slice(pubkey);
        v.extend_from_slice(body_bytes);
        v
    }

    /// SHA-256 fingerprint of the envelope (for logging / receipt chaining).
    pub fn fingerprint(&self) -> String {
        let body_bytes = self.body.encode();
        let payload = Self::signing_bytes(
            self.version, self.kind, self.ts_secs, &self.pubkey, &body_bytes,
        );
        let mut h = Sha256::new();
        h.update(&payload);
        h.update(&self.signature);
        hex::encode(h.finalize())
    }

    /// Encode to compact binary (≤ OSO_MESH_MAX_BYTES).
    pub fn encode(&self) -> Result<Vec<u8>, MeshError> {
        let body_bytes = self.body.encode();
        let total = OSO_MESH_HEADER_BYTES + body_bytes.len();
        if total > OSO_MESH_MAX_BYTES {
            return Err(MeshError::PayloadOverflow { total, max: OSO_MESH_MAX_BYTES });
        }
        let mut buf = Vec::with_capacity(total);
        buf.push(self.version);
        buf.push(self.kind as u8);
        buf.extend_from_slice(&self.ts_secs.to_be_bytes());
        buf.extend_from_slice(&self.pubkey);
        // Always write exactly 64 bytes for signature slot.
        buf.extend_from_slice(&self.sig64());
        buf.extend_from_slice(&body_bytes);
        Ok(buf)
    }

    /// Decode from compact binary.
    pub fn decode(buf: &[u8]) -> Result<Self, MeshError> {
        if buf.len() < OSO_MESH_HEADER_BYTES {
            return Err(MeshError::BufferTooShort);
        }
        let version = buf[0];
        if version != OSO_MESH_VERSION {
            return Err(MeshError::UnsupportedVersion(version));
        }
        let kind = OsoMeshKind::from_u8(buf[1]).ok_or(MeshError::UnknownKind(buf[1]))?;
        let ts_secs = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]);
        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&buf[6..38]);
        let signature = buf[38..102].to_vec();
        let body = OsoMeshBody::decode(kind, &buf[102..])?;
        Ok(Self { version, kind, ts_secs, pubkey, signature, body })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("buffer too short to decode OsoMeshEnvelope")]
    BufferTooShort,
    #[error("body too short for kind")]
    BodyTooShort,
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),
    #[error("unknown kind byte {0:#04x}")]
    UnknownKind(u8),
    #[error("payload {total} bytes exceeds LoRa budget {max}")]
    PayloadOverflow { total: usize, max: usize },
}

/// Compute the `agent_id_hint` field for MeshJoin from a full agent ID string.
pub fn agent_id_hint(agent_id: &str) -> [u8; 4] {
    let mut h = Sha256::new();
    h.update(agent_id.as_bytes());
    let d = h.finalize();
    [d[0], d[1], d[2], d[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_pubkey() -> [u8; 32] { [0xAB; 32] }
    fn fake_sig() -> Vec<u8>     { vec![0xCD; 64] }

    fn heartbeat_envelope() -> OsoMeshEnvelope {
        OsoMeshEnvelope {
            version:   OSO_MESH_VERSION,
            kind:      OsoMeshKind::Heartbeat,
            ts_secs:   1_000_000,
            pubkey:    fake_pubkey(),
            signature: fake_sig(),
            body:      OsoMeshBody::Heartbeat { stage: 1, battery: 80 },
        }
    }

    #[test]
    fn roundtrip_heartbeat() {
        let env = heartbeat_envelope();
        let buf = env.encode().unwrap();
        assert!(buf.len() <= OSO_MESH_MAX_BYTES, "exceeds LoRa budget: {} bytes", buf.len());
        let decoded = OsoMeshEnvelope::decode(&buf).unwrap();
        assert_eq!(decoded.kind, OsoMeshKind::Heartbeat);
        assert_eq!(decoded.ts_secs, 1_000_000);
        assert_eq!(decoded.pubkey, fake_pubkey());
        if let OsoMeshBody::Heartbeat { stage, battery } = decoded.body {
            assert_eq!(stage, 1);
            assert_eq!(battery, 80);
        } else {
            panic!("wrong body kind");
        }
    }

    #[test]
    fn roundtrip_mesh_join() {
        let hint = agent_id_hint("agent-xyz-123");
        let env = OsoMeshEnvelope {
            version:   OSO_MESH_VERSION,
            kind:      OsoMeshKind::MeshJoin,
            ts_secs:   2_000_000,
            pubkey:    fake_pubkey(),
            signature: fake_sig(),
            body:      OsoMeshBody::MeshJoin { agent_id_hint: hint, tier: 3 },
        };
        let buf = env.encode().unwrap();
        assert!(buf.len() <= OSO_MESH_MAX_BYTES);
        let decoded = OsoMeshEnvelope::decode(&buf).unwrap();
        if let OsoMeshBody::MeshJoin { agent_id_hint, tier } = decoded.body {
            assert_eq!(agent_id_hint, hint);
            assert_eq!(tier, 3);
        } else {
            panic!("wrong body kind");
        }
    }

    #[test]
    fn roundtrip_lifecycle_cmd() {
        let env = OsoMeshEnvelope {
            version:   OSO_MESH_VERSION,
            kind:      OsoMeshKind::LifecycleCmd,
            ts_secs:   3_000_000,
            pubkey:    fake_pubkey(),
            signature: fake_sig(),
            body:      OsoMeshBody::LifecycleCmd { transition: 2, reason_code: 0 },
        };
        let buf = env.encode().unwrap();
        assert!(buf.len() <= OSO_MESH_MAX_BYTES);
        let decoded = OsoMeshEnvelope::decode(&buf).unwrap();
        assert_eq!(decoded.kind, OsoMeshKind::LifecycleCmd);
    }

    #[test]
    fn orca_relay_truncates_at_budget() {
        // 40 bytes > OSO_MESH_BODY_BUDGET (34) but body.encode() truncates
        let big_payload = vec![0u8; 40];
        let body = OsoMeshBody::OrcaRelay { payload: big_payload };
        let encoded = body.encode();
        assert_eq!(encoded.len(), OSO_MESH_BODY_BUDGET);
    }

    #[test]
    fn all_kinds_wire_within_budget() {
        let bodies = vec![
            OsoMeshBody::Heartbeat    { stage: 2, battery: 50 },
            OsoMeshBody::LifecycleCmd { transition: 1, reason_code: 0 },
            OsoMeshBody::MeshJoin     { agent_id_hint: [1,2,3,4], tier: 1 },
            OsoMeshBody::MeshLeave    { reason: 0 },
            OsoMeshBody::OrcaRelay    { payload: vec![0xFE; 10] },
        ];
        for body in bodies {
            let env = OsoMeshEnvelope {
                version:   OSO_MESH_VERSION,
                kind:      body.kind(),
                ts_secs:   0,
                pubkey:    fake_pubkey(),
                signature: fake_sig(),
                body,
            };
            let buf = env.encode().unwrap();
            assert!(
                buf.len() <= OSO_MESH_MAX_BYTES,
                "{:?} encoded to {} bytes (budget {})",
                env.kind, buf.len(), OSO_MESH_MAX_BYTES
            );
        }
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let env = heartbeat_envelope();
        assert_eq!(env.fingerprint(), env.fingerprint());
    }
}
