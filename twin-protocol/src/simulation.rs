use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use sovereign_types::{IdentityChain, Hash, Signature, Timestamp, WitnessAttestation,
                      merkle_root, sign, hash_str};
use crate::error::{TspError, TspResult};

/// A single candidate policy from a simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimPolicy {
    pub id:         String,
    pub energy:     f32,
    pub risk:       f32,
    pub duration_s: f32,
    pub metrics:    Option<serde_json::Value>,
}

/// Proof-of-Simulation Receipt.
/// Commits to ALL candidate policies — not just the selected one.
/// Requires >= 2 independent witness attestations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationReceipt {
    pub kind:               String,     // "proof_of_simulation"
    pub receipt_id:         String,
    pub twin_id:            Hash,
    pub identity:           IdentityChain,

    pub sim_engine:         String,     // "osovm/2.0"
    pub robot_model:        String,
    pub trajectory_count:   u32,

    /// sha256 over canonical JSON of all_policies — commits to the full set
    pub all_policies_hash:  Hash,
    pub all_policies:       Vec<SimPolicy>,

    pub selected_policy_id: String,
    pub selection_criteria: String,

    /// Binds twin_id + params + all_policies_hash
    pub merkle_commitment:  Hash,

    pub witness_ids:        Vec<String>,
    pub witness_sigs:       Vec<WitnessAttestation>,

    pub timestamp:          Timestamp,
    pub signature:          Signature,
}

impl SimulationReceipt {
    pub fn build(
        identity:           IdentityChain,
        twin_id:            Hash,
        sim_engine:         impl Into<String>,
        robot_model:        impl Into<String>,
        trajectory_count:   u32,
        all_policies:       Vec<SimPolicy>,
        selected_policy_id: impl Into<String>,
        selection_criteria: impl Into<String>,
        witnesses:          Vec<WitnessAttestation>,
        private_key:        &str,
    ) -> TspResult<Self> {
        let selected_policy_id = selected_policy_id.into();

        // RULE: must have >= 2 candidate policies
        if all_policies.len() < 2 {
            return Err(TspError::InsufficientPolicies(all_policies.len()));
        }

        // RULE: selected policy must exist in the list
        if !all_policies.iter().any(|p| p.id == selected_policy_id) {
            return Err(TspError::PolicyNotFound(selected_policy_id));
        }

        // RULE: must have >= 2 witnesses
        if witnesses.len() < 2 {
            return Err(TspError::InsufficientWitnesses(witnesses.len()));
        }

        let receipt_id = format!("rcpt:sim:{}", uuid::Uuid::new_v4());
        let timestamp  = now_ms();
        let sim_engine = sim_engine.into();
        let robot_model = robot_model.into();

        // Hash ALL policies (commit to the full set, not just winner)
        let policies_json = serde_json::to_string(&all_policies)?;
        let all_policies_hash = hash_str(&policies_json);

        // Merkle commitment: binds twin_id + engine + trajectory_count + all_policies_hash
        let mut fields = BTreeMap::new();
        fields.insert("all_policies_hash",  serde_json::json!(&all_policies_hash));
        fields.insert("robot_model",        serde_json::json!(&robot_model));
        fields.insert("sim_engine",         serde_json::json!(&sim_engine));
        fields.insert("trajectory_count",   serde_json::json!(trajectory_count));
        fields.insert("twin_id",            serde_json::json!(&twin_id));

        let commitment = merkle_root(&fields);

        // Verify all witnesses attested to this same commitment
        for w in &witnesses {
            if w.merkle_commitment != commitment {
                return Err(TspError::Sovereign(
                    sovereign_types::SovereignError::MerkleRootMismatch {
                        expected: commitment.clone(),
                        got: w.merkle_commitment.clone(),
                    }
                ));
            }
        }

        let sig = sign(&commitment, private_key)?;
        let witness_ids: Vec<String> = witnesses.iter().map(|w| w.witness_id.clone()).collect();

        Ok(Self {
            kind: "proof_of_simulation".into(),
            receipt_id,
            twin_id,
            identity,
            sim_engine,
            robot_model,
            trajectory_count,
            all_policies_hash,
            all_policies,
            selected_policy_id,
            selection_criteria: selection_criteria.into(),
            merkle_commitment: commitment,
            witness_ids,
            witness_sigs: witnesses,
            timestamp,
            signature: sig,
        })
    }

    pub fn selected_policy(&self) -> Option<&SimPolicy> {
        self.all_policies.iter().find(|p| p.id == self.selected_policy_id)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovereign_types::crypto::generate_keypair;

    fn make_witnesses(commitment: &str, n: usize) -> Vec<WitnessAttestation> {
        (0..n).map(|i| {
            let (priv_key, _) = generate_keypair();
            let sig = sign(commitment, &priv_key).unwrap();
            WitnessAttestation {
                witness_id: format!("did:witness:node0{}", i),
                merkle_commitment: commitment.to_string(),
                timestamp: 0,
                signature: sig,
            }
        }).collect()
    }

    #[test]
    fn requires_two_policies() {
        let (priv_key, _) = generate_keypair();
        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let policies = vec![SimPolicy { id: "A".into(), energy: 60.0, risk: 0.1, duration_s: 20.0, metrics: None }];
        // Need to figure out the commitment first for witnesses, so just test the error
        let result = SimulationReceipt::build(
            identity, "twin:sha256:test".into(), "osovm/2.0", "go2", 100,
            policies, "A", "min_risk", vec![], &priv_key
        );
        assert!(matches!(result, Err(TspError::InsufficientPolicies(1))));
    }

    #[test]
    fn selected_must_be_in_list() {
        let (priv_key, _) = generate_keypair();
        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let policies = vec![
            SimPolicy { id: "A".into(), energy: 60.0, risk: 0.1, duration_s: 20.0, metrics: None },
            SimPolicy { id: "B".into(), energy: 70.0, risk: 0.05, duration_s: 25.0, metrics: None },
        ];
        let result = SimulationReceipt::build(
            identity, "twin:sha256:test".into(), "osovm/2.0", "go2", 100,
            policies, "C", "min_risk", vec![], &priv_key
        );
        assert!(matches!(result, Err(TspError::PolicyNotFound(_))));
    }

    #[test]
    fn requires_two_witnesses() {
        let (priv_key, _) = generate_keypair();
        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let policies = vec![
            SimPolicy { id: "A".into(), energy: 60.0, risk: 0.1, duration_s: 20.0, metrics: None },
            SimPolicy { id: "B".into(), energy: 70.0, risk: 0.05, duration_s: 25.0, metrics: None },
        ];
        let result = SimulationReceipt::build(
            identity, "twin:sha256:test".into(), "osovm/2.0", "go2", 100,
            policies, "A", "min_risk", vec![], &priv_key
        );
        assert!(matches!(result, Err(TspError::InsufficientWitnesses(0))));
    }
}
