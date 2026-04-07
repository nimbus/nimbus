use std::sync::Arc;

use crate::executor::{RuntimeWorkerQueue, RuntimeWorkerShutdown};
use crate::runtime::CooperativeRuntimeSlotPoll;

use super::{CooperativeInvocation, CooperativeRunnableSlot, CooperativeWorkerLoop, WorkerLoop};

impl CooperativeWorkerLoop {
    pub(super) fn next_slot(
        &mut self,
        queue: &Arc<dyn RuntimeWorkerQueue>,
        shutdown: &RuntimeWorkerShutdown,
    ) -> Option<CooperativeRunnableSlot<CooperativeInvocation>> {
        loop {
            while let Some(job) = queue.try_recv() {
                self.admit_job(queue, job);
            }
            self.drain_ready_parked_slots();

            if let Some(slot) = self.scheduler.pop_runnable() {
                return Some(slot);
            }

            if shutdown.is_cancelled() {
                return None;
            }

            if self.scheduler.has_parked() {
                let activity_signal = self.activity_signal.clone();
                let mut activity_generation = self.activity_generation;
                self.activity_generation = self.worker_runtime.block_on(async move {
                    activity_signal
                        .wait_for_change_async(&mut activity_generation)
                        .await;
                    activity_generation
                });
                continue;
            }

            self.drain_deferred_runtime_drops_if_idle();
            let job = queue.recv_blocking()?;
            self.admit_job(queue, job);
        }
    }
}

impl WorkerLoop for CooperativeWorkerLoop {
    fn run(&mut self, queue: Arc<dyn RuntimeWorkerQueue>, shutdown: RuntimeWorkerShutdown) {
        self.activity_signal = queue.activity_signal();
        self.activity_generation = self.activity_signal.current_generation();
        while !shutdown.is_cancelled() {
            let Some(slot) = self.next_slot(&queue, &shutdown) else {
                if self.scheduler.is_idle() {
                    break;
                }
                continue;
            };

            let slot_id = slot.slot_id;
            let mut invocation = slot.payload;
            match self.worker_runtime.block_on(invocation.slot.poll_once()) {
                Ok(CooperativeRuntimeSlotPoll::Runnable) => {
                    self.scheduler.requeue_runnable(CooperativeRunnableSlot {
                        slot_id,
                        payload: invocation,
                    });
                }
                Ok(CooperativeRuntimeSlotPoll::Parked) => {
                    self.scheduler.park(CooperativeRunnableSlot {
                        slot_id,
                        payload: invocation,
                    });
                }
                Ok(CooperativeRuntimeSlotPoll::Completed) => {
                    let CooperativeInvocation {
                        job,
                        permit,
                        slot,
                        execution_started_at,
                        cancellation_for_metrics,
                    } = invocation;
                    let (result, reusable_runtime) =
                        self.worker_runtime.block_on(slot.finish_with_runtime());
                    if let Some(runtime) = reusable_runtime {
                        self.retain_or_defer_runtime_drop(
                            &job.runtime,
                            &job.bundle,
                            &job.context,
                            runtime,
                        );
                    }
                    let (job, result, ready_jobs) =
                        self.worker_runtime.block_on(Self::finish_invocation(
                            self.policy.clone(),
                            self.worker_id,
                            job,
                            permit,
                            execution_started_at,
                            cancellation_for_metrics,
                            result,
                        ));
                    self.scheduler.finish(slot_id);
                    self.drain_deferred_runtime_drops_if_idle();
                    queue.complete_job(job, result, ready_jobs);
                }
                Err(error) => {
                    let CooperativeInvocation {
                        job,
                        permit,
                        slot,
                        execution_started_at,
                        cancellation_for_metrics,
                    } = invocation;
                    let (result, reusable_runtime) = self
                        .worker_runtime
                        .block_on(slot.finish_with_result_and_runtime(Err(error)));
                    if let Some(runtime) = reusable_runtime {
                        self.retain_or_defer_runtime_drop(
                            &job.runtime,
                            &job.bundle,
                            &job.context,
                            runtime,
                        );
                    }
                    let (job, result, ready_jobs) =
                        self.worker_runtime.block_on(Self::finish_invocation(
                            self.policy.clone(),
                            self.worker_id,
                            job,
                            permit,
                            execution_started_at,
                            cancellation_for_metrics,
                            result,
                        ));
                    self.scheduler.finish(slot_id);
                    self.drain_deferred_runtime_drops_if_idle();
                    queue.complete_job(job, result, ready_jobs);
                }
            }
        }

        self.deferred_runtime_drops.clear();
    }
}
