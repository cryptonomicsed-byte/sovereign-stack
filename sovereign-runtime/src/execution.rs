use serde_json::Value;
use sovereign_types::identity::Timestamp;
use crate::principal::Principal;
use crate::capability::{Capability, CapabilityAction, CapabilityKernel};
use crate::evidence::EvidenceBundle;
use crate::receipt::{ActionReceipt, ActionOutcome};

/// Live execution context — lives for one consequential operation.
/// Created by `ExecutionEngine::begin`, resolved by `ExecutionContext::complete`.
#[derive(Debug)]
pub struct ExecutionContext {
    pub principal:     Principal,
    pub capability:    Option<Capability>,
    pub action:        CapabilityAction,
    pub resource:      String,
    pub params:        Value,
    pub evidence:      EvidenceBundle,
    pub started_at:    Timestamp,
}

impl ExecutionContext {
    pub fn add_evidence(&mut self, e: crate::evidence::Evidence) {
        self.evidence.push(e);
    }

    pub fn complete(self, result: Value, now: Timestamp) -> ActionReceipt {
        let cap_id = self.capability.as_ref().map(|c| c.capability_id.clone());
        ActionReceipt::new(
            &self.principal,
            self.action,
            self.resource,
            cap_id,
            self.params,
            result,
            ActionOutcome::Success,
            &self.evidence,
            self.started_at,
            now,
        )
    }

    pub fn fail(self, reason: impl Into<String>, now: Timestamp) -> ActionReceipt {
        let cap_id = self.capability.as_ref().map(|c| c.capability_id.clone());
        let mut r = ActionReceipt::new(
            &self.principal,
            self.action,
            self.resource,
            cap_id,
            self.params,
            Value::Null,
            ActionOutcome::Failure,
            &self.evidence,
            self.started_at,
            now,
        );
        r.error = Some(reason.into());
        r
    }
}

/// The ExecutionEngine — entry point for beginning a sovereign execution.
pub struct ExecutionEngine;

impl ExecutionEngine {
    /// Begin an execution: validate principal, evaluate capability, return context.
    pub fn begin(
        principal:  Principal,
        capability: Option<Capability>,
        action:     CapabilityAction,
        resource:   impl Into<String>,
        params:     Value,
        now:        Timestamp,
    ) -> Result<ExecutionContext, ActionReceipt> {
        // Validate principal
        if let Err(e) = principal.validate() {
            let receipt = ActionReceipt::denied(&principal, action.clone(), resource.into(), e.to_string(), now);
            return Err(receipt);
        }

        let resource = resource.into();

        // Evaluate capability if provided
        if let Some(ref cap) = capability {
            let decision = CapabilityKernel::evaluate(&principal, cap, &action, &resource, now);
            if !decision.granted {
                let reason = decision.denial_reason.unwrap_or_else(|| "denied".into());
                let receipt = ActionReceipt::denied(&principal, action, resource, reason, now);
                return Err(receipt);
            }
        }

        Ok(ExecutionContext {
            principal,
            capability,
            action,
            resource,
            params,
            evidence:   EvidenceBundle::new(),
            started_at: now,
        })
    }
}
