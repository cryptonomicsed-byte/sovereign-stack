//! In-memory store for TwinLicenseGrants — Phase 3.6 twin licensing marketplace.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use twin_protocol::TwinLicenseGrant;

#[derive(Clone, Default)]
pub struct LicenseStore(Arc<RwLock<HashMap<String, TwinLicenseGrant>>>);

impl LicenseStore {
    pub fn new() -> Self { Self::default() }

    pub async fn insert(&self, g: TwinLicenseGrant) {
        self.0.write().await.insert(g.grant_id.clone(), g);
    }

    pub async fn get(&self, grant_id: &str) -> Option<TwinLicenseGrant> {
        self.0.read().await.get(grant_id).cloned()
    }

    pub async fn for_twin(&self, twin_id: &str) -> Vec<TwinLicenseGrant> {
        self.0.read().await.values()
            .filter(|g| g.twin_id == twin_id)
            .cloned()
            .collect()
    }

    pub async fn for_grantee(&self, grantee_did: &str) -> Vec<TwinLicenseGrant> {
        self.0.read().await.values()
            .filter(|g| g.grantee_did == grantee_did)
            .cloned()
            .collect()
    }

    pub async fn all(&self) -> Vec<TwinLicenseGrant> {
        self.0.read().await.values().cloned().collect()
    }

    /// Counter-sign an existing grant (grantee acceptance).
    pub async fn accept(&self, grant_id: &str, grantee_sig: impl Into<String>) -> Option<TwinLicenseGrant> {
        let mut map = self.0.write().await;
        if let Some(g) = map.get_mut(grant_id) {
            g.grantee_sig = Some(grantee_sig.into());
            return Some(g.clone());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twin_protocol::{TwinLicenseGrant, TwinLicenseConstraints};
    use sovereign_types::UsageRight;

    fn sample() -> TwinLicenseGrant {
        TwinLicenseGrant {
            grant_id:    "lic:test-1".into(),
            twin_id:     "sha256:abc".into(),
            grantor_did: "did:key:grantor".into(),
            grantee_did: "did:key:grantee".into(),
            rights:      vec![UsageRight::Simulate],
            constraints: TwinLicenseConstraints::default(),
            issued_at:   0,
            expires_at:  None,
            fee_mist:    None,
            sui_tx:      None,
            merkle_root: "root".into(),
            grantor_sig: "sig".into(),
            grantee_sig: None,
        }
    }

    #[tokio::test]
    async fn insert_and_get() {
        let store = LicenseStore::new();
        store.insert(sample()).await;
        assert!(store.get("lic:test-1").await.is_some());
        assert!(store.get("missing").await.is_none());
    }

    #[tokio::test]
    async fn for_twin_filters() {
        let store = LicenseStore::new();
        store.insert(sample()).await;
        let mut other = sample();
        other.grant_id = "lic:test-2".into();
        other.twin_id  = "sha256:other".into();
        store.insert(other).await;
        assert_eq!(store.for_twin("sha256:abc").await.len(), 1);
        assert_eq!(store.for_twin("sha256:other").await.len(), 1);
    }

    #[tokio::test]
    async fn accept_sets_grantee_sig() {
        let store = LicenseStore::new();
        store.insert(sample()).await;
        let updated = store.accept("lic:test-1", "grantee-sig-hex").await.unwrap();
        assert_eq!(updated.grantee_sig.as_deref(), Some("grantee-sig-hex"));
    }
}
