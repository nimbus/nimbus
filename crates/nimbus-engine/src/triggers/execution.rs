use std::sync::Arc;

use nimbus_core::{TenantId, TriggerInvocationRecord};

/// Engine-owned disposition for one durable trigger invocation attempt.
///
/// Concrete runtimes classify failures as retryable or terminal, while the
/// engine keeps ownership of retry timing, durable replay, and max-attempt
/// policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerInvocationExecution {
    Completed,
    RetryableFailure { error: String },
    TerminalFailure { error: String },
}

impl TriggerInvocationExecution {
    pub fn completed() -> Self {
        Self::Completed
    }

    pub fn retryable(error: impl Into<String>) -> Self {
        Self::RetryableFailure {
            error: error.into(),
        }
    }

    pub fn terminal(error: impl Into<String>) -> Self {
        Self::TerminalFailure {
            error: error.into(),
        }
    }
}

/// Protocol-neutral execution seam for durable trigger invocations.
///
/// The engine owns durable claiming and lifecycle transitions for trigger
/// invocations, but the concrete runtime artifact lookup and JavaScript
/// execution surface live above the engine boundary. This trait lets the
/// engine drive at-least-once delivery without depending on any one server or
/// adapter runtime implementation.
pub trait TriggerInvocationExecutor: Send + Sync + 'static {
    fn execute_invocation(
        &self,
        tenant_id: &TenantId,
        record: &TriggerInvocationRecord,
    ) -> TriggerInvocationExecution;
}

pub(crate) type SharedTriggerInvocationExecutor = Arc<dyn TriggerInvocationExecutor>;
