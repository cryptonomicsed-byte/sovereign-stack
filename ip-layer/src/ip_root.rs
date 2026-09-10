/// IP Root — kind:31900 (NIP-33 parameterized replaceable).
///
/// One per creator/agent. Published at birth; updateable (same `d` tag = same address).
/// Owns all Creation Receipts (1901), Attestations (1902), Twin Bindings (1903) under it.
///
/// Schema: schemas/ip_root.md in the ip-layer repo.

use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::nostr::{NostrEvent, NostrSecretKey, sign_event, IpLayerError};

pub const KIND_IP_ROOT: u32 = 31900;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRootMetadata {
    pub display_name: String,
    /// "omo-koda2" | "external" | custom
    pub framework:    String,
    pub notes:        Option<String>,
}

#[derive(Debug, Clone)]
pub struct IpRootBuilder {
    /// The `d` tag — stable addressable id. For Omo-Koda2 agents, this is the pubkey.
    pub ip_root_id:   String,
    /// Optional: distinct human owner if agent acts on behalf of one
    pub owner_npub:   Option<String>,
    /// Optional: BIPON39 soul-binding reference
    pub soul_binding: Option<String>,
    /// Default license stance (SPDX id or URI)
    pub license:      String,
    /// Optional Wyoming DAO LLC membership
    pub dao_id:       Option<String>,
    /// Optional: link to genesis/birth receipt event id
    pub genesis_id:   Option<String>,
    pub metadata:     IpRootMetadata,
}

impl IpRootBuilder {
    /// Minimal IP Root — just the agent's own pubkey as the root id.
    pub fn for_agent(pubkey_hex: &str) -> Self {
        Self {
            ip_root_id:   pubkey_hex.to_string(),
            owner_npub:   None,
            soul_binding: None,
            license:      "cc-by-4.0".into(),
            dao_id:       None,
            genesis_id:   None,
            metadata:     IpRootMetadata {
                display_name: pubkey_hex[..8].to_string(),
                framework:    "omo-koda2".into(),
                notes:        None,
            },
        }
    }

    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.metadata.display_name = name.into();
        self
    }

    pub fn with_owner(mut self, owner_npub: impl Into<String>) -> Self {
        self.owner_npub = Some(owner_npub.into());
        self
    }

    pub fn with_soul(mut self, soul_pubkey: impl Into<String>) -> Self {
        self.soul_binding = Some(soul_pubkey.into());
        self
    }

    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = license.into();
        self
    }

    pub fn with_dao(mut self, dao_id: impl Into<String>) -> Self {
        self.dao_id = Some(dao_id.into());
        self
    }

    pub fn with_genesis(mut self, genesis_receipt_id: impl Into<String>) -> Self {
        self.genesis_id = Some(genesis_receipt_id.into());
        self
    }

    /// Build and sign the kind:31900 Nostr event.
    pub fn sign(self, seckey: &NostrSecretKey, created_at: u64) -> Result<NostrEvent, IpLayerError> {
        let mut tags = vec![
            vec!["d".into(), self.ip_root_id.clone()],
            vec!["license".into(), self.license.clone()],
        ];

        if let Some(owner) = &self.owner_npub {
            tags.push(vec!["owner".into(), owner.clone()]);
        }
        if let Some(soul) = &self.soul_binding {
            tags.push(vec!["soul".into(), soul.clone()]);
        }
        if let Some(dao) = &self.dao_id {
            tags.push(vec!["dao".into(), dao.clone()]);
        }
        if let Some(genesis) = &self.genesis_id {
            tags.push(vec!["genesis".into(), genesis.clone()]);
        }

        let content = serde_json::to_string(&self.metadata)?;

        sign_event(KIND_IP_ROOT, tags, content, seckey, created_at)
    }
}

/// Parsed IP Root event — deserialized from a kind:31900 Nostr event.
#[derive(Debug, Clone)]
pub struct IpRoot {
    pub event_id:     String,
    pub pubkey:       String,
    pub ip_root_id:   String,   // d tag
    pub owner_npub:   Option<String>,
    pub soul_binding: Option<String>,
    pub license:      String,
    pub dao_id:       Option<String>,
    pub genesis_id:   Option<String>,
    pub metadata:     IpRootMetadata,
    pub created_at:   u64,
}

impl IpRoot {
    /// Parse from a raw NostrEvent.
    pub fn from_event(ev: &NostrEvent) -> Result<Self, IpLayerError> {
        if ev.kind != KIND_IP_ROOT {
            return Err(IpLayerError::Validation(
                format!("expected kind {KIND_IP_ROOT}, got {}", ev.kind)
            ));
        }

        let tag_val = |name: &str| -> Option<String> {
            ev.tags.iter()
                .find(|t| t.first().map(|s| s == name).unwrap_or(false))
                .and_then(|t| t.get(1).cloned())
        };

        let ip_root_id = tag_val("d")
            .ok_or_else(|| IpLayerError::Validation("missing d tag".into()))?;

        let metadata: IpRootMetadata = serde_json::from_str(&ev.content)?;

        Ok(Self {
            event_id:     ev.id.clone(),
            pubkey:       ev.pubkey.clone(),
            ip_root_id,
            owner_npub:   tag_val("owner"),
            soul_binding: tag_val("soul"),
            license:      tag_val("license").unwrap_or_else(|| "cc-by-4.0".into()),
            dao_id:       tag_val("dao"),
            genesis_id:   tag_val("genesis"),
            metadata,
            created_at:   ev.created_at,
        })
    }
}
