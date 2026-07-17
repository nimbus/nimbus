use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use nimbus_core::{Error, Result};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::requests::DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY;
use super::stats::MutationIsolateAdmissionStats;

const DEFAULT_TENANT_MUTATION_ISOLATE_CEILING: usize = 16;

pub(in crate::tenant) struct MutationIsolateAdmission {
    semaphore: Arc<Semaphore>,
    ceiling: usize,
    waiting_capacity: usize,
    concurrent_count: AtomicUsize,
    waiting_count: AtomicUsize,
    max_concurrent_count: AtomicUsize,
    admitted_count: AtomicU64,
    shed_count: AtomicU64,
}

pub(crate) struct MutationIsolateAdmissionPermit {
    admission: Arc<MutationIsolateAdmission>,
    _permit: OwnedSemaphorePermit,
}

struct WaitingRegistration<'a> {
    admission: &'a MutationIsolateAdmission,
}

impl Drop for WaitingRegistration<'_> {
    fn drop(&mut self) {
        self.admission.waiting_count.fetch_sub(1, Ordering::AcqRel);
    }
}

impl MutationIsolateAdmission {
    pub(in crate::tenant) fn from_env() -> Self {
        Self::new(
            env_positive_usize(
                "NIMBUS_TENANT_MUTATION_ISOLATE_CEILING",
                DEFAULT_TENANT_MUTATION_ISOLATE_CEILING,
            ),
            DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY,
        )
    }

    fn new(ceiling: usize, waiting_capacity: usize) -> Self {
        let ceiling = ceiling.max(1);
        Self {
            semaphore: Arc::new(Semaphore::new(ceiling)),
            ceiling,
            waiting_capacity: waiting_capacity.max(1),
            concurrent_count: AtomicUsize::new(0),
            waiting_count: AtomicUsize::new(0),
            max_concurrent_count: AtomicUsize::new(0),
            admitted_count: AtomicU64::new(0),
            shed_count: AtomicU64::new(0),
        }
    }

    pub(in crate::tenant) async fn acquire(
        self: &Arc<Self>,
    ) -> Result<MutationIsolateAdmissionPermit> {
        let prior_waiters = self.waiting_count.fetch_add(1, Ordering::AcqRel);
        if prior_waiters >= self.waiting_capacity {
            self.waiting_count.fetch_sub(1, Ordering::AcqRel);
            self.shed_count.fetch_add(1, Ordering::Relaxed);
            return Err(Error::overloaded(format!(
                "tenant mutation isolate admission queue full (capacity {})",
                self.waiting_capacity
            )));
        }
        let waiting = WaitingRegistration { admission: self };
        let permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .map_err(|_| Error::Internal("tenant mutation isolate admission closed".to_string()))?;
        drop(waiting);

        let concurrent = self.concurrent_count.fetch_add(1, Ordering::AcqRel) + 1;
        debug_assert!(
            concurrent <= self.ceiling,
            "mutation isolate count {concurrent} exceeded tenant ceiling {}",
            self.ceiling
        );
        self.max_concurrent_count
            .fetch_max(concurrent, Ordering::AcqRel);
        self.admitted_count.fetch_add(1, Ordering::Relaxed);
        Ok(MutationIsolateAdmissionPermit {
            admission: Arc::clone(self),
            _permit: permit,
        })
    }

    pub(in crate::tenant) fn stats(&self) -> MutationIsolateAdmissionStats {
        MutationIsolateAdmissionStats {
            concurrent_count: self.concurrent_count.load(Ordering::Acquire),
            ceiling: self.ceiling,
            waiting_count: self.waiting_count.load(Ordering::Acquire),
            waiting_capacity: self.waiting_capacity,
            max_concurrent_count: self.max_concurrent_count.load(Ordering::Acquire),
            admitted_count: self.admitted_count.load(Ordering::Relaxed),
            shed_count: self.shed_count.load(Ordering::Relaxed),
        }
    }
}

impl Drop for MutationIsolateAdmissionPermit {
    fn drop(&mut self) {
        let previous = self
            .admission
            .concurrent_count
            .fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "mutation isolate permit count underflow");
    }
}

fn env_positive_usize(name: &str, default: usize) -> usize {
    std::env::var_os(name)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use nimbus_core::Error;
    use tokio::sync::Barrier;

    use super::*;

    async fn wait_for_waiting_count(admission: &MutationIsolateAdmission, expected: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while admission.stats().waiting_count < expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("mutation isolate waiters should register");
    }

    #[tokio::test]
    async fn concurrent_burst_respects_mutation_isolate_ceiling_and_reports_counts() {
        const CEILING: usize = 2;
        const REQUESTS: usize = 6;

        let admission = Arc::new(MutationIsolateAdmission::new(CEILING, REQUESTS));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let first_wave = Arc::new(Barrier::new(CEILING + 1));
        let release = Arc::new(Barrier::new(CEILING + 1));
        let mut tasks = Vec::new();
        for _ in 0..REQUESTS {
            let admission = admission.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            let first_wave = first_wave.clone();
            let release = release.clone();
            tasks.push(tokio::spawn(async move {
                let permit = admission.acquire().await.expect("burst should wait");
                let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                max_active.fetch_max(current, Ordering::AcqRel);
                if admission.stats().admitted_count <= CEILING as u64 {
                    first_wave.wait().await;
                    release.wait().await;
                }
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::AcqRel);
                drop(permit);
            }));
        }

        first_wave.wait().await;
        wait_for_waiting_count(&admission, REQUESTS - CEILING).await;
        let held_stats = admission.stats();
        assert_eq!(held_stats.concurrent_count, CEILING);
        assert_eq!(held_stats.ceiling, CEILING);
        assert!(held_stats.waiting_count >= REQUESTS - CEILING);
        release.wait().await;
        for task in tasks {
            task.await.expect("burst task should join");
        }

        let stats = admission.stats();
        assert_eq!(max_active.load(Ordering::Acquire), CEILING);
        assert_eq!(stats.concurrent_count, 0);
        assert_eq!(stats.waiting_count, 0);
        assert_eq!(stats.max_concurrent_count, CEILING);
        assert_eq!(stats.admitted_count, REQUESTS as u64);
        assert_eq!(stats.shed_count, 0);
    }

    #[tokio::test]
    async fn mutation_isolate_ceiling_one_serializes_execution() {
        const REQUESTS: usize = 4;

        let admission = Arc::new(MutationIsolateAdmission::new(1, REQUESTS));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..REQUESTS {
            let admission = admission.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            tasks.push(tokio::spawn(async move {
                let _permit = admission.acquire().await.expect("request should wait");
                let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                max_active.fetch_max(current, Ordering::AcqRel);
                tokio::time::sleep(Duration::from_millis(2)).await;
                active.fetch_sub(1, Ordering::AcqRel);
            }));
        }
        for task in tasks {
            task.await.expect("serialized task should join");
        }

        assert_eq!(max_active.load(Ordering::Acquire), 1);
        let stats = admission.stats();
        assert_eq!(stats.ceiling, 1);
        assert_eq!(stats.max_concurrent_count, 1);
        assert_eq!(stats.admitted_count, REQUESTS as u64);
        assert_eq!(stats.shed_count, 0);
    }

    #[tokio::test]
    async fn mutation_isolate_wait_queue_sheds_with_typed_overload() {
        let admission = Arc::new(MutationIsolateAdmission::new(1, 1));
        let held = admission.acquire().await.expect("first seat should admit");
        let queued_admission = admission.clone();
        let queued = tokio::spawn(async move { queued_admission.acquire().await });
        wait_for_waiting_count(&admission, 1).await;

        let error = match admission.acquire().await {
            Ok(_) => panic!("request beyond bounded wait queue should shed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            Error::Overloaded { message }
                if message.contains("mutation isolate admission queue full (capacity 1)")
        ));
        assert_eq!(admission.stats().shed_count, 1);

        drop(held);
        drop(
            queued
                .await
                .expect("queued task should join")
                .expect("queued seat"),
        );
    }
}
