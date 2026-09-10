use serde::{Deserialize, Serialize};
use sovereign_types::identity::SafetyLevel;

/// A safety verdict — the output of SafetySupervisor evaluation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyVerdict {
    /// Command may proceed unconditionally.
    Clear,
    /// Command may proceed but must be logged and monitored.
    Conditional { reason: String },
    /// Command is blocked until conditions are met.
    Blocked { reason: String },
    /// Command is hard-blocked — no override possible at this tier.
    Halt { reason: String },
}

impl SafetyVerdict {
    pub fn is_clear(&self)   -> bool { matches!(self, Self::Clear | Self::Conditional { .. }) }
    pub fn is_blocked(&self) -> bool { matches!(self, Self::Blocked { .. } | Self::Halt { .. }) }
}

/// A command submitted to the safety hierarchy for evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCommand {
    pub command_type:  SafetyCommandType,
    pub target:        String,
    pub magnitude:     f32,    // 0.0–1.0: intensity of action (0 = idle, 1 = max)
    pub requester_did: String,
    pub safety_level:  SafetyLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyCommandType {
    Move,
    Grasp,
    Release,
    Rotate,
    Thrust,
    Actuate,
    Capture,        // camera/sensor — lower risk
    Broadcast,      // network — lowest risk
    Custom(String),
}

/// SafetySupervisor — enforces the safety hierarchy between Omo-Koda and RTOS/actuators.
///
/// Stack:
///   Agent intent (Omo-Koda) → VCP capability gate → SafetySupervisor → RTOS → actuator
///
/// Omo-Koda says "move toward target."
/// SafetySupervisor decides whether that command can physically execute.
pub struct SafetySupervisor {
    pub tier: SafetyTier,
    pub emergency_stop_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyTier {
    /// Development — most commands allowed, all logged
    Development,
    /// Production — standard physical safety constraints
    Production,
    /// Critical — robot operating near humans; strict magnitude limits
    CriticalZone,
    /// Emergency — only halt commands accepted
    EmergencyStop,
}

impl SafetySupervisor {
    pub fn new(tier: SafetyTier) -> Self {
        Self { tier, emergency_stop_active: false }
    }

    pub fn emergency_stop(&mut self) {
        self.emergency_stop_active = true;
        self.tier = SafetyTier::EmergencyStop;
    }

    pub fn clear_emergency(&mut self) {
        self.emergency_stop_active = false;
        // Do NOT auto-restore tier — operator must manually reconfigure
    }

    pub fn evaluate(&self, cmd: &SafetyCommand) -> SafetyVerdict {
        if self.emergency_stop_active {
            return SafetyVerdict::Halt {
                reason: "emergency stop active — no commands accepted".into(),
            };
        }

        match self.tier {
            SafetyTier::Development => {
                SafetyVerdict::Conditional {
                    reason: "development tier — logged".into(),
                }
            }

            SafetyTier::Production => {
                self.check_production(cmd)
            }

            SafetyTier::CriticalZone => {
                // In critical zone, only low-magnitude commands allowed
                if cmd.magnitude > 0.25 {
                    return SafetyVerdict::Blocked {
                        reason: format!(
                            "magnitude {:.2} exceeds critical-zone limit 0.25", cmd.magnitude
                        ),
                    };
                }
                self.check_production(cmd)
            }

            SafetyTier::EmergencyStop => {
                SafetyVerdict::Halt {
                    reason: "emergency stop tier — all commands halted".into(),
                }
            }
        }
    }

    fn check_production(&self, cmd: &SafetyCommand) -> SafetyVerdict {
        // Safety level gate
        if cmd.safety_level > sovereign_types::identity::SafetyLevel::Elevated {
            return SafetyVerdict::Blocked {
                reason: format!("safety level {:?} exceeds production ceiling", cmd.safety_level),
            };
        }

        // Magnitude gate for physical actuation
        let is_physical = matches!(
            cmd.command_type,
            SafetyCommandType::Move
            | SafetyCommandType::Grasp
            | SafetyCommandType::Release
            | SafetyCommandType::Rotate
            | SafetyCommandType::Thrust
            | SafetyCommandType::Actuate
        );

        if is_physical && cmd.magnitude > 0.85 {
            return SafetyVerdict::Blocked {
                reason: format!("magnitude {:.2} exceeds production physical limit 0.85", cmd.magnitude),
            };
        }

        SafetyVerdict::Clear
    }
}
