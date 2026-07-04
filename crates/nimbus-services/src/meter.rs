//! CB10: connection metering — Active-CPU vs residency, as usage records.
//!
//! This is the accounting that proves the broker's economic story: a
//! Hibernated instance bills ~nothing while idle (its isolate is evicted
//! between frames — no warm memory), while a Resident instance holds a warm
//! isolate. Each per-frame invocation emits a [`UsageRecord`] carrying the
//! frame's **active-CPU** time and the **residency** class it ran under, so
//! the "idle billed ~nothing" claim is measured, and — per the plan's #1
//! must-do — a silent Resident fallback is never a silent invoice: the
//! residency and warm-hit are on every record.
//!
//! Honesty note (contrast with the EE3 task-time metric): a per-frame invoke
//! runs the handler to completion SYNCHRONOUSLY — load state, invoke, drain —
//! with no mid-frame I/O await, so timing the invoke IS the isolate's on-CPU
//! time for that frame, not wall-clock occupancy. `active_cpu` measures what
//! it is named for.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::broker::InstanceKey;
use crate::broker::Residency;

/// One per-frame usage record: active-CPU time and the residency class the
/// frame ran under, plus whether it reused a warm slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    pub key: InstanceKey,
    /// On-CPU time for this frame (the synchronous invoke duration).
    pub active_cpu: Duration,
    /// Residency the frame ran under (Hibernated = isolate freed after).
    pub residency: Residency,
    /// True if the frame reused a warm isolate rather than cold-starting.
    pub warm_hit: bool,
}

/// A sink for usage records. Billing / tenant-admission-audit plug their own
/// collector in here; the broker owns only the emission point.
pub trait FrameMeter: Send + Sync {
    fn record(&self, usage: UsageRecord);
}

/// Per-instance aggregate for a tenant's residency + active-CPU accounting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstanceUsage {
    pub frames: u64,
    pub active_cpu: Duration,
    pub warm_hits: u64,
    /// Frames that ran Resident (warm-memory-billed) vs Hibernated.
    pub resident_frames: u64,
    pub hibernated_frames: u64,
}

/// An in-memory metering sink with per-instance aggregates. Node-wide,
/// keyed by instance (the same identity the broker/fairness use); a real
/// billing collector implements [`FrameMeter`] instead.
pub struct CountingMeter {
    usage: Mutex<HashMap<InstanceKey, InstanceUsage>>,
}

impl CountingMeter {
    pub fn new() -> Self {
        Self {
            usage: Mutex::new(HashMap::new()),
        }
    }

    /// The aggregate for `key` (all-zero if never metered).
    pub fn usage_for(&self, key: &InstanceKey) -> InstanceUsage {
        self.usage
            .lock()
            .ok()
            .and_then(|u| u.get(key).cloned())
            .unwrap_or_default()
    }

    /// Number of distinct instances metered.
    pub fn instance_count(&self) -> usize {
        self.usage.lock().map(|u| u.len()).unwrap_or(0)
    }
}

impl Default for CountingMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameMeter for CountingMeter {
    fn record(&self, usage: UsageRecord) {
        let Ok(mut map) = self.usage.lock() else {
            return; // fail-open: metering is accounting, never a request gate
        };
        let entry = map.entry(usage.key.clone()).or_default();
        entry.frames += 1;
        entry.active_cpu = entry.active_cpu.saturating_add(usage.active_cpu);
        if usage.warm_hit {
            entry.warm_hits += 1;
        }
        match usage.residency {
            Residency::Resident => entry.resident_frames += 1,
            Residency::Hibernated => entry.hibernated_frames += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;
    use std::sync::Arc;

    fn key(instance: &str) -> InstanceKey {
        InstanceKey::new(TenantId::new("tenant-a").unwrap(), "chat", instance)
    }

    #[test]
    fn counting_meter_aggregates_per_instance_by_residency() {
        let meter = CountingMeter::new();
        let a = key("room-1");
        let b = key("room-2");

        meter.record(UsageRecord {
            key: a.clone(),
            active_cpu: Duration::from_micros(100),
            residency: Residency::Hibernated,
            warm_hit: false,
        });
        meter.record(UsageRecord {
            key: a.clone(),
            active_cpu: Duration::from_micros(50),
            residency: Residency::Resident,
            warm_hit: true,
        });
        meter.record(UsageRecord {
            key: b.clone(),
            active_cpu: Duration::from_micros(10),
            residency: Residency::Resident,
            warm_hit: false,
        });

        let ua = meter.usage_for(&a);
        assert_eq!(ua.frames, 2);
        assert_eq!(ua.active_cpu, Duration::from_micros(150));
        assert_eq!(ua.warm_hits, 1);
        assert_eq!(ua.hibernated_frames, 1);
        assert_eq!(ua.resident_frames, 1);

        assert_eq!(
            meter.usage_for(&b).resident_frames,
            1,
            "one instance's usage never counts on another's"
        );
        assert_eq!(meter.instance_count(), 2);
    }

    #[test]
    fn meter_is_shareable_as_a_frame_meter_trait_object() {
        let meter: Arc<dyn FrameMeter> = Arc::new(CountingMeter::new());
        meter.record(UsageRecord {
            key: key("room-1"),
            active_cpu: Duration::from_micros(1),
            residency: Residency::Hibernated,
            warm_hit: false,
        });
    }
}
