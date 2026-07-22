use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::{
    HostCallCancellation, NimbusRuntimeError, Result, RuntimeDeploymentAuthorityId,
    RuntimeDeploymentAuthorityRevocation, RuntimeInvocationContext, RuntimeOwnerId,
    RuntimeOwnerRevocation,
};

use super::RuntimeExecutor;
use super::queue::RuntimeWorkerControlCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeRetirementReport {
    pub workers_acknowledged: usize,
    pub retained_entries_purged: usize,
    pub invocations_cancelled: usize,
    pub affinity_entries_purged: usize,
}

pub(super) struct RuntimeRetirementRegistry {
    entries: Mutex<HashMap<u64, RuntimeRetirementEntry>>,
    changed: Notify,
}

struct RuntimeRetirementEntry {
    owner_id: Option<RuntimeOwnerId>,
    deployment_authority_id: Option<RuntimeDeploymentAuthorityId>,
    cancellation: HostCallCancellation,
}

pub(crate) struct RuntimeRetirementGuard {
    invocation_id: u64,
    registry: Arc<RuntimeRetirementRegistry>,
}

impl RuntimeRetirementRegistry {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(HashMap::new()),
            changed: Notify::new(),
        })
    }

    pub(super) fn register(
        self: &Arc<Self>,
        context: &RuntimeInvocationContext,
        cancellation: HostCallCancellation,
    ) -> Option<RuntimeRetirementGuard> {
        let owner_id = context
            .runtime_owner_lease()
            .map(|lease| lease.owner_id().clone());
        let deployment_authority_id = context
            .deployment_authority_lease()
            .map(|lease| lease.authority_id().clone());
        if owner_id.is_none() && deployment_authority_id.is_none() {
            return None;
        }
        self.entries
            .lock()
            .expect("runtime retirement registry lock should not be poisoned")
            .insert(
                context.invocation_id,
                RuntimeRetirementEntry {
                    owner_id,
                    deployment_authority_id,
                    cancellation,
                },
            );
        Some(RuntimeRetirementGuard {
            invocation_id: context.invocation_id,
            registry: self.clone(),
        })
    }

    pub(super) fn cancel_owner(&self, owner_id: &RuntimeOwnerId) -> usize {
        self.cancel_matching(|entry| entry.owner_id.as_ref() == Some(owner_id))
    }

    pub(super) async fn wait_for_owner_drain(&self, owner_id: &RuntimeOwnerId) {
        self.wait_for_matching_drain(|entry| entry.owner_id.as_ref() == Some(owner_id))
            .await;
    }

    pub(super) async fn wait_for_deployment_authority_drain(
        &self,
        authority_id: &RuntimeDeploymentAuthorityId,
    ) {
        self.wait_for_matching_drain(|entry| {
            entry.deployment_authority_id.as_ref() == Some(authority_id)
        })
        .await;
    }

    fn cancel_matching(&self, predicate: impl Fn(&RuntimeRetirementEntry) -> bool) -> usize {
        let cancellations = self
            .entries
            .lock()
            .expect("runtime retirement registry lock should not be poisoned")
            .values()
            .filter(|entry| predicate(entry))
            .map(|entry| entry.cancellation.clone())
            .collect::<Vec<_>>();
        let count = cancellations.len();
        for cancellation in cancellations {
            cancellation.cancel();
        }
        count
    }

    async fn wait_for_matching_drain(&self, predicate: impl Fn(&RuntimeRetirementEntry) -> bool) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let has_matching = self
                .entries
                .lock()
                .expect("runtime retirement registry lock should not be poisoned")
                .values()
                .any(&predicate);
            if !has_matching {
                return;
            }
            notified.await;
        }
    }
}

impl Drop for RuntimeRetirementGuard {
    fn drop(&mut self) {
        self.registry
            .entries
            .lock()
            .expect("runtime retirement registry lock should not be poisoned")
            .remove(&self.invocation_id);
        self.registry.changed.notify_waiters();
    }
}

impl RuntimeExecutor {
    pub async fn retire_owner(
        &self,
        revocation: &RuntimeOwnerRevocation,
        timeout: std::time::Duration,
    ) -> Result<RuntimeRetirementReport> {
        revocation.revoke();
        self.inner
            .policy
            .metrics()
            .record_retained_owner_revocation();
        let owner_id = revocation.owner_id().clone();
        let mut affinity_entries_purged = self.inner.router.purge_owner_affinity(&owner_id);
        let queued_jobs = self.inner.admission.cancel_queued_owner(&owner_id);
        let queued_cancelled = queued_jobs.len();
        for job in queued_jobs {
            if let Some(cancellation) = &job.cancellation {
                cancellation.cancel();
            }
            job.result_tx.send(Err(NimbusRuntimeError::Cancelled));
        }
        let invocations_cancelled = self.inner.retirement.cancel_owner(&owner_id);
        let retirement = async {
            let acknowledgements = self
                .inner
                .router
                .broadcast_retirement(|| RuntimeWorkerControlCommand::RetireOwner(owner_id.clone()))
                .await?;
            let mut workers_acknowledged = 0;
            let mut retained_entries_purged = 0;
            for acknowledgement in acknowledgements {
                let acknowledgement = acknowledgement.await.map_err(|_| {
                    NimbusRuntimeError::Contract(
                        "runtime owner retirement acknowledgement channel closed".to_string(),
                    )
                })?;
                workers_acknowledged += 1;
                retained_entries_purged += acknowledgement.retained_entries_purged;
            }
            self.inner.retirement.wait_for_owner_drain(&owner_id).await;
            affinity_entries_purged = affinity_entries_purged
                .saturating_add(self.inner.router.purge_owner_affinity(&owner_id));
            Ok(RuntimeRetirementReport {
                workers_acknowledged,
                retained_entries_purged,
                invocations_cancelled: invocations_cancelled.saturating_add(queued_cancelled),
                affinity_entries_purged,
            })
        };
        let result = if timeout.is_zero() {
            Err(NimbusRuntimeError::RetirementTimeout {
                scope: "owner",
                timeout,
            })
        } else {
            match tokio::time::timeout(timeout, retirement).await {
                Ok(result) => result,
                Err(_) => Err(NimbusRuntimeError::RetirementTimeout {
                    scope: "owner",
                    timeout,
                }),
            }
        };
        match &result {
            Ok(report) => self
                .inner
                .policy
                .metrics()
                .record_retained_owner_retirement_purges(report.retained_entries_purged),
            Err(_) => self
                .inner
                .policy
                .metrics()
                .record_retained_owner_retirement_acknowledgement_failure(),
        }
        result
    }

    pub async fn retire_deployment_authority(
        &self,
        revocation: &RuntimeDeploymentAuthorityRevocation,
        timeout: std::time::Duration,
    ) -> Result<RuntimeRetirementReport> {
        revocation.revoke();
        self.inner
            .policy
            .metrics()
            .record_retained_owner_revocation();
        let authority_id = revocation.authority_id().clone();
        let mut affinity_entries_purged = self
            .inner
            .router
            .purge_deployment_authority_affinity(&authority_id);
        let queued_jobs = self
            .inner
            .admission
            .cancel_queued_deployment_authority(&authority_id);
        let queued_cancelled = queued_jobs.len();
        for job in queued_jobs {
            if let Some(cancellation) = &job.cancellation {
                cancellation.cancel();
            }
            job.result_tx.send(Err(NimbusRuntimeError::Cancelled));
        }
        let retirement = async {
            let acknowledgements = self
                .inner
                .router
                .broadcast_retirement(|| {
                    RuntimeWorkerControlCommand::RetireDeploymentAuthority(authority_id.clone())
                })
                .await?;
            let mut workers_acknowledged = 0;
            let mut retained_entries_purged = 0;
            for acknowledgement in acknowledgements {
                let acknowledgement = acknowledgement.await.map_err(|_| {
                    NimbusRuntimeError::Contract(
                        "runtime deployment retirement acknowledgement channel closed".to_string(),
                    )
                })?;
                workers_acknowledged += 1;
                retained_entries_purged += acknowledgement.retained_entries_purged;
            }
            self.inner
                .retirement
                .wait_for_deployment_authority_drain(&authority_id)
                .await;
            affinity_entries_purged = affinity_entries_purged.saturating_add(
                self.inner
                    .router
                    .purge_deployment_authority_affinity(&authority_id),
            );
            Ok(RuntimeRetirementReport {
                workers_acknowledged,
                retained_entries_purged,
                // Deployment activation condemns retained state immediately,
                // but work that was already executing may drain normally.
                invocations_cancelled: queued_cancelled,
                affinity_entries_purged,
            })
        };
        let result = if timeout.is_zero() {
            Err(NimbusRuntimeError::RetirementTimeout {
                scope: "deployment authority",
                timeout,
            })
        } else {
            match tokio::time::timeout(timeout, retirement).await {
                Ok(result) => result,
                Err(_) => Err(NimbusRuntimeError::RetirementTimeout {
                    scope: "deployment authority",
                    timeout,
                }),
            }
        };
        match &result {
            Ok(report) => self
                .inner
                .policy
                .metrics()
                .record_retained_owner_retirement_purges(report.retained_entries_purged),
            Err(_) => self
                .inner
                .policy
                .metrics()
                .record_retained_owner_retirement_acknowledgement_failure(),
        }
        result
    }
}
