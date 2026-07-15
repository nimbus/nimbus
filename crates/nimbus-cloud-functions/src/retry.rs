use std::thread;

use nimbus_bridge::mutation_retry::{MutationOccConflictDecision, MutationOccRetryPolicy};
use nimbus_core::{Result, TenantId};
use nimbus_engine::Engine;

pub(crate) fn execute_mutation_with_occ_retries<T, F>(
    engine: &Engine,
    tenant_id: &TenantId,
    mut invoke_and_commit: F,
) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let policy = MutationOccRetryPolicy::from_env();
    let mut attempt = 1;
    loop {
        match invoke_and_commit() {
            Ok(value) => return Ok(value),
            Err(error) => match policy.classify(&error, attempt) {
                MutationOccConflictDecision::NotRetryable => return Err(error),
                MutationOccConflictDecision::Exhausted => {
                    if let Err(metrics_error) = engine.record_mutation_conflict_exhausted(tenant_id)
                    {
                        tracing::warn!(
                            tenant = %tenant_id,
                            error = %metrics_error,
                            "failed to record exhausted mutation conflict"
                        );
                    }
                    return Err(error.with_conflict_attempts(attempt));
                }
                MutationOccConflictDecision::Retry {
                    conflicting_sequence,
                    backoff,
                } => {
                    // Retrying is safe only while guest mutations are
                    // deterministic and side-effect-free between attempts.
                    // Each closure call must build a fresh execution unit;
                    // staged writes from this failed attempt are single-use.
                    if let Some(sequence) = conflicting_sequence {
                        engine.wait_for_applied_sequence_blocking(tenant_id, sequence)?;
                    }
                    thread::sleep(backoff);
                    if let Err(metrics_error) = engine.record_mutation_conflict_retry(tenant_id) {
                        tracing::warn!(
                            tenant = %tenant_id,
                            error = %metrics_error,
                            "failed to record mutation conflict retry"
                        );
                    }
                    attempt += 1;
                }
            },
        }
    }
}
