//! Semantic lifecycle-lock barrier for deterministic runner tests.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::error::{Result, SandboxError};

use super::*;

#[derive(Clone)]
pub(in crate::backends::container::runtime) struct RunnerLifecycleLockTestProbe {
    shared: Arc<(Mutex<bool>, Condvar)>,
    timeout: Duration,
}

impl RunnerLifecycleLockTestProbe {
    pub(in crate::backends::container::runtime) fn new(timeout: Duration) -> Self {
        Self {
            shared: Arc::new((Mutex::new(false), Condvar::new())),
            timeout,
        }
    }

    pub(in crate::backends::container::runtime) fn record_contended(&self) -> Result<()> {
        let (lock, changed) = &*self.shared;
        let mut contended = lock.lock().map_err(|_| SandboxError::OperationFailed {
            message: "runner lifecycle-lock test probe was poisoned".to_owned(),
        })?;
        *contended = true;
        changed.notify_all();
        Ok(())
    }

    pub(in crate::backends::container::runtime) fn wait_until_contended(&self) -> bool {
        let (lock, changed) = &*self.shared;
        let contended = lock
            .lock()
            .expect("runner lifecycle-lock test probe should not be poisoned");
        let (contended, _) = changed
            .wait_timeout_while(contended, self.timeout, |contended| !*contended)
            .expect("runner lifecycle-lock test probe wait should not be poisoned");
        *contended
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::backends::container::runtime) enum RunnerDecisionStageFault {
    AfterCreate,
    AfterWrite,
}

pub(in crate::backends::container::runtime) fn claim_runner_execution_for_test(
    manifest: &ContainerSandboxManifest,
) -> Result<PathBuf> {
    let _handoff = lock_runner_handoff(manifest)?;
    validate_durable_prepared_manifest(manifest)?;
    claim_runner_handoff_decision(
        manifest,
        RunnerHandoffDecision::Execute,
        ContainerLifecycleCoordinator::PreparedServiceRunner,
        "container runner",
    )
}

pub(in crate::backends::container::runtime) fn claim_runner_execution_with_stage_fault_for_test(
    manifest: &ContainerSandboxManifest,
    fault: RunnerDecisionStageFault,
) -> Result<PathBuf> {
    let _handoff = lock_runner_handoff(manifest)?;
    validate_durable_prepared_manifest(manifest)?;
    claim_runner_handoff_decision_with_fault(
        manifest,
        RunnerHandoffDecision::Execute,
        ContainerLifecycleCoordinator::PreparedServiceRunner,
        "container runner",
        Some(fault),
    )
}

pub(in crate::backends::container::runtime) fn persist_claimed_runner_execution_for_test(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    decision_path: &Path,
) -> Result<()> {
    let expected_path = runner_handoff_decision_path(manifest);
    if decision_path != expected_path {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container runner execution claim path {} does not match prepared authority {}",
                decision_path.display(),
                expected_path.display()
            ),
        });
    }
    persist_runner_execution_ownership(backend, manifest).map(drop)
}
