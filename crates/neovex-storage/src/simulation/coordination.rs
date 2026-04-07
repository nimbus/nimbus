use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use super::seeding::splitmix64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioMetadata {
    name: String,
    seed: u64,
}

impl ScenarioMetadata {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            seed,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn describe(&self) -> String {
        format!("{} (seed {})", self.name, self.seed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioSignalKind {
    Cancellation,
    Disconnect,
    Restart,
}

impl ScenarioSignalKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cancellation => "cancellation",
            Self::Disconnect => "disconnect",
            Self::Restart => "restart",
        }
    }
}

struct ScenarioSignalState {
    name: String,
    kind: ScenarioSignalKind,
    triggered: AtomicBool,
    notify: Notify,
}

impl ScenarioSignalState {
    fn new(name: impl Into<String>, kind: ScenarioSignalKind) -> Self {
        Self {
            name: name.into(),
            kind,
            triggered: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }
}

#[derive(Clone)]
pub struct ScenarioSignal {
    state: Arc<ScenarioSignalState>,
}

impl ScenarioSignal {
    pub fn name(&self) -> &str {
        &self.state.name
    }

    pub fn kind(&self) -> ScenarioSignalKind {
        self.state.kind
    }

    pub fn describe(&self) -> String {
        format!("{} signal '{}'", self.kind().as_str(), self.name())
    }

    pub fn trigger(&self) {
        self.state.triggered.store(true, Ordering::Release);
        self.state.notify.notify_waiters();
    }

    pub fn is_triggered(&self) -> bool {
        self.state.triggered.load(Ordering::Acquire)
    }

    pub async fn wait(&self) {
        if self.is_triggered() {
            return;
        }

        loop {
            let notified = self.state.notify.notified();
            if self.is_triggered() {
                return;
            }
            notified.await;
            if self.is_triggered() {
                return;
            }
        }
    }
}

pub(crate) struct SignalRegistry {
    kind: ScenarioSignalKind,
    signals: Mutex<BTreeMap<String, Arc<ScenarioSignalState>>>,
}

impl SignalRegistry {
    pub(crate) fn new(kind: ScenarioSignalKind) -> Self {
        Self {
            kind,
            signals: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn signal(&self, name: &str) -> ScenarioSignal {
        let mut signals = self
            .signals
            .lock()
            .expect("scenario signal registry lock should not be poisoned");
        let state = signals
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(ScenarioSignalState::new(name, self.kind)))
            .clone();
        ScenarioSignal { state }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RestartBoundary {
    DurableAppendBeforeApply,
    CheckpointPublish,
    CompactionBoundary,
    SchedulerClaim,
    SchedulerCompletion,
}

impl RestartBoundary {
    fn as_str(self) -> &'static str {
        match self {
            Self::DurableAppendBeforeApply => "durable-append-before-apply",
            Self::CheckpointPublish => "checkpoint-publish",
            Self::CompactionBoundary => "compaction-boundary",
            Self::SchedulerClaim => "scheduler-claim",
            Self::SchedulerCompletion => "scheduler-completion",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RestartPoint {
    pub step_index: usize,
    pub boundary: RestartBoundary,
}

impl RestartPoint {
    pub fn new(step_index: usize, boundary: RestartBoundary) -> Self {
        Self {
            step_index,
            boundary,
        }
    }

    pub fn describe(self) -> String {
        format!(
            "restart after step {} at {}",
            self.step_index,
            self.boundary.as_str()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedRestartSchedule {
    metadata: ScenarioMetadata,
    restart_points: Vec<RestartPoint>,
}

impl ScriptedRestartSchedule {
    pub fn scripted(
        metadata: ScenarioMetadata,
        restart_points: impl IntoIterator<Item = RestartPoint>,
    ) -> Self {
        let mut restart_points = restart_points.into_iter().collect::<Vec<_>>();
        restart_points.sort_by_key(|point| point.step_index);
        Self {
            metadata,
            restart_points,
        }
    }

    pub fn seeded(
        name: impl Into<String>,
        seed: u64,
        step_count: usize,
        restart_count: usize,
        boundaries: &[RestartBoundary],
    ) -> Self {
        let metadata = ScenarioMetadata::new(name, seed);
        if step_count == 0 || restart_count == 0 || boundaries.is_empty() {
            return Self {
                metadata,
                restart_points: Vec::new(),
            };
        }

        let target_count = restart_count.min(step_count);
        let mut used_steps = BTreeSet::new();
        let mut restart_points = Vec::with_capacity(target_count);
        for index in 0..target_count {
            let boundary = boundaries[index % boundaries.len()];
            let draw = splitmix64(seed ^ ((index as u64) << 32) ^ ((boundary as u64) << 8));
            let mut step_index = (draw as usize) % step_count;
            while !used_steps.insert(step_index) {
                step_index = (step_index + 1) % step_count;
            }
            restart_points.push(RestartPoint::new(step_index, boundary));
        }
        restart_points.sort_by_key(|point| point.step_index);
        Self {
            metadata,
            restart_points,
        }
    }

    pub fn metadata(&self) -> &ScenarioMetadata {
        &self.metadata
    }

    pub fn restart_points(&self) -> &[RestartPoint] {
        &self.restart_points
    }

    pub fn restart_point_after_step(&self, step_index: usize) -> Option<RestartPoint> {
        self.restart_points
            .iter()
            .copied()
            .find(|point| point.step_index == step_index)
    }

    pub fn describe(&self) -> String {
        if self.restart_points.is_empty() {
            return format!("{} with no restart points", self.metadata.describe());
        }
        let points = self
            .restart_points
            .iter()
            .map(|point| point.describe())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} with {}", self.metadata.describe(), points)
    }

    pub fn failure_context(&self, invariant: &str, step_index: Option<usize>) -> String {
        match step_index {
            Some(step_index) => {
                let boundary = self
                    .restart_point_after_step(step_index)
                    .map(|point| point.boundary.as_str())
                    .unwrap_or("no-restart");
                format!(
                    "{invariant}; {}; step {step_index} at {boundary}",
                    self.describe()
                )
            }
            None => format!("{invariant}; {}", self.describe()),
        }
    }
}
