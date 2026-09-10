/// VCP body interface — tier-gated physical embodiment.
///
/// A BodyCapability declares what trust tier and risk class an operation
/// requires.  Tier gates are checked before a VcpCapabilityGrant is issued.
use serde::{Deserialize, Serialize};
use sovereign_types::{TrustTier, SimulationProof, Timestamp};

// ── Capability risk classes ───────────────────────────────────────────────────

/// Risk class of a body operation — C0 (safe read) to C5 (irreversible/critical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityClass {
    C0, // read-only sensor
    C1, // safe low-power actuator
    C2, // moderate actuation (MIDI, GPIO write, motor start)
    C3, // significant motion / environmental effect
    C4, // high-risk motion / physical consequence
    C5, // irreversible / safety-critical
}

/// A named body operation with its tier and risk requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyCapability {
    pub id:           String,
    pub description:  String,
    pub risk_class:   CapabilityClass,
    /// Minimum trust tier the agent must hold to request this capability.
    pub min_tier:     TrustTier,
    /// If true, a human operator must be connected during execution.
    pub requires_human_loop: bool,
    /// Optional: agent must have proven sim competence in this domain.
    pub requires_sim_proof:  bool,
}

impl BodyCapability {
    pub fn check_tier(&self, agent_tier: TrustTier) -> Result<(), String> {
        if agent_tier < self.min_tier {
            Err(format!(
                "capability '{}' requires {}, agent is {}",
                self.id, self.min_tier, agent_tier
            ))
        } else {
            Ok(())
        }
    }
}

// ── Body session ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodySessionMode {
    /// Agent proposes; human approves each action.
    HumanSupervised,
    /// Agent operates autonomously within approved capability envelope.
    Autonomous,
    /// Replay of a verified simulation policy.
    PolicyReplay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodySession {
    pub session_id:      String,
    pub agent_id:        String,
    pub agent_tier:      TrustTier,
    pub body_id:         String,
    pub mode:            BodySessionMode,
    /// Optional: the simulation proof authorising this embodiment.
    pub sim_proof_id:    Option<String>,
    pub capabilities:    Vec<String>,
    pub started_at:      Timestamp,
}

impl BodySession {
    pub fn new(
        agent_id: impl Into<String>,
        agent_tier: TrustTier,
        body_id: impl Into<String>,
        mode: BodySessionMode,
        capabilities: Vec<String>,
        sim_proof_id: Option<String>,
    ) -> Result<Self, String> {
        let mode_ok = match &mode {
            BodySessionMode::HumanSupervised => agent_tier.supervised_embodiment_eligible(),
            BodySessionMode::Autonomous | BodySessionMode::PolicyReplay =>
                agent_tier.autonomous_embodiment_eligible(),
        };
        if !mode_ok {
            return Err(format!(
                "agent tier {} not eligible for {:?} body session",
                agent_tier, mode
            ));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Ok(Self {
            session_id:   format!("body:{}", uuid::Uuid::new_v4()),
            agent_id:     agent_id.into(),
            agent_tier,
            body_id:      body_id.into(),
            mode,
            sim_proof_id,
            capabilities,
            started_at:   now,
        })
    }
}

// ── Flight session (drone/aerial body) ───────────────────────────────────────

/// Telemetry snapshot from a physical flight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightTelemetry {
    pub timestamp_ms:   u64,
    pub position:       [f64; 3],   // x, y, z metres
    pub orientation:    [f64; 4],   // quaternion
    pub altitude_m:     f64,
    pub battery_pct:    f64,
    pub velocity_ms:    f64,
    pub obstacle_dist_m: Option<f64>,
}

/// Receipt produced at the end of a physical flight session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightReceipt {
    pub receipt_id:          String,
    pub session_id:          String,
    pub agent_id:            String,
    pub body_id:             String,
    pub sim_proof_id:        Option<String>,
    pub trajectory_hash:     String,
    pub telemetry_count:     u32,
    pub duration_ms:         u64,
    pub max_altitude_m:      f64,
    pub battery_consumed_pct: f64,
    pub mission_success:     bool,
    pub witness_ids:         Vec<String>,
    pub timestamp:           Timestamp,
    pub signature:           String,
}

// ── StampFly v1.1 device profile ──────────────────────────────────────────────

/// Capability catalogue for the M5Stack StampFly v1.1.
pub fn stampfly_capabilities() -> Vec<BodyCapability> {
    vec![
        BodyCapability {
            id:          "sensor.imu".into(),
            description: "Read BMI270 IMU (orientation, accel, gyro)".into(),
            risk_class:  CapabilityClass::C0,
            min_tier:    TrustTier::T0,
            requires_human_loop: false,
            requires_sim_proof:  false,
        },
        BodyCapability {
            id:          "sensor.altitude".into(),
            description: "Read BMP280 barometric altitude".into(),
            risk_class:  CapabilityClass::C0,
            min_tier:    TrustTier::T0,
            requires_human_loop: false,
            requires_sim_proof:  false,
        },
        BodyCapability {
            id:          "sensor.distance".into(),
            description: "Read VL53L3 front/bottom distance sensors".into(),
            risk_class:  CapabilityClass::C0,
            min_tier:    TrustTier::T0,
            requires_human_loop: false,
            requires_sim_proof:  false,
        },
        BodyCapability {
            id:          "sensor.battery".into(),
            description: "Read INA3221 battery voltage/current".into(),
            risk_class:  CapabilityClass::C0,
            min_tier:    TrustTier::T0,
            requires_human_loop: false,
            requires_sim_proof:  false,
        },
        BodyCapability {
            id:          "flight.arm".into(),
            description: "Arm motors (hover ready)".into(),
            risk_class:  CapabilityClass::C2,
            min_tier:    TrustTier::T2,
            requires_human_loop: true,
            requires_sim_proof:  false,
        },
        BodyCapability {
            id:          "flight.takeoff".into(),
            description: "Automated takeoff to hover altitude".into(),
            risk_class:  CapabilityClass::C3,
            min_tier:    TrustTier::T3,
            requires_human_loop: true,
            requires_sim_proof:  true,
        },
        BodyCapability {
            id:          "flight.navigate".into(),
            description: "Waypoint navigation mission".into(),
            risk_class:  CapabilityClass::C4,
            min_tier:    TrustTier::T4,
            requires_human_loop: true,
            requires_sim_proof:  true,
        },
        BodyCapability {
            id:          "flight.autonomous_mission".into(),
            description: "Fully autonomous mission without human in loop".into(),
            risk_class:  CapabilityClass::C4,
            min_tier:    TrustTier::T5,
            requires_human_loop: false,
            requires_sim_proof:  true,
        },
        BodyCapability {
            id:          "flight.land".into(),
            description: "Automated landing sequence".into(),
            risk_class:  CapabilityClass::C3,
            min_tier:    TrustTier::T3,
            requires_human_loop: false,
            requires_sim_proof:  false,
        },
        BodyCapability {
            id:          "flight.emergency_stop".into(),
            description: "Immediate motor kill / emergency stop".into(),
            risk_class:  CapabilityClass::C1,
            min_tier:    TrustTier::T0,
            requires_human_loop: false,
            requires_sim_proof:  false,
        },
    ]
}

/// Policy check: given an agent tier and optional sim proof, which StampFly
/// capabilities are currently grantable?
pub fn grantable_stampfly_caps(
    agent_tier: TrustTier,
    has_sim_proof: bool,
) -> Vec<String> {
    stampfly_capabilities()
        .into_iter()
        .filter(|cap| {
            cap.check_tier(agent_tier).is_ok()
                && (!cap.requires_sim_proof || has_sim_proof)
        })
        .map(|cap| cap.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t0_gets_sensors_only() {
        let caps = grantable_stampfly_caps(TrustTier::T0, false);
        assert!(caps.contains(&"sensor.imu".to_string()));
        assert!(!caps.contains(&"flight.arm".to_string()));
        assert!(!caps.contains(&"flight.takeoff".to_string()));
    }

    #[test]
    fn t2_gets_arm_but_not_takeoff_without_sim() {
        let caps = grantable_stampfly_caps(TrustTier::T2, false);
        assert!(caps.contains(&"flight.arm".to_string()));
        assert!(!caps.contains(&"flight.takeoff".to_string()));
    }

    #[test]
    fn t3_with_sim_proof_gets_takeoff() {
        let caps = grantable_stampfly_caps(TrustTier::T3, true);
        assert!(caps.contains(&"flight.takeoff".to_string()));
        assert!(!caps.contains(&"flight.autonomous_mission".to_string()));
    }

    #[test]
    fn t5_with_sim_proof_gets_autonomous_mission() {
        let caps = grantable_stampfly_caps(TrustTier::T5, true);
        assert!(caps.contains(&"flight.autonomous_mission".to_string()));
    }

    #[test]
    fn body_session_t4_supervised_ok() {
        let s = BodySession::new(
            "agent:1", TrustTier::T4, "stampfly:1",
            BodySessionMode::HumanSupervised,
            vec!["flight.navigate".into()],
            Some("proof:1".into()),
        );
        assert!(s.is_ok());
    }

    #[test]
    fn body_session_t3_autonomous_rejected() {
        let s = BodySession::new(
            "agent:1", TrustTier::T3, "stampfly:1",
            BodySessionMode::Autonomous,
            vec!["flight.navigate".into()],
            None,
        );
        assert!(s.is_err());
    }

    #[test]
    fn body_session_t4_autonomous_rejected() {
        let s = BodySession::new(
            "agent:1", TrustTier::T4, "stampfly:1",
            BodySessionMode::Autonomous,
            vec![],
            None,
        );
        assert!(s.is_err(), "autonomous requires T5");
    }

    #[test]
    fn emergency_stop_always_grantable() {
        let caps = grantable_stampfly_caps(TrustTier::T0, false);
        assert!(caps.contains(&"flight.emergency_stop".to_string()));
    }
}
