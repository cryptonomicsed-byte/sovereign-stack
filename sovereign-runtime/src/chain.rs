/// The canonical sovereign execution chain:
///   Principal → Capability → Authorization → Execution → Evidence → ActionReceipt
///
/// This module re-exports the full flow as a single import surface.
/// Every consequential operation in the ecosystem should run through this chain.

pub use crate::principal::{Principal, HardwareAttestation};
pub use crate::capability::{Capability, CapabilityAction, CapabilityKernel, AuthorizationDecision, Constraint};
pub use crate::evidence::{Evidence, EvidenceBundle, EvidenceKind};
pub use crate::execution::{ExecutionContext, ExecutionEngine};
pub use crate::receipt::{ActionReceipt, ActionOutcome};

use serde_json::Value;
use sovereign_types::identity::Timestamp;

/// One-shot helper: run the full chain for a simple operation.
/// Calls `execute_fn` with the live context and collects the result.
pub async fn run_sovereign<F, Fut>(
    principal:  Principal,
    capability: Option<Capability>,
    action:     CapabilityAction,
    resource:   impl Into<String>,
    params:     Value,
    now:        Timestamp,
    execute_fn: F,
) -> ActionReceipt
where
    F:   FnOnce(ExecutionContext) -> Fut,
    Fut: std::future::Future<Output = (ExecutionContext, Result<Value, String>)>,
{
    match ExecutionEngine::begin(principal, capability, action, resource, params, now) {
        Err(denied_receipt) => denied_receipt,
        Ok(ctx) => {
            let (ctx, outcome) = execute_fn(ctx).await;
            match outcome {
                Ok(result) => ctx.complete(result, now),
                Err(e)     => ctx.fail(e, now),
            }
        }
    }
}
