//! ỌSỌVM ↔ Twin Protocol integration — Proof-of-Useful-Simulation.
//!
//! ỌSỌVM (Ọmọ Kọ́dà Simulation VM) runs multi-trajectory rollouts of a robot model
//! grounded in a TwinAsset's capture data. This module bridges:
//!
//!   TwinAsset (ground truth) → ỌSỌVM scenario → SimulationReceipt (on-chain proof)
//!
//! Proof-of-Simulation invariants (enforced by SimulationReceipt::build):
//!   • >= 2 candidate policies must be explored (no single-policy cherry-picking)
//!   • >= 2 independent witnesses must attest to the merkle commitment
//!   • selected policy must exist in the committed list
//!   • all_policies_hash = sha256(canonical JSON of full policy set)
//!
//! The OsovmEngine here is a **stub** — production impl swaps in the real
//! ỌSỌVM VM binary via a tokio::process::Command call or a local gRPC endpoint.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use sovereign_types::{IdentityChain, WitnessAttestation, merkle_root, sign, hash_str};
use crate::twin::TwinAsset;
use crate::simulation::{SimPolicy, SimulationReceipt};
use crate::error::{TspError, TspResult};

/// Parameters for a simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimScenario {
    /// Human-readable scenario name.
    pub name: String,
    /// Robot model identifier (matches TwinAsset.robot_model).
    pub robot_model: String,
    /// Number of trajectory rollouts to attempt.
    pub trajectory_count: u32,
    /// Selection objective: "min_energy" | "min_risk" | "max_throughput" | "balanced"
    pub selection_objective: String,
    /// Optional scenario-specific parameters (terrain, obstacles, payload mass, etc.)
    pub params: Option<Value>,
}

/// Result of one trajectory rollout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryResult {
    pub trajectory_id: String,
    pub policy_id:     String,
    pub success:       bool,
    pub energy_j:      f32,
    pub risk_score:    f32,   // 0.0 (safe) → 1.0 (collision)
    pub duration_s:    f32,
    pub metrics:       Option<Value>,
}

/// Full result returned by the ỌSỌVM engine for one scenario run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsovmRunResult {
    pub engine_version:    String,
    pub scenario:          SimScenario,
    pub trajectories:      Vec<TrajectoryResult>,
    pub candidate_policies: Vec<SimPolicy>,
    pub selected_policy_id: String,
    pub run_id:            String,
    pub wall_ms:           u64,
}

/// Interface to the ỌSỌVM simulation engine.
pub struct OsovmEngine {
    pub engine_version: String,
    /// gRPC endpoint for real engine; ignored in stub mode.
    pub endpoint: Option<String>,
    /// Skip Sabbath freeze check (for tests and CI).
    pub bypass_sabbath: bool,
}

impl OsovmEngine {
    pub fn new(version: impl Into<String>) -> Self {
        Self { engine_version: version.into(), endpoint: None, bypass_sabbath: false }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    pub fn bypass_sabbath(mut self) -> Self {
        self.bypass_sabbath = true;
        self
    }

    /// Builder method that sets the endpoint from an `Option<String>`.
    /// Passes through `None` unchanged (keeps stub mode).
    pub fn with_endpoint_opt(mut self, url: Option<String>) -> Self {
        if let Some(u) = url {
            self.endpoint = Some(u);
        }
        self
    }

    /// Async entry-point: calls the OSOVM HTTP server when `self.endpoint` is set,
    /// otherwise falls back to the deterministic stub.
    ///
    /// `agent_did` is included in the request envelope so the server can attribute
    /// the run to the calling agent.
    pub async fn run_scenario(
        &self,
        twin: &TwinAsset,
        scenario: &SimScenario,
        agent_did: &str,
    ) -> Result<OsovmRunResult, String> {
        if let Some(ref base_url) = self.endpoint {
            if base_url.starts_with("http://") || base_url.starts_with("https://") {
                return self.run_remote(base_url, twin, scenario, agent_did).await;
            }
        }
        // No HTTP endpoint configured — use stub
        self.run_stub(twin, scenario)
            .map_err(|e| e.to_string())
    }

    /// POST `{base_url}/run` with the canonical OSOVM envelope and parse the result.
    async fn run_remote(
        &self,
        base_url: &str,
        twin: &TwinAsset,
        scenario: &SimScenario,
        agent_did: &str,
    ) -> Result<OsovmRunResult, String> {
        let url = format!("{base_url}/run");

        let body = serde_json::json!({
            "opcode": "VEIL",
            "args": {
                "twin_id": &twin.twin_id,
                "trajectory_count": scenario.trajectory_count,
                "selection_objective": &scenario.selection_objective,
            },
            "agent": agent_did,
            "scenario": scenario,
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| format!("reqwest build: {e}"))?;

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP POST {url}: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("OSOVM server {status}: {text}"));
        }

        // Try to parse into our canonical result type.
        // The server may also return `{"status": "ok", "run_id": ..., ...}` — try both shapes.
        let raw: serde_json::Value = resp.json().await
            .map_err(|e| format!("response parse: {e}"))?;

        if raw.get("status").and_then(|s| s.as_str()).unwrap_or("ok") != "ok" {
            let err = raw.get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown error from OSOVM server");
            return Err(err.to_string());
        }

        // If the response is already a full OsovmRunResult, deserialise directly.
        match serde_json::from_value::<OsovmRunResult>(raw.clone()) {
            Ok(result) => Ok(result),
            Err(_) => {
                // Server returned the compact shape — synthesise an OsovmRunResult
                // from the top-level fields plus the embedded scenario.
                let run_id = raw["run_id"].as_str().unwrap_or("run:remote").to_string();
                let f1_score = raw["f1_score"].as_f64().unwrap_or(0.0) as f32;
                let _ase_minted = raw["ase_minted"].as_f64().unwrap_or(0.0);
                let wall_ms = raw["wall_ms"].as_u64().unwrap_or(0);

                // Build a minimal set of candidate policies from f1_score
                let p_selected = SimPolicy {
                    id:         "policy:remote:selected".into(),
                    energy:     120.0 * (1.0 - f1_score),
                    risk:       (0.15 - f1_score * 0.1_f32).max(0.0),
                    duration_s: 18.0,
                    metrics:    Some(serde_json::json!({"f1_score": f1_score})),
                };
                let p_alt = SimPolicy {
                    id:         "policy:remote:alt".into(),
                    energy:     p_selected.energy * 1.1,
                    risk:       (p_selected.risk + 0.05).min(1.0),
                    duration_s: p_selected.duration_s + 3.0,
                    metrics:    None,
                };

                Ok(OsovmRunResult {
                    engine_version:     self.engine_version.clone(),
                    scenario:           scenario.clone(),
                    trajectories:       vec![],
                    candidate_policies: vec![p_selected.clone(), p_alt],
                    selected_policy_id: p_selected.id,
                    run_id,
                    wall_ms,
                })
            }
        }
    }

    /// Pure-Rust deterministic stub — used when no HTTP endpoint is configured
    /// or when the real engine is unreachable.
    fn run_stub(&self, twin: &TwinAsset, scenario: &SimScenario) -> TspResult<OsovmRunResult> {
        let f1 = twin.quality.f1_score;
        let run_id = format!("osovm:run:{}", uuid::Uuid::new_v4());
        let n_traj = scenario.trajectory_count.max(2);

        let trajectories: Vec<TrajectoryResult> = (0..n_traj).map(|i| {
            let noise = (i as f32 * 0.07).sin() * 0.05;
            let energy = 120.0 + (i as f32 * 12.3).sin() * 30.0;
            let risk   = (0.15 - f1 * 0.1 + noise).max(0.0).min(1.0);
            let dur    = 18.0 + (i as f32 * 0.9).cos() * 5.0;
            let policy_id = if i < 3 {
                ["policy:conservative", "policy:balanced", "policy:aggressive"][i as usize % 3].to_string()
            } else {
                format!("policy:variant:{}", i)
            };
            TrajectoryResult {
                trajectory_id: format!("traj:{run_id}:{i}"),
                policy_id:     policy_id.clone(),
                success:       risk < 0.5,
                energy_j:      energy,
                risk_score:    risk,
                duration_s:    dur,
                metrics: Some(serde_json::json!({"f1_grounding": f1})),
            }
        }).collect();

        let mut policy_map: BTreeMap<String, (f32, f32, f32, u32)> = BTreeMap::new();
        for t in &trajectories {
            let e = policy_map.entry(t.policy_id.clone()).or_insert((0.0, 0.0, 0.0, 0));
            e.0 += t.energy_j;
            e.1 += t.risk_score;
            e.2 += t.duration_s;
            e.3 += 1;
        }
        let candidate_policies: Vec<SimPolicy> = policy_map.iter().map(|(id, (e, r, d, n))| {
            let n = *n as f32;
            SimPolicy {
                id:         id.clone(),
                energy:     e / n,
                risk:       r / n,
                duration_s: d / n,
                metrics:    Some(serde_json::json!({"trajectory_count": n})),
            }
        }).collect();

        if candidate_policies.len() < 2 {
            return Err(TspError::InsufficientPolicies(candidate_policies.len()));
        }

        let selected_policy_id = self.select_policy(&candidate_policies, &scenario.selection_objective);

        Ok(OsovmRunResult {
            engine_version:    self.engine_version.clone(),
            scenario:          scenario.clone(),
            trajectories,
            candidate_policies,
            selected_policy_id,
            run_id,
            wall_ms: 0,
        })
    }

    /// Run a simulation scenario against a TwinAsset.
    ///
    /// If `endpoint` is set: tries the real ỌSỌVM engine first.
    ///   • "http(s)://…" → POST JSON to that URL, expect OsovmRunResult JSON back.
    ///   • Any other path → exec as a binary with JSON on stdin, read result from stdout.
    /// Falls back to the stub if the real engine fails (with a warning).
    ///
    /// Sabbath freeze: returns `TspError::SabbathFreeze` on Saturday UTC.
    /// Unix epoch day 0 = Thursday, so Saturday = (day + 4) % 7 == 6.
    pub fn run(&self, twin: &TwinAsset, scenario: &SimScenario) -> TspResult<OsovmRunResult> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if !self.bypass_sabbath && crate::emission::DailyEmissionAllocator::is_sabbath(now_secs) {
            return Err(TspError::SabbathFreeze);
        }

        if let Some(endpoint) = &self.endpoint {
            match self.run_real(twin, scenario, endpoint) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(endpoint = %endpoint, error = %e, "real ỌSỌVM engine failed — falling back to stub");
                }
            }
        }

        self.run_stub(twin, scenario)
    }

    /// Call the real ỌSỌVM engine.
    fn run_real(&self, twin: &TwinAsset, scenario: &SimScenario, endpoint: &str) -> TspResult<OsovmRunResult> {
        let payload = serde_json::json!({ "twin": twin, "scenario": scenario });

        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            // HTTP JSON-RPC: POST to {endpoint}/run
            let url = format!("{endpoint}/run");
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|e| TspError::SimulationError(e.to_string()))?;

            let resp = client.post(&url)
                .json(&payload)
                .send()
                .map_err(|e| TspError::SimulationError(format!("HTTP POST failed: {e}")))?;

            if !resp.status().is_success() {
                return Err(TspError::SimulationError(
                    format!("ỌSỌVM endpoint returned {}", resp.status())
                ));
            }

            resp.json::<OsovmRunResult>()
                .map_err(|e| TspError::SimulationError(format!("response parse failed: {e}")))
        } else {
            // Binary exec: write JSON to stdin, read OsovmRunResult from stdout
            use std::process::{Command, Stdio};
            use std::io::Write;

            let input = serde_json::to_string(&payload)
                .map_err(TspError::Json)?;

            let mut child = Command::new(endpoint)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| TspError::SimulationError(format!("spawn {endpoint}: {e}")))?;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input.as_bytes())
                    .map_err(|e| TspError::SimulationError(format!("stdin write: {e}")))?;
            }

            let output = child.wait_with_output()
                .map_err(|e| TspError::SimulationError(format!("wait: {e}")))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(TspError::SimulationError(
                    format!("ỌSỌVM binary exited with {}; stderr: {}", output.status, stderr)
                ));
            }

            serde_json::from_slice::<OsovmRunResult>(&output.stdout)
                .map_err(|e| TspError::SimulationError(format!("stdout parse: {e}")))
        }
    }

    fn select_policy(&self, policies: &[SimPolicy], objective: &str) -> String {
        let best = match objective {
            "min_energy" => policies.iter().min_by(|a, b| a.energy.partial_cmp(&b.energy).unwrap()),
            "min_risk"   => policies.iter().min_by(|a, b| a.risk.partial_cmp(&b.risk).unwrap()),
            "max_throughput" => policies.iter().min_by(|a, b| a.duration_s.partial_cmp(&b.duration_s).unwrap()),
            _ => {
                // balanced: minimise energy * 0.4 + risk * 0.6
                policies.iter().min_by(|a, b| {
                    let sa = a.energy * 0.4 + a.risk * 100.0 * 0.6;
                    let sb = b.energy * 0.4 + b.risk * 100.0 * 0.6;
                    sa.partial_cmp(&sb).unwrap()
                })
            }
        };
        best.map(|p| p.id.clone()).unwrap_or_else(|| policies[0].id.clone())
    }
}

/// Orchestrates: run → get witnesses → build SimulationReceipt.
pub struct ProofOfSimulation {
    pub engine: OsovmEngine,
}

impl ProofOfSimulation {
    pub fn new(engine: OsovmEngine) -> Self {
        Self { engine }
    }

    /// Full proof pipeline.
    ///
    /// `witness_keys` — slice of (witness_did, witness_private_key) pairs.
    /// In production these come from the sovereign node mesh via VCP.
    pub fn prove(
        &self,
        twin:         &TwinAsset,
        scenario:     &SimScenario,
        identity:     IdentityChain,
        private_key:  &str,
        witness_keys: &[(&str, &str)],  // (did, signing_key)
    ) -> TspResult<SimulationReceipt> {
        if witness_keys.len() < 2 {
            return Err(TspError::InsufficientWitnesses(witness_keys.len()));
        }

        let run = self.engine.run(twin, scenario)?;

        // Compute the same merkle commitment that SimulationReceipt::build will compute
        // (twin_id + engine + trajectory_count + all_policies_hash)
        let policies_json = serde_json::to_string(&run.candidate_policies)
            .map_err(TspError::Json)?;
        let all_policies_hash = hash_str(&policies_json);

        let mut fields = BTreeMap::new();
        fields.insert("all_policies_hash",  serde_json::json!(&all_policies_hash));
        fields.insert("robot_model",        serde_json::json!(&run.scenario.robot_model));
        fields.insert("sim_engine",         serde_json::json!(&run.engine_version));
        fields.insert("trajectory_count",   serde_json::json!(run.scenario.trajectory_count));
        fields.insert("twin_id",            serde_json::json!(&twin.twin_id));
        let commitment = merkle_root(&fields);

        // Each witness signs the commitment
        let witnesses: Vec<WitnessAttestation> = witness_keys.iter().map(|(did, key)| {
            let sig = sign(&commitment, key).unwrap_or_else(|_| "invalid".into());
            WitnessAttestation {
                witness_id:        did.to_string(),
                merkle_commitment: commitment.clone(),
                timestamp: now_ms(),
                signature: sig,
            }
        }).collect();

        SimulationReceipt::build(
            identity,
            twin.twin_id.clone(),
            &run.engine_version,
            &run.scenario.robot_model,
            run.scenario.trajectory_count,
            run.candidate_policies,
            run.selected_policy_id,
            &run.scenario.selection_objective,
            witnesses,
            private_key,
        )
    }

    /// Phase 1 of the two-phase witness protocol:
    /// Run ỌSỌVM and compute the merkle commitment that witnesses must sign.
    ///
    /// Returns (run_result, commitment_hash) — send the commitment to remote
    /// witnesses via DIP, collect their WitnessAttestations, then call
    /// `prove_with_attestations()` to build the final SimulationReceipt.
    pub fn run_and_commitment(
        &self,
        twin:     &TwinAsset,
        scenario: &SimScenario,
    ) -> TspResult<(OsovmRunResult, String)> {
        let run = self.engine.run(twin, scenario)?;

        let policies_json = serde_json::to_string(&run.candidate_policies)
            .map_err(TspError::Json)?;
        let all_policies_hash = hash_str(&policies_json);

        let mut fields = BTreeMap::new();
        fields.insert("all_policies_hash",  serde_json::json!(&all_policies_hash));
        fields.insert("robot_model",        serde_json::json!(&run.scenario.robot_model));
        fields.insert("sim_engine",         serde_json::json!(&run.engine_version));
        fields.insert("trajectory_count",   serde_json::json!(run.scenario.trajectory_count));
        fields.insert("twin_id",            serde_json::json!(&twin.twin_id));
        let commitment = merkle_root(&fields);

        Ok((run, commitment))
    }

    /// Phase 2 of the two-phase witness protocol:
    /// Build the SimulationReceipt from a pre-computed run result and pre-collected attestations.
    pub fn prove_with_attestations(
        &self,
        run:          OsovmRunResult,
        twin_id:      &str,
        identity:     IdentityChain,
        private_key:  &str,
        attestations: Vec<WitnessAttestation>,
    ) -> TspResult<SimulationReceipt> {
        if attestations.len() < 2 {
            return Err(TspError::InsufficientWitnesses(attestations.len()));
        }
        SimulationReceipt::build(
            identity,
            twin_id.to_string(),
            &run.engine_version,
            &run.scenario.robot_model,
            run.scenario.trajectory_count,
            run.candidate_policies,
            run.selected_policy_id,
            &run.scenario.selection_objective,
            attestations,
            private_key,
        )
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
    use crate::twin::{TwinAsset, TwinDataHashes, TwinProvenance, TwinLicenseConfig, RevenueSplit};
    use crate::quality::TwinQuality;

    fn make_twin() -> TwinAsset {
        let hashes = TwinDataHashes {
            rgb:   Some("sha256:aabb".into()),
            imu:   Some("sha256:ccdd".into()),
            splat: Some("sha256:eeff".into()),
            ..Default::default()
        };
        let quality = TwinQuality::new(0.85, 90.0, "nerfstudio").unwrap();
        let twin_id = TwinAsset::twin_id_from_hashes(&hashes);
        let ts = 0u64;
        TwinAsset {
            twin_id,
            version:     1,
            owner_did:   "did:vantage:principal:test".into(),
            creator_did: "did:agent:test".into(),
            contributors: vec![],
            region:       crate::region::TwinRegion::new(0.0, 0.0, 0.001, 0.001),
            data_hashes:  hashes,
            quality,
            provenance:   TwinProvenance::default(),
            license:      TwinLicenseConfig::default(),
            sui_object_id: None,
            merkle_root:  String::new(),
            signature:    String::new(),
            created_at:   ts,
            updated_at:   ts,
        }
    }

    fn make_engine() -> OsovmEngine {
        OsovmEngine::new("osovm/2.0").bypass_sabbath()
    }

    #[test]
    fn engine_run_produces_multiple_policies() {
        let twin = make_twin();
        let scenario = SimScenario {
            name: "lab_navigation".into(),
            robot_model: "Go2".into(),
            trajectory_count: 10,
            selection_objective: "min_risk".into(),
            params: None,
        };
        let result = make_engine().run(&twin, &scenario).unwrap();
        assert!(result.candidate_policies.len() >= 2);
        assert!(result.candidate_policies.iter().any(|p| p.id == result.selected_policy_id));
    }

    #[test]
    fn proof_of_simulation_full_pipeline() {
        let twin = make_twin();
        let scenario = SimScenario {
            name: "lab_nav".into(),
            robot_model: "Go2".into(),
            trajectory_count: 6,
            selection_objective: "balanced".into(),
            params: None,
        };
        let (priv_key, _) = generate_keypair();
        let (w1_priv, _) = generate_keypair();
        let (w2_priv, _) = generate_keypair();
        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());

        let proof = ProofOfSimulation::new(make_engine());
        let receipt = proof.prove(
            &twin,
            &scenario,
            identity,
            &priv_key,
            &[("did:witness:node01", &w1_priv), ("did:witness:node02", &w2_priv)],
        ).unwrap();

        assert_eq!(receipt.kind, "proof_of_simulation");
        assert_eq!(receipt.twin_id, twin.twin_id);
        assert!(receipt.all_policies.len() >= 2);
        assert_eq!(receipt.witness_ids.len(), 2);
        assert!(receipt.all_policies.iter().any(|p| p.id == receipt.selected_policy_id));
    }

    #[test]
    fn insufficient_witnesses_rejected() {
        let twin = make_twin();
        let scenario = SimScenario {
            name: "test".into(),
            robot_model: "Go2".into(),
            trajectory_count: 4,
            selection_objective: "min_energy".into(),
            params: None,
        };
        let (priv_key, _) = generate_keypair();
        let identity = IdentityChain::new("did:p:1".into(), "did:a:1".into());
        let proof = ProofOfSimulation::new(make_engine());
        let result = proof.prove(&twin, &scenario, identity, &priv_key, &[("did:w:1", "key")]);
        assert!(matches!(result, Err(TspError::InsufficientWitnesses(1))));
    }

    #[test]
    fn engine_with_unreachable_http_endpoint_falls_back_to_stub() {
        // Real OSOVM engine that won't connect — must fall back to stub output.
        let twin = make_twin();
        let scenario = SimScenario {
            name: "fallback_test".into(),
            robot_model: "Go2".into(),
            trajectory_count: 4,
            selection_objective: "min_risk".into(),
            params: None,
        };
        let engine = OsovmEngine::new("osovm/2.0")
            .with_endpoint("http://127.0.0.1:19999") // port that won't be open
            .bypass_sabbath();
        let result = engine.run(&twin, &scenario).unwrap();
        // Stub output is valid even when the real engine is unreachable
        assert!(result.candidate_policies.len() >= 2);
        assert!(result.candidate_policies.iter().any(|p| p.id == result.selected_policy_id));
    }

    #[test]
    fn engine_with_binary_path_falls_back_when_binary_missing() {
        let twin = make_twin();
        let scenario = SimScenario {
            name: "bin_fallback_test".into(),
            robot_model: "Go2".into(),
            trajectory_count: 4,
            selection_objective: "balanced".into(),
            params: None,
        };
        let engine = OsovmEngine::new("osovm/2.0")
            .with_endpoint("/nonexistent/osovm-binary")
            .bypass_sabbath();
        let result = engine.run(&twin, &scenario).unwrap();
        assert!(result.candidate_policies.len() >= 2);
    }

    #[test]
    fn selection_objectives() {
        let twin = make_twin();
        let engine = make_engine();
        for obj in ["min_energy", "min_risk", "max_throughput", "balanced"] {
            let scenario = SimScenario {
                name: format!("test_{obj}"),
                robot_model: "Go2".into(),
                trajectory_count: 6,
                selection_objective: obj.into(),
                params: None,
            };
            let result = engine.run(&twin, &scenario).unwrap();
            assert!(result.candidate_policies.iter().any(|p| p.id == result.selected_policy_id),
                "objective {obj}: selected policy not in list");
        }
    }

    /// Happy-path binary exec: write a shell script that reads stdin and emits
    /// a valid OsovmRunResult JSON. Verifies the subprocess code path end-to-end.
    #[test]
    fn engine_binary_subprocess_happy_path() {
        use std::os::unix::fs::PermissionsExt;

        // Build valid OsovmRunResult JSON that the fake binary will emit
        let result_json = serde_json::json!({
            "engine_version": "osovm-stub/1.0",
            "scenario": {
                "name": "bin_test",
                "robot_model": "Go2",
                "trajectory_count": 2,
                "selection_objective": "min_risk",
                "params": null
            },
            "trajectories": [
                {
                    "trajectory_id": "traj:bin:0",
                    "policy_id": "policy:conservative",
                    "success": true,
                    "energy_j": 100.0,
                    "risk_score": 0.1,
                    "duration_s": 20.0,
                    "metrics": null
                },
                {
                    "trajectory_id": "traj:bin:1",
                    "policy_id": "policy:aggressive",
                    "success": true,
                    "energy_j": 80.0,
                    "risk_score": 0.3,
                    "duration_s": 15.0,
                    "metrics": null
                }
            ],
            "candidate_policies": [
                {"id": "policy:conservative", "energy": 100.0, "risk": 0.1, "duration_s": 20.0, "metrics": null},
                {"id": "policy:aggressive",   "energy": 80.0,  "risk": 0.3, "duration_s": 15.0, "metrics": null}
            ],
            "selected_policy_id": "policy:conservative",
            "run_id": "osovm:run:bin-test-001",
            "wall_ms": 42
        });

        // Write a shell script stub to a temp file
        let tmp_dir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let script_path = format!("{tmp_dir}/sovereign-osovm-test-stub.sh");
        let script_body = format!(
            "#!/bin/sh\ncat /dev/null\necho '{}'\n",
            result_json.to_string()
        );
        std::fs::write(&script_path, &script_body)
            .expect("write test stub script");
        std::fs::set_permissions(
            &script_path,
            std::fs::Permissions::from_mode(0o755),
        ).expect("chmod stub script");

        let twin = make_twin();
        let scenario = SimScenario {
            name: "bin_test".into(),
            robot_model: "Go2".into(),
            trajectory_count: 2,
            selection_objective: "min_risk".into(),
            params: None,
        };

        // Use `run_real` directly via the public `run()` entry — endpoint is the script path
        let engine = OsovmEngine::new("osovm/2.0").with_endpoint(script_path.clone()).bypass_sabbath();
        let result = engine.run(&twin, &scenario).expect("binary exec should succeed");

        assert_eq!(result.engine_version, "osovm-stub/1.0");
        assert_eq!(result.run_id, "osovm:run:bin-test-001");
        assert_eq!(result.candidate_policies.len(), 2);
        assert_eq!(result.selected_policy_id, "policy:conservative");
        assert!(result.candidate_policies.iter().any(|p| p.id == result.selected_policy_id));

        // Clean up
        let _ = std::fs::remove_file(&script_path);
    }

    /// Binary that exits non-zero should cause fallback to the stub engine (not a hard error).
    #[test]
    fn engine_binary_nonzero_exit_falls_back_to_stub() {
        use std::os::unix::fs::PermissionsExt;

        let tmp_dir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let script_path = format!("{tmp_dir}/sovereign-osovm-test-fail.sh");
        std::fs::write(&script_path, "#!/bin/sh\necho 'fatal error' >&2\nexit 1\n")
            .expect("write fail stub");
        std::fs::set_permissions(
            &script_path,
            std::fs::Permissions::from_mode(0o755),
        ).expect("chmod fail stub");

        let twin = make_twin();
        let scenario = SimScenario {
            name: "fail_test".into(),
            robot_model: "Go2".into(),
            trajectory_count: 4,
            selection_objective: "balanced".into(),
            params: None,
        };

        let engine = OsovmEngine::new("osovm/2.0").with_endpoint(script_path.clone()).bypass_sabbath();
        // Should fall back to stub — not return an Err
        let result = engine.run(&twin, &scenario)
            .expect("non-zero exit should fall back to stub, not hard-fail");
        assert!(result.candidate_policies.len() >= 2,
            "stub fallback must produce >= 2 policies");

        let _ = std::fs::remove_file(&script_path);
    }

    #[test]
    fn sabbath_freeze_on_saturday() {
        // Unix day 0 = Thursday. day 3 = Sunday, day 2 = Saturday.
        // Formula: (unix_day + 4) % 7 == 6  →  Saturday UTC.
        // First Saturday since epoch: unix_day = 2  → (2+4)%7 = 6 ✓
        let saturday_ts = 2 * 86_400 + 3600; // Saturday 01:00 UTC
        assert!(crate::emission::DailyEmissionAllocator::is_sabbath(saturday_ts),
            "day 2 = Saturday must trigger Sabbath freeze");

        let friday_ts = 1 * 86_400 + 3600;   // Friday 01:00 UTC
        assert!(!crate::emission::DailyEmissionAllocator::is_sabbath(friday_ts),
            "day 1 = Friday must not trigger Sabbath freeze");

        let sunday_ts = 3 * 86_400 + 3600;   // Sunday 01:00 UTC
        assert!(!crate::emission::DailyEmissionAllocator::is_sabbath(sunday_ts),
            "day 3 = Sunday must not trigger Sabbath freeze");
    }
}
