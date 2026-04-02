use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use neovex_core::{
    Cursor, Error, Filter, FilterOp, OrderBy, OrderDirection, PaginatedQuery, Query, Result,
    TableName, Timestamp,
};
use serde_json::{Map, Value, json};
use tokio::sync::Notify;

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

pub struct ManualClock {
    now_ms: Mutex<u64>,
}

impl ManualClock {
    pub fn new(now: Timestamp) -> Self {
        Self {
            now_ms: Mutex::new(now.0),
        }
    }

    pub fn set(&self, now: Timestamp) {
        *self
            .now_ms
            .lock()
            .expect("manual clock lock should not be poisoned") = now.0;
    }

    pub fn advance(&self, duration: Duration) -> Timestamp {
        self.advance_ms(duration.as_millis().try_into().unwrap_or(u64::MAX))
    }

    pub fn advance_ms(&self, millis: u64) -> Timestamp {
        let mut now = self
            .now_ms
            .lock()
            .expect("manual clock lock should not be poisoned");
        *now = now.saturating_add(millis);
        Timestamp(*now)
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(
            *self
                .now_ms
                .lock()
                .expect("manual clock lock should not be poisoned"),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FaultPoint {
    StorageCommitBeforeVisibility = 1,
    StorageCommitAfterVisibilityBeforeReturn = 2,
    JournalAppendBeforeDurableFlush = 3,
    JournalFlushBeforeVisibility = 4,
    CheckpointPublishBeforeManifestUpdate = 5,
    CompactionStartBeforePublish = 6,
    JournalDurableAppendBeforeApply = 7,
}

impl FaultPoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StorageCommitBeforeVisibility => "storage_commit_before_visibility",
            Self::StorageCommitAfterVisibilityBeforeReturn => {
                "storage_commit_after_visibility_before_return"
            }
            Self::JournalAppendBeforeDurableFlush => "journal_append_before_durable_flush",
            Self::JournalFlushBeforeVisibility => "journal_flush_before_visibility",
            Self::CheckpointPublishBeforeManifestUpdate => {
                "checkpoint_publish_before_manifest_update"
            }
            Self::CompactionStartBeforePublish => "compaction_start_before_publish",
            Self::JournalDurableAppendBeforeApply => "journal_durable_append_before_apply",
        }
    }
}

pub trait FaultInjector: Send + Sync {
    fn check(&self, point: FaultPoint) -> Result<()>;
}

#[derive(Default)]
pub struct NoopFaultInjector;

impl FaultInjector for NoopFaultInjector {
    fn check(&self, _point: FaultPoint) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaultOccurrence {
    pub point: FaultPoint,
    pub visit: u64,
}

#[derive(Default)]
struct FaultState {
    visits: HashMap<FaultPoint, u64>,
}

pub struct ScriptedFaultInjector {
    scheduled: HashSet<FaultOccurrence>,
    state: Mutex<FaultState>,
}

impl ScriptedFaultInjector {
    pub fn new(scheduled: impl IntoIterator<Item = FaultOccurrence>) -> Self {
        Self {
            scheduled: scheduled.into_iter().collect(),
            state: Mutex::new(FaultState::default()),
        }
    }
}

impl FaultInjector for ScriptedFaultInjector {
    fn check(&self, point: FaultPoint) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("scripted fault injector lock should not be poisoned");
        let visit = state.visits.entry(point).or_insert(0);
        *visit = visit.saturating_add(1);
        if self.scheduled.contains(&FaultOccurrence {
            point,
            visit: *visit,
        }) {
            return Err(injected_fault(point, *visit));
        }
        Ok(())
    }
}

pub struct SeededFaultInjector {
    seed: u64,
    one_in: NonZeroU64,
    state: Mutex<FaultState>,
}

impl SeededFaultInjector {
    pub fn new(seed: u64, one_in: NonZeroU64) -> Self {
        Self {
            seed,
            one_in,
            state: Mutex::new(FaultState::default()),
        }
    }
}

impl FaultInjector for SeededFaultInjector {
    fn check(&self, point: FaultPoint) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("seeded fault injector lock should not be poisoned");
        let visit = state.visits.entry(point).or_insert(0);
        *visit = visit.saturating_add(1);
        let draw = splitmix64(self.seed ^ ((*visit).rotate_left(17)) ^ ((point as u64) << 48));
        if draw.is_multiple_of(self.one_in.get()) {
            return Err(injected_fault(point, *visit));
        }
        Ok(())
    }
}

fn injected_fault(point: FaultPoint, visit: u64) -> Error {
    Error::Internal(format!(
        "injected fault at {} on visit {}",
        point.as_str(),
        visit
    ))
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

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

struct SignalRegistry {
    kind: ScenarioSignalKind,
    signals: Mutex<BTreeMap<String, Arc<ScenarioSignalState>>>,
}

impl SignalRegistry {
    fn new(kind: ScenarioSignalKind) -> Self {
        Self {
            kind,
            signals: Mutex::new(BTreeMap::new()),
        }
    }

    fn signal(&self, name: &str) -> ScenarioSignal {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskRecord {
    pub title: String,
    pub status: String,
    pub rank: i64,
}

impl GeneratedTaskRecord {
    fn generated(seed: u64, slot: u32, step: usize, draw: u64) -> Self {
        let status = match draw % 3 {
            0 => "todo",
            1 => "done",
            _ => "in_progress",
        };
        Self {
            title: format!("seed-{seed}-slot-{slot}-step-{step}"),
            status: status.to_string(),
            rank: ((step as i64) * 32) + i64::from(slot),
        }
    }

    pub fn fields(&self) -> Map<String, Value> {
        Map::from_iter([
            ("title".to_string(), json!(self.title)),
            ("status".to_string(), json!(self.status)),
            ("rank".to_string(), json!(self.rank)),
        ])
    }

    pub fn from_json(value: &Value) -> Self {
        let object = value
            .as_object()
            .expect("generated task json should be an object");
        Self {
            title: object
                .get("title")
                .and_then(Value::as_str)
                .expect("generated task title should be present")
                .to_string(),
            status: object
                .get("status")
                .and_then(Value::as_str)
                .expect("generated task status should be present")
                .to_string(),
            rank: object
                .get("rank")
                .and_then(Value::as_i64)
                .expect("generated task rank should be present"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedTaskHistoryStep {
    Insert {
        slot: u32,
        record: GeneratedTaskRecord,
    },
    Update {
        slot: u32,
        record: GeneratedTaskRecord,
    },
    Delete {
        slot: u32,
    },
}

impl GeneratedTaskHistoryStep {
    pub fn slot(&self) -> u32 {
        match self {
            Self::Insert { slot, .. } | Self::Update { slot, .. } | Self::Delete { slot } => *slot,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Insert { slot, record } => format!(
                "insert(slot={slot}, title={}, status={}, rank={})",
                record.title, record.status, record.rank
            ),
            Self::Update { slot, record } => format!(
                "update(slot={slot}, title={}, status={}, rank={})",
                record.title, record.status, record.rank
            ),
            Self::Delete { slot } => format!("delete(slot={slot})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskPageExpectation {
    pub data: Vec<GeneratedTaskRecord>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskHistoryModel {
    records_by_slot: BTreeMap<u32, GeneratedTaskRecord>,
    query_status: String,
    page_size: usize,
}

impl GeneratedTaskHistoryModel {
    pub fn final_documents(&self) -> Vec<GeneratedTaskRecord> {
        let mut records = self.records_by_slot.values().cloned().collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.title
                .cmp(&right.title)
                .then_with(|| left.rank.cmp(&right.rank))
                .then_with(|| left.status.cmp(&right.status))
        });
        records
    }

    pub fn query_result(&self) -> Vec<GeneratedTaskRecord> {
        let mut records = self
            .records_by_slot
            .values()
            .filter(|record| record.status == self.query_status)
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.title
                .cmp(&right.title)
                .then_with(|| left.rank.cmp(&right.rank))
                .then_with(|| left.status.cmp(&right.status))
        });
        records
    }

    pub fn first_page(&self) -> GeneratedTaskPageExpectation {
        self.page_from_offset(0)
    }

    pub fn second_page(&self) -> GeneratedTaskPageExpectation {
        self.page_from_offset(self.page_size)
    }

    fn page_from_offset(&self, offset: usize) -> GeneratedTaskPageExpectation {
        let query = self.query_result();
        let remaining = query.len().saturating_sub(offset);
        let data = query
            .into_iter()
            .skip(offset)
            .take(self.page_size)
            .collect::<Vec<_>>();
        GeneratedTaskPageExpectation {
            data,
            has_more: remaining > self.page_size,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskHistory {
    metadata: ScenarioMetadata,
    steps: Vec<GeneratedTaskHistoryStep>,
    table: String,
    query_status: String,
    page_size: usize,
}

impl GeneratedTaskHistory {
    pub fn seeded(name: impl Into<String>, seed: u64, step_count: usize) -> Self {
        let mut live_slots = Vec::new();
        let mut next_slot = 0_u32;
        let mut steps = Vec::with_capacity(step_count);

        for step in 0..step_count {
            let draw = splitmix64(seed ^ ((step as u64) << 32) ^ 0xa5a5_a5a5_a5a5_a5a5);
            let should_insert = live_slots.is_empty() || draw % 100 < 45;
            if should_insert {
                let slot = next_slot;
                next_slot = next_slot.saturating_add(1);
                live_slots.push(slot);
                steps.push(GeneratedTaskHistoryStep::Insert {
                    slot,
                    record: GeneratedTaskRecord::generated(seed, slot, step, draw),
                });
                continue;
            }

            let slot_index = (draw as usize) % live_slots.len();
            let slot = live_slots[slot_index];
            if draw % 100 < 80 {
                steps.push(GeneratedTaskHistoryStep::Update {
                    slot,
                    record: GeneratedTaskRecord::generated(seed, slot, step, draw ^ 0x5a5a_5a5a),
                });
            } else {
                live_slots.swap_remove(slot_index);
                steps.push(GeneratedTaskHistoryStep::Delete { slot });
            }
        }

        let query_status = dominant_generated_task_status(&steps);

        Self {
            metadata: ScenarioMetadata::new(name, seed),
            steps,
            table: "tasks".to_string(),
            query_status,
            page_size: 2,
        }
    }

    pub fn metadata(&self) -> &ScenarioMetadata {
        &self.metadata
    }

    pub fn describe(&self) -> String {
        format!(
            "{} with {} generated steps",
            self.metadata.describe(),
            self.steps.len()
        )
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn query_status(&self) -> &str {
        &self.query_status
    }

    pub fn page_size(&self) -> usize {
        self.page_size
    }

    pub fn steps(&self) -> &[GeneratedTaskHistoryStep] {
        &self.steps
    }

    pub fn ordered_query(&self) -> Query {
        Query {
            table: TableName::new(self.table()).expect("generated task table should be valid"),
            filters: vec![Filter {
                field: "status".to_string(),
                op: FilterOp::Eq,
                value: json!(self.query_status()),
            }],
            order: Some(OrderBy {
                field: "title".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        }
    }

    pub fn paginated_query(&self, after: Option<Cursor>) -> PaginatedQuery {
        PaginatedQuery {
            query: self.ordered_query(),
            page_size: self.page_size(),
            after,
        }
    }

    pub fn step_description(&self, step_index: usize) -> String {
        self.steps
            .get(step_index)
            .map(GeneratedTaskHistoryStep::describe)
            .unwrap_or_else(|| format!("unknown-step-{step_index}"))
    }

    pub fn failure_context(&self, invariant: &str, step_index: Option<usize>) -> String {
        match step_index {
            Some(step_index) => format!(
                "{invariant}; {}; step {step_index}: {}",
                self.describe(),
                self.step_description(step_index)
            ),
            None => format!("{invariant}; {}", self.describe()),
        }
    }

    pub fn model(&self) -> GeneratedTaskHistoryModel {
        self.model_through(self.steps.len())
    }

    pub fn model_through(&self, step_count: usize) -> GeneratedTaskHistoryModel {
        let mut records_by_slot = BTreeMap::new();
        for step in self.steps.iter().take(step_count) {
            match step {
                GeneratedTaskHistoryStep::Insert { slot, record }
                | GeneratedTaskHistoryStep::Update { slot, record } => {
                    records_by_slot.insert(*slot, record.clone());
                }
                GeneratedTaskHistoryStep::Delete { slot } => {
                    records_by_slot.remove(slot);
                }
            }
        }
        GeneratedTaskHistoryModel {
            records_by_slot,
            query_status: self.query_status.clone(),
            page_size: self.page_size,
        }
    }

    pub fn model_after_step(&self, step_index: usize) -> GeneratedTaskHistoryModel {
        self.model_through(step_index.saturating_add(1))
    }
}

pub const VERIFICATION_CASE_FILTER_ENV: &str = "NEOVEX_VERIFY_CASE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationHarnessMode {
    PullRequest,
    Nightly,
}

impl VerificationHarnessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PullRequest => "pr",
            Self::Nightly => "nightly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedTaskHistorySeedCase {
    pub id: &'static str,
    pub seed: u64,
    pub step_count: usize,
    pub regression: bool,
    pub description: &'static str,
    pub mode: VerificationHarnessMode,
}

impl GeneratedTaskHistorySeedCase {
    const fn new(
        id: &'static str,
        seed: u64,
        step_count: usize,
        regression: bool,
        description: &'static str,
        mode: VerificationHarnessMode,
    ) -> Self {
        Self {
            id,
            seed,
            step_count,
            regression,
            description,
            mode,
        }
    }

    pub fn history(self, surface: &str) -> GeneratedTaskHistory {
        GeneratedTaskHistory::seeded(format!("{surface}-{}", self.id), self.seed, self.step_count)
    }

    pub fn repro_command(self, package: &str, test_name: &str) -> String {
        format!(
            "{VERIFICATION_CASE_FILTER_ENV}={} cargo test -p {package} {test_name} -- --ignored --nocapture",
            self.id
        )
    }

    pub fn failure_context(self, package: &str, test_name: &str, invariant: &str) -> String {
        format!(
            "{invariant}; case {} [{} mode, seed {}, steps {}, regression={}]: {}. Repro: {}",
            self.id,
            self.mode.as_str(),
            self.seed,
            self.step_count,
            self.regression,
            self.description,
            self.repro_command(package, test_name)
        )
    }
}

const PR_GENERATED_TASK_HISTORY_CASES: [GeneratedTaskHistorySeedCase; 2] = [
    GeneratedTaskHistorySeedCase::new(
        "smoke-storage-baseline-31",
        31,
        24,
        false,
        "baseline smoke seed for cross-surface generated-history replay",
        VerificationHarnessMode::PullRequest,
    ),
    GeneratedTaskHistorySeedCase::new(
        "regression-two-page-pagination-41",
        41,
        48,
        true,
        "regression seed that guarantees multi-page query and pagination coverage",
        VerificationHarnessMode::PullRequest,
    ),
];

const NIGHTLY_GENERATED_TASK_HISTORY_CASES: [GeneratedTaskHistorySeedCase; 4] = [
    GeneratedTaskHistorySeedCase::new(
        "smoke-storage-baseline-31",
        31,
        24,
        false,
        "baseline smoke seed for cross-surface generated-history replay",
        VerificationHarnessMode::Nightly,
    ),
    GeneratedTaskHistorySeedCase::new(
        "regression-two-page-pagination-41",
        41,
        48,
        true,
        "regression seed that guarantees multi-page query and pagination coverage",
        VerificationHarnessMode::Nightly,
    ),
    GeneratedTaskHistorySeedCase::new(
        "adversarial-dense-updates-83",
        83,
        72,
        false,
        "heavier nightly seed with dense update churn before deletes",
        VerificationHarnessMode::Nightly,
    ),
    GeneratedTaskHistorySeedCase::new(
        "adversarial-long-tail-131",
        131,
        96,
        false,
        "longer nightly seed that stretches pagination and final-state convergence",
        VerificationHarnessMode::Nightly,
    ),
];

pub fn generated_task_history_seed_corpus(
    mode: VerificationHarnessMode,
) -> &'static [GeneratedTaskHistorySeedCase] {
    match mode {
        VerificationHarnessMode::PullRequest => &PR_GENERATED_TASK_HISTORY_CASES,
        VerificationHarnessMode::Nightly => &NIGHTLY_GENERATED_TASK_HISTORY_CASES,
    }
}

pub fn filter_generated_task_history_seed_corpus(
    cases: &[GeneratedTaskHistorySeedCase],
    filter: Option<&str>,
) -> Result<Vec<GeneratedTaskHistorySeedCase>> {
    match filter {
        Some(filter) => {
            let selected = cases
                .iter()
                .copied()
                .filter(|case| case.id == filter)
                .collect::<Vec<_>>();
            if selected.is_empty() {
                return Err(Error::InvalidInput(format!(
                    "unknown verification harness case `{filter}`"
                )));
            }
            Ok(selected)
        }
        None => Ok(cases.to_vec()),
    }
}

pub fn selected_generated_task_history_seed_corpus(
    mode: VerificationHarnessMode,
) -> Result<Vec<GeneratedTaskHistorySeedCase>> {
    let filter = match std::env::var(VERIFICATION_CASE_FILTER_ENV) {
        Ok(filter) => Some(filter),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(Error::InvalidInput(format!(
                "failed to read {VERIFICATION_CASE_FILTER_ENV}: {error}"
            )));
        }
    };
    filter_generated_task_history_seed_corpus(
        generated_task_history_seed_corpus(mode),
        filter.as_deref(),
    )
}

fn dominant_generated_task_status(steps: &[GeneratedTaskHistoryStep]) -> String {
    let mut records_by_slot = BTreeMap::new();
    for step in steps {
        match step {
            GeneratedTaskHistoryStep::Insert { slot, record }
            | GeneratedTaskHistoryStep::Update { slot, record } => {
                records_by_slot.insert(*slot, record);
            }
            GeneratedTaskHistoryStep::Delete { slot } => {
                records_by_slot.remove(slot);
            }
        }
    }

    let mut counts = BTreeMap::from([
        ("done".to_string(), 0_usize),
        ("in_progress".to_string(), 0_usize),
        ("todo".to_string(), 0_usize),
    ]);
    for record in records_by_slot.values() {
        *counts.entry(record.status.clone()).or_insert(0) += 1;
    }

    counts
        .into_iter()
        .max_by(|(left_status, left_count), (right_status, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_status.cmp(left_status))
        })
        .map(|(status, _)| status)
        .unwrap_or_else(|| "todo".to_string())
}

pub fn replay_generated_task_history<Id, E, Insert, Update, Delete>(
    history: &GeneratedTaskHistory,
    mut insert: Insert,
    mut update: Update,
    mut delete: Delete,
) -> std::result::Result<BTreeMap<u32, Id>, E>
where
    Insert: FnMut(u32, &GeneratedTaskRecord) -> std::result::Result<Id, E>,
    Update: FnMut(u32, &Id, &GeneratedTaskRecord) -> std::result::Result<(), E>,
    Delete: FnMut(u32, &Id) -> std::result::Result<(), E>,
{
    let mut ids_by_slot = BTreeMap::new();
    for (step_index, step) in history.steps().iter().enumerate() {
        match step {
            GeneratedTaskHistoryStep::Insert { slot, record } => {
                let id = insert(*slot, record)?;
                ids_by_slot.insert(*slot, id);
            }
            GeneratedTaskHistoryStep::Update { slot, record } => {
                let id = ids_by_slot.get(slot).unwrap_or_else(|| {
                    panic!(
                        "{}",
                        history.failure_context(
                            "missing slot binding during update replay",
                            Some(step_index)
                        )
                    )
                });
                update(*slot, id, record)?;
            }
            GeneratedTaskHistoryStep::Delete { slot } => {
                let id = ids_by_slot.get(slot).unwrap_or_else(|| {
                    panic!(
                        "{}",
                        history.failure_context(
                            "missing slot binding during delete replay",
                            Some(step_index)
                        )
                    )
                });
                delete(*slot, id)?;
                ids_by_slot.remove(slot);
            }
        }
    }
    Ok(ids_by_slot)
}

pub async fn replay_generated_task_history_async<
    Id,
    E,
    Insert,
    InsertFuture,
    Update,
    UpdateFuture,
    Delete,
    DeleteFuture,
>(
    history: &GeneratedTaskHistory,
    mut insert: Insert,
    mut update: Update,
    mut delete: Delete,
) -> std::result::Result<BTreeMap<u32, Id>, E>
where
    Id: Clone,
    Insert: FnMut(u32, &GeneratedTaskRecord) -> InsertFuture,
    InsertFuture: Future<Output = std::result::Result<Id, E>>,
    Update: FnMut(u32, Id, &GeneratedTaskRecord) -> UpdateFuture,
    UpdateFuture: Future<Output = std::result::Result<(), E>>,
    Delete: FnMut(u32, Id) -> DeleteFuture,
    DeleteFuture: Future<Output = std::result::Result<(), E>>,
{
    let mut ids_by_slot = BTreeMap::new();
    for (step_index, step) in history.steps().iter().enumerate() {
        match step {
            GeneratedTaskHistoryStep::Insert { slot, record } => {
                let id = insert(*slot, record).await?;
                ids_by_slot.insert(*slot, id);
            }
            GeneratedTaskHistoryStep::Update { slot, record } => {
                let id = ids_by_slot.get(slot).cloned().unwrap_or_else(|| {
                    panic!(
                        "{}",
                        history.failure_context(
                            "missing slot binding during async update replay",
                            Some(step_index)
                        )
                    )
                });
                update(*slot, id, record).await?;
            }
            GeneratedTaskHistoryStep::Delete { slot } => {
                let id = ids_by_slot.get(slot).cloned().unwrap_or_else(|| {
                    panic!(
                        "{}",
                        history.failure_context(
                            "missing slot binding during async delete replay",
                            Some(step_index)
                        )
                    )
                });
                delete(*slot, id).await?;
                ids_by_slot.remove(slot);
            }
        }
    }
    Ok(ids_by_slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scenario_signal_wait_returns_after_trigger_even_if_triggered_first() {
        let harness = DeterministicHarness::scenario("signal-wait", 7, Timestamp(1_000));
        let signal = harness.cancellation("client-drop");
        signal.trigger();
        signal.wait().await;
        assert!(signal.is_triggered());
        assert_eq!(signal.describe(), "cancellation signal 'client-drop'");
    }

    #[test]
    fn harness_reuses_named_signals_and_preserves_metadata() {
        let harness = DeterministicHarness::scenario("metadata", 42, Timestamp(5_000));
        let left = harness.disconnect("socket-1");
        let right = harness.disconnect("socket-1");

        assert_eq!(harness.name(), "metadata");
        assert_eq!(harness.seed(), 42);
        assert_eq!(harness.describe(), "metadata (seed 42)");
        assert_eq!(left.name(), right.name());
        assert_eq!(left.kind(), ScenarioSignalKind::Disconnect);
        assert!(!left.is_triggered());

        left.trigger();
        assert!(right.is_triggered());
    }

    #[test]
    fn seeded_harness_replays_the_same_fault_schedule_for_the_same_seed() {
        let left = DeterministicHarness::seeded(
            "left",
            11,
            Timestamp(10_000),
            NonZeroU64::new(3).expect("period should be non-zero"),
        );
        let right = DeterministicHarness::seeded(
            "right",
            11,
            Timestamp(10_000),
            NonZeroU64::new(3).expect("period should be non-zero"),
        );

        let left_results = [
            FaultPoint::StorageCommitBeforeVisibility,
            FaultPoint::JournalAppendBeforeDurableFlush,
            FaultPoint::StorageCommitBeforeVisibility,
            FaultPoint::CheckpointPublishBeforeManifestUpdate,
            FaultPoint::StorageCommitBeforeVisibility,
            FaultPoint::CompactionStartBeforePublish,
        ]
        .into_iter()
        .map(|point| left.check_fault(point).is_err())
        .collect::<Vec<_>>();
        let right_results = [
            FaultPoint::StorageCommitBeforeVisibility,
            FaultPoint::JournalAppendBeforeDurableFlush,
            FaultPoint::StorageCommitBeforeVisibility,
            FaultPoint::CheckpointPublishBeforeManifestUpdate,
            FaultPoint::StorageCommitBeforeVisibility,
            FaultPoint::CompactionStartBeforePublish,
        ]
        .into_iter()
        .map(|point| right.check_fault(point).is_err())
        .collect::<Vec<_>>();

        assert_eq!(left_results, right_results);
    }

    #[test]
    fn generated_task_history_is_reproducible_for_the_same_seed() {
        let left = GeneratedTaskHistory::seeded("left", 23, 12);
        let right = GeneratedTaskHistory::seeded("right", 23, 12);

        assert_eq!(left.steps(), right.steps());
        assert_eq!(left.model(), right.model());
        assert_eq!(left.query_status(), right.query_status());
        assert!(matches!(
            left.query_status(),
            "todo" | "done" | "in_progress"
        ));
        assert_eq!(left.page_size(), 2);
    }

    #[test]
    fn scripted_restart_schedule_is_reproducible_for_the_same_seed() {
        let left = ScriptedRestartSchedule::seeded(
            "left",
            91,
            12,
            3,
            &[
                RestartBoundary::DurableAppendBeforeApply,
                RestartBoundary::SchedulerClaim,
                RestartBoundary::SchedulerCompletion,
            ],
        );
        let right = ScriptedRestartSchedule::seeded(
            "right",
            91,
            12,
            3,
            &[
                RestartBoundary::DurableAppendBeforeApply,
                RestartBoundary::SchedulerClaim,
                RestartBoundary::SchedulerCompletion,
            ],
        );

        assert_eq!(left.restart_points(), right.restart_points());
        assert_eq!(left.restart_points().len(), 3);
        assert!(left.describe().contains("seed 91"));
    }

    #[tokio::test]
    async fn generated_task_history_async_replay_preserves_slot_bindings() {
        let history = GeneratedTaskHistory::seeded("async-runner", 23, 12);
        let remaining = replay_generated_task_history_async(
            &history,
            |slot, _record| async move {
                Ok::<String, std::convert::Infallible>(format!("slot-{slot}"))
            },
            |_slot, _id, _record| async move { Ok::<(), std::convert::Infallible>(()) },
            |_slot, _id| async move { Ok::<(), std::convert::Infallible>(()) },
        )
        .await
        .expect("async replay should succeed");

        assert_eq!(remaining.len(), history.model().final_documents().len());
    }

    #[test]
    fn verification_harness_seed_corpus_has_explicit_pr_and_nightly_modes() {
        let pr = generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest);
        let nightly = generated_task_history_seed_corpus(VerificationHarnessMode::Nightly);

        assert_eq!(pr.len(), 2);
        assert!(
            pr.iter()
                .all(|case| case.mode == VerificationHarnessMode::PullRequest)
        );
        assert!(nightly.len() > pr.len());
        assert!(
            nightly
                .iter()
                .all(|case| case.mode == VerificationHarnessMode::Nightly)
        );
        assert!(pr.iter().all(|case| {
            nightly
                .iter()
                .any(|nightly_case| nightly_case.id == case.id)
        }));
        assert!(nightly.iter().any(|case| case.regression));
    }

    #[test]
    fn verification_harness_seed_corpus_can_filter_to_one_named_case() {
        let selected = filter_generated_task_history_seed_corpus(
            generated_task_history_seed_corpus(VerificationHarnessMode::Nightly),
            Some("regression-two-page-pagination-41"),
        )
        .expect("seed corpus filter should accept a named case");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "regression-two-page-pagination-41");
        assert!(selected[0].regression);
    }

    #[test]
    fn verification_harness_seed_case_formats_deterministic_repro_command() {
        let case = generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)[1];
        assert_eq!(
            case.repro_command(
                "neovex-engine",
                "verification_harness_pr_generated_history_seed_corpus_matches_model"
            ),
            "NEOVEX_VERIFY_CASE=regression-two-page-pagination-41 cargo test -p neovex-engine verification_harness_pr_generated_history_seed_corpus_matches_model -- --ignored --nocapture"
        );
    }
}
