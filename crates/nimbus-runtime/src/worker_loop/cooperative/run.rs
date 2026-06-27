use std::sync::Arc;

use crate::error::NimbusRuntimeError;
use crate::executor::{RuntimeWorkerJob, RuntimeWorkerQueue, RuntimeWorkerShutdown};
use crate::runtime::CooperativeRuntimeSlotPoll;

use super::backend::{CooperativeBackendDriver, CooperativeBackendSlot};
use super::{CooperativeInvocation, CooperativeRunnableSlot, CooperativeWorkerLoop, WorkerLoop};

impl<D: CooperativeBackendDriver> CooperativeWorkerLoop<D> {
    fn finish_slot_with_result(
        &mut self,
        queue: &Arc<dyn RuntimeWorkerQueue>,
        slot_id: usize,
        invocation: CooperativeInvocation<D::Slot>,
        result: crate::error::Result<serde_json::Value>,
        finish_scheduler_slot: bool,
    ) {
        let CooperativeInvocation {
            job,
            permit,
            slot,
            execution_started_at,
            cancellation_for_metrics,
        } = invocation;
        let (result, reusable_runtime) = self
            .worker_runtime
            .block_on(slot.finish_with_result_and_runtime(result));
        if let Some(runtime) = reusable_runtime {
            self.retain_or_defer_runtime_drop(&job.host, &job.bundle, &job.context, runtime);
        }
        let (job, result, ready_jobs) = self.worker_runtime.block_on(Self::finish_invocation(
            self.policy.clone(),
            self.worker_id,
            job,
            permit,
            execution_started_at,
            cancellation_for_metrics,
            result,
        ));
        if finish_scheduler_slot {
            self.scheduler.finish(slot_id);
        }
        self.drain_deferred_v8_runtime_drops_if_idle();
        queue.complete_job(job, result, ready_jobs);
    }

    fn drain_cancelled_slots(&mut self, queue: &Arc<dyn RuntimeWorkerQueue>) {
        for slot in self.scheduler.drain_retained_slots() {
            self.finish_slot_with_result(
                queue,
                slot.slot_id,
                slot.payload,
                Err(NimbusRuntimeError::Cancelled),
                false,
            );
        }
    }

    fn drain_pending_admissions(&mut self, queue: &Arc<dyn RuntimeWorkerQueue>) {
        for job in self.pending_admissions.drain(..) {
            queue.complete_job(job, Err(NimbusRuntimeError::Cancelled), Vec::new());
        }
    }

    fn next_admission_job(
        &mut self,
        queue: &Arc<dyn RuntimeWorkerQueue>,
    ) -> Option<RuntimeWorkerJob> {
        self.pending_admissions
            .pop_front()
            .or_else(|| queue.try_recv())
    }

    pub(super) fn next_slot(
        &mut self,
        queue: &Arc<dyn RuntimeWorkerQueue>,
        shutdown: &RuntimeWorkerShutdown,
    ) -> Option<CooperativeRunnableSlot<CooperativeInvocation<D::Slot>>> {
        loop {
            self.drain_ready_parked_slots();

            if let Some(slot) = self.scheduler.pop_runnable() {
                return Some(slot);
            }

            // Admit at most one job per iteration. A parked slot can reacquire
            // the runtime semaphore between the ready drain above and a fresh
            // queue admission. While parked work exists, avoid blocking this
            // single worker on a new admission; defer the job locally so the
            // slot holding capacity can be polled.
            if let Some(job) = self.next_admission_job(queue) {
                if !self.driver.permits_scheduler_admission(&job.execution_plan)
                    && !self.scheduler.is_idle()
                {
                    self.pending_admissions.push_front(job);
                } else {
                    let deferred = if self.scheduler.has_parked() {
                        self.try_admit_job(queue, job)
                    } else {
                        self.admit_job(queue, job);
                        None
                    };
                    if let Some(job) = deferred {
                        self.pending_admissions.push_front(job);
                    } else {
                        continue;
                    }
                }
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

            self.drain_deferred_v8_runtime_drops_if_idle();
            let job = queue.recv_blocking()?;
            self.admit_job(queue, job);
        }
    }
}

impl<D: CooperativeBackendDriver> WorkerLoop for CooperativeWorkerLoop<D>
where
    D::Slot: CooperativeBackendSlot,
{
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
                Ok(CooperativeRuntimeSlotPoll::ResponseReady) => {
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
                            &job.host,
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
                    self.drain_deferred_v8_runtime_drops_if_idle();
                    queue.complete_job(job, result, ready_jobs);
                }
                Err(error) => {
                    self.finish_slot_with_result(&queue, slot_id, invocation, Err(error), true);
                }
            }
        }

        if shutdown.is_cancelled() {
            self.drain_cancelled_slots(&queue);
            self.drain_pending_admissions(&queue);
        }
        self.driver.clear_retained();
    }
}
