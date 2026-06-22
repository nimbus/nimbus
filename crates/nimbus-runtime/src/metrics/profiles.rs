use std::sync::atomic::AtomicU64;
use std::time::Duration;

use serde::Serialize;

use crate::limits::RuntimeProfile;

use super::{DIAGNOSTIC_COUNTER_ORDERING, duration_to_nanos};

#[derive(Debug, Default)]
pub(super) struct RuntimeProfileTelemetryRegistry {
    web_lean: RuntimeProfileCounters,
    node_full: RuntimeProfileCounters,
    unprofiled: RuntimeProfileCounters,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RuntimeProfileTelemetrySnapshot {
    pub web_lean: RuntimeProfileCountersSnapshot,
    pub node_full: RuntimeProfileCountersSnapshot,
    pub unprofiled: RuntimeProfileCountersSnapshot,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RuntimeProfileCountersSnapshot {
    pub started_invocations: u64,
    pub completed_invocations: u64,
    pub queue_wait_nanos_total: u64,
    pub execution_nanos_total: u64,
    pub runtime_pool_hits: u64,
    pub runtime_pool_misses: u64,
    pub runtime_pool_replacements: u64,
}

impl RuntimeProfileTelemetryRegistry {
    pub(super) fn record_invocation_started(&self, profile: Option<RuntimeProfile>) {
        self.bucket(profile)
            .started_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_invocation_completed(&self, profile: Option<RuntimeProfile>) {
        self.bucket(profile)
            .completed_invocations
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_queue_wait(&self, profile: Option<RuntimeProfile>, duration: Duration) {
        self.bucket(profile)
            .queue_wait_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_execution(&self, profile: Option<RuntimeProfile>, duration: Duration) {
        self.bucket(profile)
            .execution_nanos_total
            .fetch_add(duration_to_nanos(duration), DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_runtime_pool_hit(&self, profile: Option<RuntimeProfile>) {
        self.bucket(profile)
            .runtime_pool_hits
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_runtime_pool_miss(&self, profile: Option<RuntimeProfile>) {
        self.bucket(profile)
            .runtime_pool_misses
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn record_runtime_pool_replacement(&self, profile: Option<RuntimeProfile>) {
        self.bucket(profile)
            .runtime_pool_replacements
            .fetch_add(1, DIAGNOSTIC_COUNTER_ORDERING);
    }

    pub(super) fn snapshot(&self) -> RuntimeProfileTelemetrySnapshot {
        RuntimeProfileTelemetrySnapshot {
            web_lean: self.web_lean.snapshot(),
            node_full: self.node_full.snapshot(),
            unprofiled: self.unprofiled.snapshot(),
        }
    }

    fn bucket(&self, profile: Option<RuntimeProfile>) -> &RuntimeProfileCounters {
        match profile {
            Some(RuntimeProfile::WebLean) => &self.web_lean,
            Some(RuntimeProfile::NodeFull) => &self.node_full,
            None => &self.unprofiled,
        }
    }
}

#[derive(Debug, Default)]
struct RuntimeProfileCounters {
    started_invocations: AtomicU64,
    completed_invocations: AtomicU64,
    queue_wait_nanos_total: AtomicU64,
    execution_nanos_total: AtomicU64,
    runtime_pool_hits: AtomicU64,
    runtime_pool_misses: AtomicU64,
    runtime_pool_replacements: AtomicU64,
}

impl RuntimeProfileCounters {
    fn snapshot(&self) -> RuntimeProfileCountersSnapshot {
        RuntimeProfileCountersSnapshot {
            started_invocations: self.started_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            completed_invocations: self.completed_invocations.load(DIAGNOSTIC_COUNTER_ORDERING),
            queue_wait_nanos_total: self
                .queue_wait_nanos_total
                .load(DIAGNOSTIC_COUNTER_ORDERING),
            execution_nanos_total: self.execution_nanos_total.load(DIAGNOSTIC_COUNTER_ORDERING),
            runtime_pool_hits: self.runtime_pool_hits.load(DIAGNOSTIC_COUNTER_ORDERING),
            runtime_pool_misses: self.runtime_pool_misses.load(DIAGNOSTIC_COUNTER_ORDERING),
            runtime_pool_replacements: self
                .runtime_pool_replacements
                .load(DIAGNOSTIC_COUNTER_ORDERING),
        }
    }
}
