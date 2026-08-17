//! Explicit retained runtime for exact workload teardown keys.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_network::NetworkCapabilityRegistry;
use nimbus_workloads::WorkloadSagaKey;
use thiserror::Error;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::teardown_dispatch::WorkloadTeardownDispatcher;
use super::teardown_driver::{
    WorkloadTeardownDriver, WorkloadTeardownRun, WorkloadTeardownRunError,
};
use super::teardown_registry::WorkloadTeardownCapabilityRegistry;
use super::{WorkloadProvisionSourceAuthority, WorkloadSagaCoordinator};

#[cfg(test)]
type TestRegistrationBoundary = (Arc<std::sync::Barrier>, Arc<std::sync::Barrier>);

/// Caller cancellation can prevent submission or detach one waiter. It never
/// revokes retained durable teardown work after submission.
#[derive(Debug, Clone)]
pub struct WorkloadTeardownCancellationToken {
    signal: watch::Sender<bool>,
    #[cfg(test)]
    registration_boundary: Arc<Mutex<Option<TestRegistrationBoundary>>>,
}

impl Default for WorkloadTeardownCancellationToken {
    fn default() -> Self {
        let (signal, _) = watch::channel(false);
        Self {
            signal,
            #[cfg(test)]
            registration_boundary: Arc::new(Mutex::new(None)),
        }
    }
}

impl WorkloadTeardownCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.signal.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.signal.borrow()
    }

    /// Wait until this caller cancels its interest in retained teardown work.
    /// Cancellation never revokes work that is already durable or retained by
    /// the runtime.
    pub async fn cancelled(&self) {
        let mut receiver = self.signal.subscribe();
        if *receiver.borrow() {
            return;
        }
        while receiver.changed().await.is_ok() {
            if *receiver.borrow() {
                return;
            }
        }
    }

    fn register_waiter(&self) -> Result<watch::Receiver<bool>, WorkloadTeardownSubmissionError> {
        let mut receiver = self.signal.subscribe();
        #[cfg(test)]
        self.pause_at_test_registration_boundary();
        let cancelled = *receiver.borrow_and_update();
        if cancelled {
            return Err(WorkloadTeardownSubmissionError::Cancelled);
        }
        Ok(receiver)
    }

    #[cfg(test)]
    fn install_test_registration_boundary(
        &self,
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    ) {
        *self
            .registration_boundary
            .lock()
            .expect("teardown cancellation registration lock should not be poisoned") =
            Some((entered, release));
    }

    #[cfg(test)]
    fn pause_at_test_registration_boundary(&self) {
        let boundary = self
            .registration_boundary
            .lock()
            .expect("teardown cancellation registration lock should not be poisoned")
            .clone();
        if let Some((entered, release)) = boundary {
            entered.wait();
            release.wait();
        }
    }
}

/// Explicit submission failure for one exact retained teardown key.
#[derive(Debug, Error)]
pub enum WorkloadTeardownSubmissionError {
    #[error("workload teardown submission was cancelled")]
    Cancelled,
    #[error("retained workload teardown task ended before publishing a result: {0}")]
    TaskEnded(Arc<str>),
    #[error("workload teardown run failed: {0}")]
    Run(Arc<WorkloadTeardownRunError>),
}

#[derive(Debug, Clone)]
enum RetainedWorkloadTeardownFailure {
    TaskEnded(Arc<str>),
    Run(Arc<WorkloadTeardownRunError>),
}

type RetainedWorkloadTeardownResult = Result<WorkloadTeardownRun, RetainedWorkloadTeardownFailure>;

struct InFlightWorkloadTeardown {
    completion: watch::Receiver<Option<RetainedWorkloadTeardownResult>>,
    _task: Option<JoinHandle<()>>,
}

/// Exact-key runtime with no watch, tenant scan, or `ComputeState` ownership.
pub struct WorkloadTeardownRuntime {
    driver: Arc<WorkloadTeardownDriver>,
    in_flight: Arc<Mutex<BTreeMap<WorkloadSagaKey, InFlightWorkloadTeardown>>>,
    #[cfg(test)]
    retained_join_boundary: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
}

impl WorkloadTeardownRuntime {
    pub fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
        provider_reports: NetworkCapabilityRegistry,
        capabilities: Arc<WorkloadTeardownCapabilityRegistry>,
    ) -> Self {
        let dispatcher = Arc::new(WorkloadTeardownDispatcher::new(
            source_authority,
            provider_reports,
            capabilities,
        ));
        Self {
            driver: Arc::new(WorkloadTeardownDriver::new(coordinator, dispatcher)),
            in_flight: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            retained_join_boundary: Mutex::new(None),
        }
    }

    /// Submit retained work for one exact key. Duplicate waiters attach to the
    /// same task. Cancelling after registration detaches only that waiter.
    pub async fn submit(
        &self,
        key: WorkloadSagaKey,
        cancellation: &WorkloadTeardownCancellationToken,
    ) -> Result<WorkloadTeardownRun, WorkloadTeardownSubmissionError> {
        let cancelled = cancellation.register_waiter()?;
        let completion = self.track_resume(key);
        wait_for_completion(completion, cancelled).await
    }

    fn track_resume(
        &self,
        key: WorkloadSagaKey,
    ) -> watch::Receiver<Option<RetainedWorkloadTeardownResult>> {
        let mut in_flight = self
            .in_flight
            .lock()
            .expect("workload teardown supervisor lock should not be poisoned");
        if let Some(existing) = in_flight.get(&key) {
            #[cfg(test)]
            self.notify_test_retained_join_boundary();
            return existing.completion.clone();
        }

        let (sender, receiver) = watch::channel(None);
        in_flight.insert(
            key.clone(),
            InFlightWorkloadTeardown {
                completion: receiver.clone(),
                _task: None,
            },
        );
        drop(in_flight);

        let driver = Arc::clone(&self.driver);
        let worker_key = key.clone();
        let worker = tokio::spawn(async move { driver.resume(&worker_key).await });
        let retained = Arc::clone(&self.in_flight);
        let task_key = key.clone();
        let supervisor = tokio::spawn(async move {
            let result = match worker.await {
                Ok(Ok(run)) => Ok(run),
                Ok(Err(error)) => Err(RetainedWorkloadTeardownFailure::Run(Arc::new(error))),
                Err(error) => Err(RetainedWorkloadTeardownFailure::TaskEnded(Arc::from(
                    error.to_string(),
                ))),
            };
            sender.send_replace(Some(result));
            retained
                .lock()
                .expect("workload teardown supervisor lock should not be poisoned")
                .remove(&task_key);
        });
        if let Some(entry) = self
            .in_flight
            .lock()
            .expect("workload teardown supervisor lock should not be poisoned")
            .get_mut(&key)
        {
            entry._task = Some(supervisor);
        }
        receiver
    }

    #[cfg(test)]
    fn install_test_retained_join_boundary(&self, entered: Arc<tokio::sync::Semaphore>) {
        *self
            .retained_join_boundary
            .lock()
            .expect("teardown retained-join lock should not be poisoned") = Some(entered);
    }

    #[cfg(test)]
    fn notify_test_retained_join_boundary(&self) {
        if let Some(entered) = self
            .retained_join_boundary
            .lock()
            .expect("teardown retained-join lock should not be poisoned")
            .as_ref()
        {
            entered.add_permits(1);
        }
    }
}

async fn wait_for_completion(
    mut completion: watch::Receiver<Option<RetainedWorkloadTeardownResult>>,
    mut cancelled: watch::Receiver<bool>,
) -> Result<WorkloadTeardownRun, WorkloadTeardownSubmissionError> {
    loop {
        if let Some(result) = completion.borrow().clone() {
            return match result {
                Ok(run) => Ok(run),
                Err(RetainedWorkloadTeardownFailure::Run(error)) => {
                    Err(WorkloadTeardownSubmissionError::Run(error))
                }
                Err(RetainedWorkloadTeardownFailure::TaskEnded(error)) => {
                    Err(WorkloadTeardownSubmissionError::TaskEnded(error))
                }
            };
        }
        if *cancelled.borrow() {
            return Err(WorkloadTeardownSubmissionError::Cancelled);
        }
        tokio::select! {
            changed = cancelled.changed() => {
                if changed.is_err() || *cancelled.borrow() {
                    return Err(WorkloadTeardownSubmissionError::Cancelled);
                }
            }
            changed = completion.changed() => {
                if changed.is_err() {
                    return Err(WorkloadTeardownSubmissionError::TaskEnded(
                        Arc::from("completion channel closed"),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "teardown_runtime/tests.rs"]
mod tests;
