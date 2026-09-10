/// Creation Receipt — kind:1901.
///
/// Emitted for every significant creative output (Gaussian splat, scene, simulation, etc.).
/// Unifies OSOVM's RECEIPT opcode emission with Nostr IP provenance.
///
/// For Gaussian splats:
///   - `x` tag = sha256 of the PLY scene data
///   - `osovm_op = RECEIPT, <scene_receipt_id>` (from twin-protocol SceneReceipt)
///   - `twin` tag = kind:1903 Twin Binding event id
///   - `m` tag = "model/splat+ply"
///
/// Schema: schemas/creation_receipt.md in the ip-layer repo.

use crate::nostr::{NostrEvent, NostrSecretKey, sign_event, IpLayerError};

pub const KIND_CREATION_RECEIPT: u32 = 1901;

#[derive(Debug, Clone)]
pub struct PaymentSplit {
    pub npub_hex:     String,
    /// Basis points (sum should equal 10_000 for 100%)
    pub basis_points: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CreationReceiptBuilder {
    /// d-tag value from the IP Root (kind:31900)
    pub ip_root_id:    String,
    /// sha256 of the creative output (content hash, NIP-94 style)
    pub content_hash:  String,
    /// Optional: off-Nostr URL where content lives (Blossom, Walrus, etc.)
    pub content_url:   Option<String>,
    /// MIME type
    pub mime_type:     Option<String>,
    /// License override (if absent, inherits IP Root's default)
    pub license:       Option<String>,
    /// Payment split declarations
    pub splits:        Vec<PaymentSplit>,
    /// Optional: OSOVM RECEIPT opcode id this event publishes
    pub osovm_op:      Option<String>,
    /// Optional: Twin Binding event id (for twin-state snapshots)
    pub twin_id:       Option<String>,
    /// Optional: parent Creation Receipt id (derivative work lineage)
    pub parent_id:     Option<String>,
    /// Human-readable title + description (JSON content field)
    pub title:         String,
    pub description:   String,
}

impl CreationReceiptBuilder {
    pub fn new(ip_root_id: impl Into<String>, content_hash: impl Into<String>) -> Self {
        Self {
            ip_root_id:   ip_root_id.into(),
            content_hash: content_hash.into(),
            ..Default::default()
        }
    }

    /// Constructor for Gaussian splat / SceneReceipt.
    pub fn for_splat(
        ip_root_id:       impl Into<String>,
        splat_sha256:     impl Into<String>,
        scene_receipt_id: impl Into<String>,
        twin_binding_id:  Option<String>,
        title:            impl Into<String>,
    ) -> Self {
        Self {
            ip_root_id:    ip_root_id.into(),
            content_hash:  splat_sha256.into(),
            mime_type:     Some("model/splat+ply".into()),
            osovm_op:      Some(scene_receipt_id.into()),
            twin_id:       twin_binding_id,
            title:         title.into(),
            description:   "Sovereign Gaussian splat scene captured by agent".into(),
            ..Default::default()
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.content_url = Some(url.into()); self
    }
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into()); self
    }
    pub fn with_split(mut self, npub_hex: impl Into<String>, basis_points: u32) -> Self {
        self.splits.push(PaymentSplit { npub_hex: npub_hex.into(), basis_points }); self
    }
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into()); self
    }
    pub fn with_description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into(); self
    }

    pub fn sign(self, seckey: &NostrSecretKey, created_at: u64) -> Result<NostrEvent, IpLayerError> {
        let mut tags = vec![
            vec!["ip_root".into(), self.ip_root_id.clone()],
            vec!["x".into(), self.content_hash.clone()],
            // NIP-32 license label
            vec!["L".into(), "license".into()],
        ];

        // License: per-receipt override or inherit from IP Root
        let license = self.license.as_deref().unwrap_or("cc-by-4.0");
        tags.push(vec!["l".into(), license.into(), "license".into()]);

        if let Some(url) = &self.content_url {
            tags.push(vec!["url".into(), url.clone()]);
        }
        if let Some(mime) = &self.mime_type {
            tags.push(vec!["m".into(), mime.clone()]);
        }
        for split in &self.splits {
            tags.push(vec!["split".into(), split.npub_hex.clone(), split.basis_points.to_string()]);
        }
        if let Some(osovm_id) = &self.osovm_op {
            // osovm_op = RECEIPT opcode name + OSOVM receipt_id
            tags.push(vec!["osovm_op".into(), "RECEIPT".into(), osovm_id.clone()]);
        }
        if let Some(twin) = &self.twin_id {
            tags.push(vec!["twin".into(), twin.clone()]);
        }
        if let Some(parent) = &self.parent_id {
            tags.push(vec!["e".into(), parent.clone(), "".into(), "root".into()]);
        }

        let content = serde_json::to_string(&serde_json::json!({
            "title":       self.title,
            "description": self.description,
        }))?;

        sign_event(KIND_CREATION_RECEIPT, tags, content, seckey, created_at)
    }
}
