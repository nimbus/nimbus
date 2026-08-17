//! Bounded fresh-process recovery for durable workload lifecycle records.
//!
//! This owner enumerates durable work once and routes each exact record to the
//! existing provision, restart, teardown, or successor owner. It owns no
//! provider capability and performs no provider effect directly.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_workloads::{
    DesiredWorkloadState, MAX_WORKLOAD_SAGA_PAGE_SIZE, WorkloadSagaKey, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStoreError,
};
use thiserror::Error;

use crate::workload_provisioner::{
    WorkloadProvisionCancellation, WorkloadProvisionCompensationState, WorkloadProvisioner,
};

use super::restart_runtime::WorkloadRestartRuntime;
use super::{
    WorkloadProvisionRunDisposition, WorkloadSagaAction, WorkloadSagaCoordinator,
    WorkloadSagaDecision, WorkloadTeardownCancellationToken, WorkloadTeardownRunDisposition,
    WorkloadTeardownRuntime,
};

/// Hard bound for one serving-readiness attempt. A larger durable inventory
/// fails closed and can be retried after an operator reduces or partitions it.
const MAX_STARTUP_RECOVERY_PAGES: usize = 64;

type WorkloadStartupOwnerFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupProvisionResult {
    record: WorkloadSagaRecord,
    disposition: WorkloadProvisionRunDisposition,
    compensation: WorkloadProvisionCompensationState,
}

trait WorkloadStartupProvisionOwner: Send + Sync {
    fn resume(
        &self,
        key: WorkloadSagaKey,
        owner_reopened_publication: bool,
    ) -> WorkloadStartupOwnerFuture<'_, StartupProvisionResult>;
}

struct WorkloadStartupProvisionAdapter {
    owner: Arc<WorkloadProvisioner>,
}

impl WorkloadStartupProvisionOwner for WorkloadStartupProvisionAdapter {
    fn resume(
        &self,
        key: WorkloadSagaKey,
        owner_reopened_publication: bool,
    ) -> WorkloadStartupOwnerFuture<'_, StartupProvisionResult> {
        Box::pin(async move {
            let cancellation = WorkloadProvisionCancellation::default();
            let outcome = if owner_reopened_publication {
                self.owner
                    .resume_owner_reopened_publication(key, &cancellation)
                    .await
            } else {
                self.owner.resume(key, &cancellation).await
            }
            .map_err(|error| error.to_string())?;
            Ok(StartupProvisionResult {
                record: outcome.record().clone(),
                disposition: outcome.disposition(),
                compensation: outcome.compensation(),
            })
        })
    }
}

trait WorkloadStartupRestartOwner: Send + Sync {
    fn activate_watch(&self) -> Result<(), String>;

    fn recover(&self, record: WorkloadSagaRecord) -> WorkloadStartupOwnerFuture<'_, ()>;
}

struct WorkloadStartupRestartAdapter {
    owner: Arc<WorkloadRestartRuntime>,
}

impl WorkloadStartupRestartOwner for WorkloadStartupRestartAdapter {
    fn activate_watch(&self) -> Result<(), String> {
        self.owner.activate_watch()
    }

    fn recover(&self, record: WorkloadSagaRecord) -> WorkloadStartupOwnerFuture<'_, ()> {
        Box::pin(async move { self.owner.recover_active(record).await })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupTeardownResult {
    record: WorkloadSagaRecord,
    disposition: WorkloadTeardownRunDisposition,
}

trait WorkloadStartupTeardownOwner: Send + Sync {
    fn submit(&self, key: WorkloadSagaKey)
    -> WorkloadStartupOwnerFuture<'_, StartupTeardownResult>;
}

struct WorkloadStartupTeardownAdapter {
    owner: Arc<WorkloadTeardownRuntime>,
}

impl WorkloadStartupTeardownOwner for WorkloadStartupTeardownAdapter {
    fn submit(
        &self,
        key: WorkloadSagaKey,
    ) -> WorkloadStartupOwnerFuture<'_, StartupTeardownResult> {
        Box::pin(async move {
            let run = self
                .owner
                .submit(key, &WorkloadTeardownCancellationToken::default())
                .await
                .map_err(|error| error.to_string())?;
            Ok(StartupTeardownResult {
                record: run.record().clone(),
                disposition: run.disposition(),
            })
        })
    }
}

/// Typed result of routing one exact durable workload record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadStartupDisposition {
    ProvisionObserved,
    ProvisionWaiting,
    ProvisionCompensated,
    ProvisionCompensationWaiting,
    RestartSettled,
    RestartWaiting,
    TeardownCompleted,
    TeardownWaiting,
    SuccessorRunningObserved,
    SuccessorRunningWaiting,
    SuccessorRunningCompensated,
    SuccessorRunningCompensationWaiting,
    SuccessorSettlementTeardownCompleted,
    SuccessorSettlementTeardownWaiting,
    SuccessorStopped,
    CleanupRetained,
    Quiescent,
}

impl WorkloadStartupDisposition {
    const fn is_waiting(self) -> bool {
        matches!(
            self,
            Self::ProvisionWaiting
                | Self::ProvisionCompensationWaiting
                | Self::RestartWaiting
                | Self::TeardownWaiting
                | Self::SuccessorRunningWaiting
                | Self::SuccessorRunningCompensationWaiting
                | Self::SuccessorSettlementTeardownWaiting
        )
    }

    const fn retains_cleanup(self) -> bool {
        matches!(self, Self::CleanupRetained)
    }
}

/// Exact durable truth after one startup route returns control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadStartupRecoveryOutcome {
    record: WorkloadSagaRecord,
    disposition: WorkloadStartupDisposition,
}

impl WorkloadStartupRecoveryOutcome {
    fn new(record: WorkloadSagaRecord, disposition: WorkloadStartupDisposition) -> Self {
        Self {
            record,
            disposition,
        }
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        self.record.key()
    }

    pub fn record(&self) -> &WorkloadSagaRecord {
        &self.record
    }

    pub const fn disposition(&self) -> WorkloadStartupDisposition {
        self.disposition
    }
}

/// Aggregate result of one bounded all-phase startup pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadStartupRecoveryReport {
    tenant_retirements: usize,
    pages: usize,
    outcomes: Vec<WorkloadStartupRecoveryOutcome>,
}

impl WorkloadStartupRecoveryReport {
    pub(crate) fn empty() -> Self {
        Self {
            tenant_retirements: 0,
            pages: 0,
            outcomes: Vec::new(),
        }
    }

    pub(crate) fn with_tenant_retirements(mut self, count: usize) -> Self {
        self.tenant_retirements = count;
        self
    }

    pub const fn tenant_retirements(&self) -> usize {
        self.tenant_retirements
    }

    pub const fn pages(&self) -> usize {
        self.pages
    }

    pub fn outcomes(&self) -> &[WorkloadStartupRecoveryOutcome] {
        &self.outcomes
    }

    pub fn waiting_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.disposition().is_waiting())
            .count()
    }

    pub fn cleanup_retained_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.disposition().retains_cleanup())
            .count()
    }
}

/// Failure before startup can establish complete durable lifecycle truth.
#[derive(Debug, Error)]
pub enum WorkloadStartupRecoveryError {
    #[error("workload startup durable store failed: {0}")]
    Store(#[from] WorkloadSagaStoreError),
    #[error("workload startup exceeded {MAX_STARTUP_RECOVERY_PAGES} recovery pages")]
    PageLimit,
    #[error("workload startup recovery record disappeared for {key:?}")]
    Missing { key: WorkloadSagaKey },
    #[error("workload startup recovery page crossed current durable truth for {key:?}")]
    Crossed { key: WorkloadSagaKey },
    #[error("workload startup has no provision owner for {key:?}")]
    MissingProvisionOwner { key: WorkloadSagaKey },
    #[error("workload startup provision failed for {key:?}: {message}")]
    Provision {
        key: WorkloadSagaKey,
        message: String,
    },
    #[error("workload startup restart failed for {key:?}: {message}")]
    Restart {
        key: WorkloadSagaKey,
        message: String,
    },
    #[error("workload restart discovery could not start after recovery: {message}")]
    RestartWatch { message: String },
    #[error("workload startup has no teardown owner for {key:?}")]
    MissingTeardownOwner { key: WorkloadSagaKey },
    #[error("workload startup teardown failed for {key:?}: {message}")]
    Teardown {
        key: WorkloadSagaKey,
        message: String,
    },
}

/// One-shot compute composition for fresh-process workload recovery.
pub(crate) struct WorkloadStartupRecovery {
    coordinator: Arc<WorkloadSagaCoordinator>,
    provision_owner: Option<Arc<dyn WorkloadStartupProvisionOwner>>,
    restart_owner: Arc<dyn WorkloadStartupRestartOwner>,
    teardown_owner: Option<Arc<dyn WorkloadStartupTeardownOwner>>,
}

impl WorkloadStartupRecovery {
    pub(crate) fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        provisioner: Option<Arc<WorkloadProvisioner>>,
        restart_runtime: Arc<WorkloadRestartRuntime>,
        teardown_runtime: Option<Arc<WorkloadTeardownRuntime>>,
    ) -> Self {
        Self {
            coordinator,
            provision_owner: provisioner.map(|owner| {
                Arc::new(WorkloadStartupProvisionAdapter { owner })
                    as Arc<dyn WorkloadStartupProvisionOwner>
            }),
            restart_owner: Arc::new(WorkloadStartupRestartAdapter {
                owner: restart_runtime,
            }),
            teardown_owner: teardown_runtime.map(|owner| {
                Arc::new(WorkloadStartupTeardownAdapter { owner })
                    as Arc<dyn WorkloadStartupTeardownOwner>
            }),
        }
    }

    #[cfg(test)]
    fn with_owners(
        coordinator: Arc<WorkloadSagaCoordinator>,
        provision_owner: Option<Arc<dyn WorkloadStartupProvisionOwner>>,
        restart_owner: Arc<dyn WorkloadStartupRestartOwner>,
        teardown_owner: Option<Arc<dyn WorkloadStartupTeardownOwner>>,
    ) -> Self {
        Self {
            coordinator,
            provision_owner,
            restart_owner,
            teardown_owner,
        }
    }

    /// Enumerate every bounded recovery page once and route each exact record
    /// through its existing lifecycle owner. A stale page decision fails
    /// before this owner can call an effect-capable runtime.
    pub(crate) async fn recover_once(
        &self,
    ) -> Result<WorkloadStartupRecoveryReport, WorkloadStartupRecoveryError> {
        let mut cursor = None;
        let mut pages = 0;
        let mut outcomes = Vec::new();

        loop {
            if pages == MAX_STARTUP_RECOVERY_PAGES {
                return Err(WorkloadStartupRecoveryError::PageLimit);
            }
            let request = WorkloadSagaPageRequest::new(cursor, MAX_WORKLOAD_SAGA_PAGE_SIZE)?;
            let page = self.coordinator.plan_recoverable_page(request).await?;
            pages += 1;

            for decision in page.decisions() {
                let current = self.reload_authenticated(decision).await?;
                outcomes.push(self.route(decision, current).await?);
            }

            let Some(next) = page.next_cursor().cloned() else {
                break;
            };
            cursor = Some(next);
        }

        Ok(WorkloadStartupRecoveryReport {
            tenant_retirements: 0,
            pages,
            outcomes,
        })
    }

    /// Complete the one all-phase pass before periodic restart discovery can
    /// observe or dispatch durable candidates.
    pub(crate) async fn recover_and_activate(
        &self,
    ) -> Result<WorkloadStartupRecoveryReport, WorkloadStartupRecoveryError> {
        let report = self.recover_once().await?;
        self.restart_owner
            .activate_watch()
            .map_err(|message| WorkloadStartupRecoveryError::RestartWatch { message })?;
        Ok(report)
    }

    async fn reload_authenticated(
        &self,
        decision: &WorkloadSagaDecision,
    ) -> Result<WorkloadSagaRecord, WorkloadStartupRecoveryError> {
        let key = decision.key().clone();
        let current = self
            .coordinator
            .load(&key)
            .await?
            .ok_or_else(|| WorkloadStartupRecoveryError::Missing { key: key.clone() })?;
        if current.saga_id() != decision.saga_id()
            || current.revision() != decision.revision()
            || current.active_intent().generation() != decision.active_generation()
            || current.successor_intent() != decision.successor_intent()
        {
            return Err(WorkloadStartupRecoveryError::Crossed { key });
        }
        Ok(current)
    }

    async fn route(
        &self,
        decision: &WorkloadSagaDecision,
        current: WorkloadSagaRecord,
    ) -> Result<WorkloadStartupRecoveryOutcome, WorkloadStartupRecoveryError> {
        if current.restart_state().active().is_some() {
            return self.recover_restart(current).await;
        }

        match decision.action() {
            WorkloadSagaAction::Provision(_) => {
                let owner_reopened_publication =
                    current.needs_owner_reopened_publication_recovery();
                self.recover_provision(current, false, owner_reopened_publication)
                    .await
            }
            WorkloadSagaAction::Teardown(_) => self.recover_teardown(current, false).await,
            WorkloadSagaAction::PromoteSuccessor { .. } => self.promote_successor(current).await,
            WorkloadSagaAction::Quiescent => Ok(WorkloadStartupRecoveryOutcome::new(
                current,
                WorkloadStartupDisposition::Quiescent,
            )),
        }
    }

    async fn recover_restart(
        &self,
        current: WorkloadSagaRecord,
    ) -> Result<WorkloadStartupRecoveryOutcome, WorkloadStartupRecoveryError> {
        let key = current.key().clone();
        self.restart_owner
            .recover(current)
            .await
            .map_err(|message| WorkloadStartupRecoveryError::Restart {
                key: key.clone(),
                message,
            })?;
        let durable = self
            .coordinator
            .load(&key)
            .await?
            .ok_or_else(|| WorkloadStartupRecoveryError::Missing { key: key.clone() })?;
        let disposition = if durable.restart_state().active().is_some() {
            WorkloadStartupDisposition::RestartWaiting
        } else {
            WorkloadStartupDisposition::RestartSettled
        };
        Ok(WorkloadStartupRecoveryOutcome::new(durable, disposition))
    }

    async fn recover_provision(
        &self,
        current: WorkloadSagaRecord,
        promoted_successor: bool,
        owner_reopened_publication: bool,
    ) -> Result<WorkloadStartupRecoveryOutcome, WorkloadStartupRecoveryError> {
        let key = current.key().clone();
        let provisioner = self.provision_owner.as_ref().ok_or_else(|| {
            WorkloadStartupRecoveryError::MissingProvisionOwner { key: key.clone() }
        })?;
        let outcome = provisioner
            .resume(key.clone(), owner_reopened_publication)
            .await
            .map_err(|error| WorkloadStartupRecoveryError::Provision {
                key: key.clone(),
                message: error.to_string(),
            })?;

        if outcome.disposition == WorkloadProvisionRunDisposition::SuccessorSettlementReady {
            let withdrawal = self
                .coordinator
                .commit_provision_settlement_teardown(&outcome.record)
                .await?;
            return self.recover_teardown(withdrawal, true).await;
        }
        if outcome.disposition == WorkloadProvisionRunDisposition::SuccessorSettlementCommitted {
            return self.recover_teardown(outcome.record, true).await;
        }

        let disposition = match outcome.compensation {
            WorkloadProvisionCompensationState::CleanupPending => {
                WorkloadStartupDisposition::CleanupRetained
            }
            WorkloadProvisionCompensationState::Waiting => {
                if promoted_successor {
                    WorkloadStartupDisposition::SuccessorRunningCompensationWaiting
                } else {
                    WorkloadStartupDisposition::ProvisionCompensationWaiting
                }
            }
            WorkloadProvisionCompensationState::Completed => {
                if promoted_successor {
                    WorkloadStartupDisposition::SuccessorRunningCompensated
                } else {
                    WorkloadStartupDisposition::ProvisionCompensated
                }
            }
            WorkloadProvisionCompensationState::NotRequired => match outcome.disposition {
                WorkloadProvisionRunDisposition::Observed => {
                    if promoted_successor {
                        WorkloadStartupDisposition::SuccessorRunningObserved
                    } else {
                        WorkloadStartupDisposition::ProvisionObserved
                    }
                }
                WorkloadProvisionRunDisposition::Waiting => {
                    if promoted_successor {
                        WorkloadStartupDisposition::SuccessorRunningWaiting
                    } else {
                        WorkloadStartupDisposition::ProvisionWaiting
                    }
                }
                WorkloadProvisionRunDisposition::DefiniteFailure => {
                    return Err(WorkloadStartupRecoveryError::Provision {
                        key,
                        message: "definite failure returned without compensation state".to_owned(),
                    });
                }
                WorkloadProvisionRunDisposition::SuccessorSettlementReady => {
                    unreachable!("successor settlement returned before outcome mapping")
                }
                WorkloadProvisionRunDisposition::SuccessorSettlementCommitted => {
                    unreachable!("committed successor settlement returned before outcome mapping")
                }
            },
        };
        Ok(WorkloadStartupRecoveryOutcome::new(
            outcome.record,
            disposition,
        ))
    }

    async fn recover_teardown(
        &self,
        current: WorkloadSagaRecord,
        after_provision_settlement: bool,
    ) -> Result<WorkloadStartupRecoveryOutcome, WorkloadStartupRecoveryError> {
        let key = current.key().clone();
        let runtime = self.teardown_owner.as_ref().ok_or_else(|| {
            WorkloadStartupRecoveryError::MissingTeardownOwner { key: key.clone() }
        })?;
        let run = runtime.submit(key.clone()).await.map_err(|error| {
            WorkloadStartupRecoveryError::Teardown {
                key,
                message: error.to_string(),
            }
        })?;
        let disposition = match run.disposition {
            WorkloadTeardownRunDisposition::Completed if after_provision_settlement => {
                WorkloadStartupDisposition::SuccessorSettlementTeardownCompleted
            }
            WorkloadTeardownRunDisposition::Completed => {
                WorkloadStartupDisposition::TeardownCompleted
            }
            WorkloadTeardownRunDisposition::Waiting if after_provision_settlement => {
                WorkloadStartupDisposition::SuccessorSettlementTeardownWaiting
            }
            WorkloadTeardownRunDisposition::Waiting => WorkloadStartupDisposition::TeardownWaiting,
            WorkloadTeardownRunDisposition::CleanupPending => {
                WorkloadStartupDisposition::CleanupRetained
            }
        };
        Ok(WorkloadStartupRecoveryOutcome::new(run.record, disposition))
    }

    async fn promote_successor(
        &self,
        current: WorkloadSagaRecord,
    ) -> Result<WorkloadStartupRecoveryOutcome, WorkloadStartupRecoveryError> {
        let promoted = self
            .coordinator
            .promote_recorded_successor(&current)
            .await?;
        match promoted.active_intent().desired_state() {
            DesiredWorkloadState::Running => self.recover_provision(promoted, true, false).await,
            DesiredWorkloadState::Stopped => {
                debug_assert_eq!(promoted.phase(), WorkloadSagaPhase::Recorded);
                Ok(WorkloadStartupRecoveryOutcome::new(
                    promoted,
                    WorkloadStartupDisposition::SuccessorStopped,
                ))
            }
        }
    }
}

#[cfg(test)]
#[path = "startup_recovery/tests.rs"]
mod tests;
