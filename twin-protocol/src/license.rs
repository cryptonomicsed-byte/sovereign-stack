use serde::{Deserialize, Serialize};
use sovereign_types::{Did, Hash, Signature, Timestamp, UsageRight, LicenseType, sign, merkle_root};
use std::collections::BTreeMap;
use crate::error::TspResult;

/// A license grant — allows a grantee to exercise specific rights over a Twin Asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinLicenseGrant {
    pub grant_id:     String,
    pub twin_id:      Hash,
    pub grantor_did:  Did,
    pub grantee_did:  Did,
    pub rights:       Vec<UsageRight>,
    pub constraints:  TwinLicenseConstraints,
    pub issued_at:    Timestamp,
    pub expires_at:   Option<Timestamp>,
    pub fee_mist:     Option<u64>,        // Sui MIST (1 SUI = 1e9 MIST)
    pub sui_tx:       Option<String>,
    pub merkle_root:  Hash,
    pub grantor_sig:  Signature,
    pub grantee_sig:  Option<Signature>,  // grantee counter-signs on accept
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinLicenseConstraints {
    pub max_sim_runs:   Option<u32>,
    pub sim_agent_id:   Option<String>,   // only this agent may simulate
    pub no_commercial:  bool,
    pub attribution:    bool,
    pub sublicensing:   bool,
}

impl Default for TwinLicenseConstraints {
    fn default() -> Self {
        Self {
            max_sim_runs: None,
            sim_agent_id: None,
            no_commercial: false,
            attribution:   true,
            sublicensing:  false,
        }
    }
}

impl TwinLicenseGrant {
    pub fn issue(
        twin_id:     Hash,
        grantor_did: Did,
        grantee_did: Did,
        rights:      Vec<UsageRight>,
        constraints: TwinLicenseConstraints,
        expires_at:  Option<Timestamp>,
        fee_mist:    Option<u64>,
        private_key: &str,
    ) -> TspResult<Self> {
        let grant_id  = format!("lic:{}", uuid::Uuid::new_v4());
        let issued_at = now_ms();

        let mut fields = BTreeMap::new();
        fields.insert("expires_at",   serde_json::to_value(&expires_at)?);
        fields.insert("grantee_did",  serde_json::json!(&grantee_did));
        fields.insert("grantor_did",  serde_json::json!(&grantor_did));
        fields.insert("grant_id",     serde_json::json!(&grant_id));
        fields.insert("issued_at",    serde_json::json!(issued_at));
        fields.insert("rights",       serde_json::to_value(&rights)?);
        fields.insert("twin_id",      serde_json::json!(&twin_id));

        let root = merkle_root(&fields);
        let sig  = sign(&root, private_key)?;

        Ok(Self {
            grant_id,
            twin_id,
            grantor_did,
            grantee_did,
            rights,
            constraints,
            issued_at,
            expires_at,
            fee_mist,
            sui_tx:      None,
            merkle_root: root,
            grantor_sig: sig,
            grantee_sig: None,
        })
    }

    pub fn has_right(&self, right: &UsageRight) -> bool {
        self.rights.contains(right)
    }

    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            now_ms() > exp
        } else {
            false
        }
    }

    pub fn accept(&mut self, grantee_private_key: &str) -> TspResult<()> {
        let sig = sign(&self.merkle_root, grantee_private_key)?;
        self.grantee_sig = Some(sig);
        Ok(())
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
