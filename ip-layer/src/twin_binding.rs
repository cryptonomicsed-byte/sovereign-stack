/// Twin Binding — kind:1903.
///
/// One-time (or rarely-updated) claim: "this sim_id IS the digital twin of this IP Root."
/// This is identity/ownership, not a creative event.
///
/// For Gaussian splatting:
///   - ip_root     = agent's IP Root d-tag value
///   - sim_id      = twin_id from sovereign-node/TwinAsset
///   - twin_kind   = "gaussian-splat-1to1"
///   - osovm_op    = RECEIPT, <scene_receipt_id> (genesis capture)
///
/// Schema: schemas/twin_binding.md in the ip-layer repo.

use crate::nostr::{NostrEvent, NostrSecretKey, sign_event, IpLayerError};

pub const KIND_TWIN_BINDING: u32 = 1903;

#[derive(Debug, Clone)]
pub enum TwinKind {
    /// 1:1 Gaussian splat of a physical scene/device
    GaussianSplat1to1,
    /// 1:1 VeilSim digital twin
    VeilSim1to1,
    Custom(String),
}

impl TwinKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::GaussianSplat1to1 => "gaussian-splat-1to1",
            Self::VeilSim1to1       => "veilsim-1to1",
            Self::Custom(s)         => s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TwinBindingBuilder {
    /// IP Root d-tag of the physical agent/device
    pub ip_root_id:    String,
    /// The twin/simulation id (matches TwinAsset.twin_id in sovereign-node)
    pub sim_id:        String,
    pub twin_kind:     TwinKind,
    /// Optional self-declared fidelity claim
    pub fidelity:      Option<String>,
    /// Optional: genesis veil/capture receipt id
    pub genesis_op_id: Option<String>,
    pub notes:         String,
}

impl TwinBindingBuilder {
    pub fn new(ip_root_id: impl Into<String>, sim_id: impl Into<String>, kind: TwinKind) -> Self {
        Self {
            ip_root_id:    ip_root_id.into(),
            sim_id:        sim_id.into(),
            twin_kind:     kind,
            fidelity:      None,
            genesis_op_id: None,
            notes:         String::new(),
        }
    }

    /// Constructor for a Gaussian splat twin.
    pub fn for_splat(
        ip_root_id:       impl Into<String>,
        twin_id:          impl Into<String>,
        scene_receipt_id: impl Into<String>,
        f1_score:         Option<f32>,
    ) -> Self {
        Self {
            ip_root_id:    ip_root_id.into(),
            sim_id:        twin_id.into(),
            twin_kind:     TwinKind::GaussianSplat1to1,
            fidelity:      f1_score.map(|f| format!("{:.4}", f)),
            genesis_op_id: Some(scene_receipt_id.into()),
            notes:         "Gaussian splat twin bound at first successful capture".into(),
        }
    }

    pub fn with_fidelity(mut self, f: impl Into<String>) -> Self {
        self.fidelity = Some(f.into()); self
    }

    pub fn sign(self, seckey: &NostrSecretKey, created_at: u64) -> Result<NostrEvent, IpLayerError> {
        let mut tags = vec![
            vec!["ip_root".into(), self.ip_root_id.clone()],
            vec!["sim_id".into(), self.sim_id.clone()],
            vec!["twin_kind".into(), self.twin_kind.as_str().into()],
        ];

        if let Some(fidelity) = &self.fidelity {
            tags.push(vec!["fidelity".into(), fidelity.clone()]);
        }
        if let Some(genesis_id) = &self.genesis_op_id {
            tags.push(vec!["osovm_op".into(), "RECEIPT".into(), genesis_id.clone()]);
        }

        let content = serde_json::to_string(&serde_json::json!({ "notes": self.notes }))?;

        sign_event(KIND_TWIN_BINDING, tags, content, seckey, created_at)
    }
}
