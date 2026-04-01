use std::sync::Arc;

use neovex_core::Timestamp;
use neovex_storage::{FaultOccurrence, ManualClock, ScriptedFaultInjector};

pub struct DeterministicHarness {
    pub clock: Arc<ManualClock>,
    pub faults: Arc<ScriptedFaultInjector>,
}

impl DeterministicHarness {
    pub fn new(
        start: Timestamp,
        scheduled_faults: impl IntoIterator<Item = FaultOccurrence>,
    ) -> Self {
        Self {
            clock: Arc::new(ManualClock::new(start)),
            faults: Arc::new(ScriptedFaultInjector::new(scheduled_faults)),
        }
    }
}
