use std::num::NonZeroU64;
use std::sync::Arc;

use nimbus_core::{Result, Timestamp};

use super::clocks::ManualClock;
use super::coordination::{ScenarioMetadata, ScenarioSignal, ScenarioSignalKind, SignalRegistry};
use super::faults::{
    FaultInjector, FaultOccurrence, FaultPoint, ScriptedFaultInjector, SeededFaultInjector,
};

pub struct DeterministicHarness {
    metadata: ScenarioMetadata,
    clock: Arc<ManualClock>,
    fault_injector: Arc<dyn FaultInjector>,
    cancellations: Arc<SignalRegistry>,
    disconnects: Arc<SignalRegistry>,
    restarts: Arc<SignalRegistry>,
}

impl Clone for DeterministicHarness {
    fn clone(&self) -> Self {
        Self {
            metadata: self.metadata.clone(),
            clock: Arc::clone(&self.clock),
            fault_injector: Arc::clone(&self.fault_injector),
            cancellations: Arc::clone(&self.cancellations),
            disconnects: Arc::clone(&self.disconnects),
            restarts: Arc::clone(&self.restarts),
        }
    }
}

impl DeterministicHarness {
    pub fn new(
        start: Timestamp,
        scheduled_faults: impl IntoIterator<Item = FaultOccurrence>,
    ) -> Self {
        Self::scripted("unnamed-scenario", 0, start, scheduled_faults)
    }

    pub fn scenario(name: impl Into<String>, seed: u64, start: Timestamp) -> Self {
        Self::scripted(name, seed, start, [])
    }

    pub fn scripted(
        name: impl Into<String>,
        seed: u64,
        start: Timestamp,
        scheduled_faults: impl IntoIterator<Item = FaultOccurrence>,
    ) -> Self {
        Self::with_fault_injector(
            ScenarioMetadata::new(name, seed),
            Arc::new(ManualClock::new(start)),
            Arc::new(ScriptedFaultInjector::new(scheduled_faults)),
        )
    }

    pub fn seeded(
        name: impl Into<String>,
        seed: u64,
        start: Timestamp,
        one_in: NonZeroU64,
    ) -> Self {
        Self::with_fault_injector(
            ScenarioMetadata::new(name, seed),
            Arc::new(ManualClock::new(start)),
            Arc::new(SeededFaultInjector::new(seed, one_in)),
        )
    }

    pub fn with_fault_injector(
        metadata: ScenarioMetadata,
        clock: Arc<ManualClock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Self {
        Self {
            metadata,
            clock,
            fault_injector,
            cancellations: Arc::new(SignalRegistry::new(ScenarioSignalKind::Cancellation)),
            disconnects: Arc::new(SignalRegistry::new(ScenarioSignalKind::Disconnect)),
            restarts: Arc::new(SignalRegistry::new(ScenarioSignalKind::Restart)),
        }
    }

    pub fn metadata(&self) -> &ScenarioMetadata {
        &self.metadata
    }

    pub fn name(&self) -> &str {
        self.metadata.name()
    }

    pub fn seed(&self) -> u64 {
        self.metadata.seed()
    }

    pub fn describe(&self) -> String {
        self.metadata.describe()
    }

    pub fn clock(&self) -> Arc<ManualClock> {
        Arc::clone(&self.clock)
    }

    pub fn fault_injector(&self) -> Arc<dyn FaultInjector> {
        Arc::clone(&self.fault_injector)
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.fault_injector.check(point)
    }

    pub fn cancellation(&self, name: &str) -> ScenarioSignal {
        self.cancellations.signal(name)
    }

    pub fn disconnect(&self, name: &str) -> ScenarioSignal {
        self.disconnects.signal(name)
    }

    pub fn restart(&self, name: &str) -> ScenarioSignal {
        self.restarts.signal(name)
    }
}
