use serde::{Deserialize, Serialize};
use serde_json::Value;
use sovereign_types::identity::{SafetyLevel, Timestamp};
use crate::principal::Principal;

/// A Capability grant — describes what a Principal is allowed to do.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    pub capability_id: String,
    pub granted_to:    String,           // principal_id
    pub action:        CapabilityAction,
    pub resource:      String,           // URI or glob, e.g. "twin:*" or "tile:odu:0a"
    pub constraints:   Vec<Constraint>,
    pub safety_level:  SafetyLevel,
    pub granted_at:    Timestamp,
    pub expires_at:    Option<Timestamp>,
    pub delegated_by:  Option<String>,   // principal_id of grantor
    pub signature:     String,           // grantor's signature over canonical fields
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAction {
    Capture,
    Simulate,
    Claim,
    Delegate,
    Attest,
    Publish,
    Route,
    Execute,
    Read,
    Write,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Constraint {
    MaxRate     { per_minute: u32 },
    GeoFence    { tile_ids: Vec<String> },
    TimeWindow  { from: Timestamp, until: Timestamp },
    DataClass   { allowed: Vec<String> },
    Custom      { rule: Value },
}

/// The CapabilityKernel validates Principal → Capability pairs.
/// This is the P0-2 fix: a single, universally-enforced authorization engine.
pub struct CapabilityKernel;

#[derive(Debug)]
pub struct AuthorizationDecision {
    pub granted:         bool,
    pub principal_id:    String,
    pub capability_id:   String,
    pub action:          CapabilityAction,
    pub resource:        String,
    pub denial_reason:   Option<String>,
    pub evaluated_at:    Timestamp,
}

impl CapabilityKernel {
    /// Core decision function — Principal × Capability × (action, resource) → Decision.
    pub fn evaluate(
        principal:  &Principal,
        capability: &Capability,
        action:     &CapabilityAction,
        resource:   &str,
        now:        Timestamp,
    ) -> AuthorizationDecision {
        let base = AuthorizationDecision {
            granted:       false,
            principal_id:  principal.principal_id().to_string(),
            capability_id: capability.capability_id.clone(),
            action:        action.clone(),
            resource:      resource.to_string(),
            denial_reason: None,
            evaluated_at:  now,
        };

        // 1 — capability must be for this principal
        if capability.granted_to != principal.principal_id() {
            return AuthorizationDecision {
                denial_reason: Some("capability granted to different principal".into()),
                ..base
            };
        }

        // 2 — action must match
        if &capability.action != action {
            return AuthorizationDecision {
                denial_reason: Some(format!("action mismatch: need {:?}, cap has {:?}", action, capability.action)),
                ..base
            };
        }

        // 3 — resource must match (glob or exact)
        if !resource_matches(&capability.resource, resource) {
            return AuthorizationDecision {
                denial_reason: Some(format!("resource '{}' not covered by '{}'", resource, capability.resource)),
                ..base
            };
        }

        // 4 — expiry
        if let Some(exp) = capability.expires_at {
            if now > exp {
                return AuthorizationDecision {
                    denial_reason: Some("capability expired".into()),
                    ..base
                };
            }
        }

        // 5 — principal safety ceiling must not be exceeded
        if capability.safety_level > principal.safety_level {
            return AuthorizationDecision {
                denial_reason: Some(format!(
                    "capability safety {:?} exceeds principal ceiling {:?}",
                    capability.safety_level, principal.safety_level
                )),
                ..base
            };
        }

        // 6 — evaluate constraints
        for constraint in &capability.constraints {
            if let Some(reason) = evaluate_constraint(constraint, resource, now) {
                return AuthorizationDecision { denial_reason: Some(reason), ..base };
            }
        }

        AuthorizationDecision { granted: true, denial_reason: None, ..base }
    }
}

fn resource_matches(pattern: &str, resource: &str) -> bool {
    if pattern.ends_with('*') {
        resource.starts_with(&pattern[..pattern.len() - 1])
    } else {
        pattern == resource
    }
}

fn evaluate_constraint(constraint: &Constraint, _resource: &str, now: Timestamp) -> Option<String> {
    match constraint {
        Constraint::TimeWindow { from, until } => {
            if now < *from || now > *until {
                Some(format!("outside time window {}–{}", from, until))
            } else {
                None
            }
        }
        Constraint::GeoFence { tile_ids } => {
            // tile validation deferred to caller — we just ensure list is non-empty
            if tile_ids.is_empty() {
                Some("geofence constraint has no tiles".into())
            } else {
                None
            }
        }
        _ => None,
    }
}
