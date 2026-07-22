use std::fmt;

use serde::{Deserialize, Serialize};

pub const PPSC_MAX_STEPS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PpscBackend {
    Memory,
    Redb,
    Sqlite,
    Libsql,
    Postgres,
    Mysql,
}

impl PpscBackend {
    pub const ALL: [Self; 6] = [
        Self::Memory,
        Self::Redb,
        Self::Sqlite,
        Self::Libsql,
        Self::Postgres,
        Self::Mysql,
    ];

    pub const DURABLE: [Self; 5] = [
        Self::Redb,
        Self::Sqlite,
        Self::Libsql,
        Self::Postgres,
        Self::Mysql,
    ];

    pub const PROVIDERS: [Self; 3] = [Self::Libsql, Self::Postgres, Self::Mysql];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Redb => "redb",
            Self::Sqlite => "sqlite",
            Self::Libsql => "libsql",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }

    pub const fn capabilities(self) -> PpscBackendCapabilities {
        match self {
            Self::Memory => PpscBackendCapabilities {
                durable_reopen: false,
                provider_authority: false,
            },
            Self::Redb | Self::Sqlite => PpscBackendCapabilities {
                durable_reopen: true,
                provider_authority: false,
            },
            Self::Libsql | Self::Postgres | Self::Mysql => PpscBackendCapabilities {
                durable_reopen: true,
                provider_authority: true,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscBackendCapabilities {
    pub durable_reopen: bool,
    pub provider_authority: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PpscRoute {
    QueuedJournal,
    Direct,
    ExecutionUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PpscCommitOrder {
    QueuedDirectExecutionUnit,
    QueuedExecutionUnitDirect,
    DirectQueuedExecutionUnit,
    DirectExecutionUnitQueued,
    ExecutionUnitQueuedDirect,
    ExecutionUnitDirectQueued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PpscInjectedFault {
    AcknowledgementLoss,
    ProviderTransient,
    DurableBeforePublish,
    PublicationPredecessorHeld,
    PanicAfterDurable,
}

impl PpscInjectedFault {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AcknowledgementLoss => "acknowledgement-loss",
            Self::ProviderTransient => "provider-transient",
            Self::DurableBeforePublish => "durable-before-publish",
            Self::PublicationPredecessorHeld => "publication-predecessor-held",
            Self::PanicAfterDurable => "panic-after-durable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PpscExpectedOutcome {
    Observed,
    Committed,
    DefinitiveRollback,
    AmbiguousRecovered,
    Cancelled,
    Overloaded,
    Fenced,
    ProviderError,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PpscOperation {
    Mutation {
        tenant: String,
        route: PpscRoute,
        key: String,
        value: i64,
    },
    CommitPermutation {
        tenant: String,
        order: PpscCommitOrder,
        value_base: i64,
    },
    ZeroWriteExecutionUnit {
        tenant: String,
    },
    ConflictRetry {
        tenant: String,
        key: String,
        first: i64,
        second: i64,
    },
    SchemaSet {
        tenant: String,
        revision: u64,
    },
    SchemaDelete {
        tenant: String,
    },
    RestoreImport {
        tenant: String,
        archive: u64,
    },
    TriggerCursorAdvance {
        tenant: String,
        through: u64,
    },
    Schedule {
        tenant: String,
        job: u64,
    },
    ProjectionUpdate {
        tenant: String,
        revision: u64,
    },
    Replay {
        tenant: String,
        sequence: u64,
        identity: String,
    },
    ArmFault {
        tenant: String,
        fault: PpscInjectedFault,
    },
    ReleaseFault {
        tenant: String,
        fault: PpscInjectedFault,
    },
    CancelNext {
        tenant: String,
        route: PpscRoute,
    },
    ForceOverload {
        tenant: String,
    },
    AdvanceWallClock {
        millis: u64,
    },
    AdvanceMonotonicClock {
        millis: u64,
    },
    ExpireProviderLease {
        tenant: String,
    },
    ProviderTakeover {
        tenant: String,
    },
    AttemptStaleProviderWrite {
        tenant: String,
    },
    Crash,
    Reopen,
    Quiesce,
}

impl PpscOperation {
    pub fn tenant(&self) -> Option<&str> {
        match self {
            Self::Mutation { tenant, .. }
            | Self::CommitPermutation { tenant, .. }
            | Self::ZeroWriteExecutionUnit { tenant }
            | Self::ConflictRetry { tenant, .. }
            | Self::SchemaSet { tenant, .. }
            | Self::SchemaDelete { tenant }
            | Self::RestoreImport { tenant, .. }
            | Self::TriggerCursorAdvance { tenant, .. }
            | Self::Schedule { tenant, .. }
            | Self::ProjectionUpdate { tenant, .. }
            | Self::Replay { tenant, .. }
            | Self::ArmFault { tenant, .. }
            | Self::ReleaseFault { tenant, .. }
            | Self::CancelNext { tenant, .. }
            | Self::ForceOverload { tenant }
            | Self::ExpireProviderLease { tenant }
            | Self::ProviderTakeover { tenant }
            | Self::AttemptStaleProviderWrite { tenant } => Some(tenant),
            Self::AdvanceWallClock { .. }
            | Self::AdvanceMonotonicClock { .. }
            | Self::Crash
            | Self::Reopen
            | Self::Quiesce => None,
        }
    }

    const fn needs_durable_reopen(&self) -> bool {
        matches!(self, Self::Crash | Self::Reopen)
    }

    const fn needs_provider_authority(&self) -> bool {
        matches!(
            self,
            Self::ExpireProviderLease { .. }
                | Self::ProviderTakeover { .. }
                | Self::AttemptStaleProviderWrite { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscStep {
    pub operation: PpscOperation,
    pub expected: PpscExpectedOutcome,
}

impl PpscStep {
    pub fn new(operation: PpscOperation, expected: PpscExpectedOutcome) -> Self {
        Self {
            operation,
            expected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscScenario {
    pub name: String,
    pub seed: u64,
    pub steps: Vec<PpscStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpscScenarioError {
    message: String,
}

impl PpscScenarioError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PpscScenarioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PpscScenarioError {}

impl PpscScenario {
    pub fn new(
        name: impl Into<String>,
        seed: u64,
        steps: Vec<PpscStep>,
    ) -> Result<Self, PpscScenarioError> {
        if steps.is_empty() {
            return Err(PpscScenarioError::new(
                "PPSC scenario must contain at least one explicit step",
            ));
        }
        if steps.len() > PPSC_MAX_STEPS {
            return Err(PpscScenarioError::new(format!(
                "PPSC scenario contains {} steps; maximum is {PPSC_MAX_STEPS}",
                steps.len()
            )));
        }
        for (index, step) in steps.iter().enumerate() {
            if matches!(
                step.operation,
                PpscOperation::CancelNext {
                    route: PpscRoute::Direct,
                    ..
                }
            ) {
                return Err(PpscScenarioError::new(format!(
                    "PPSC scenario step {index} requests cancellation on the synchronous direct route; use queued-journal or execution-unit"
                )));
            }
        }
        Ok(Self {
            name: name.into(),
            seed,
            steps,
        })
    }

    pub fn seeded(seed: u64, step_count: usize) -> Result<Self, PpscScenarioError> {
        if step_count == 0 {
            return Err(PpscScenarioError::new(
                "PPSC seeded scenario must contain at least one step",
            ));
        }
        if step_count > PPSC_MAX_STEPS {
            return Err(PpscScenarioError::new(format!(
                "PPSC seeded scenario requested {step_count} steps; maximum is {PPSC_MAX_STEPS}",
            )));
        }
        let mut state = seed;
        let mut steps = Vec::with_capacity(step_count);
        for index in 0..step_count {
            let draw = splitmix64(&mut state);
            steps.push(universal_step(seed, index, step_count, draw));
        }
        Self::new(format!("seed-{seed}"), seed, steps)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("PPSC scenario serialization should be infallible")
    }

    pub fn replay_command(&self, backend: PpscBackend) -> String {
        format!(
            "NIMBUS_PPSC_SEED={} NIMBUS_PPSC_BACKEND={} make verify-ppsc-seed-farm",
            self.seed,
            backend.as_str()
        )
    }

    pub fn validate_for_backend(&self, backend: PpscBackend) -> Result<(), PpscScenarioError> {
        let capabilities = backend.capabilities();
        for (index, step) in self.steps.iter().enumerate() {
            if step.operation.needs_durable_reopen() && !capabilities.durable_reopen {
                return Err(PpscScenarioError::new(format!(
                    "scenario {} step {index} ({:?}) requires durable reopen, unsupported by {}; replay: {}",
                    self.name,
                    step.operation,
                    backend.as_str(),
                    self.replay_command(backend)
                )));
            }
            if step.operation.needs_provider_authority() && !capabilities.provider_authority {
                return Err(PpscScenarioError::new(format!(
                    "scenario {} step {index} ({:?}) requires provider sequence authority, unsupported by {}; replay: {}",
                    self.name,
                    step.operation,
                    backend.as_str(),
                    self.replay_command(backend)
                )));
            }
        }
        Ok(())
    }
}

pub fn retained_ppsc_scenarios() -> Vec<PpscScenario> {
    const SEEDS: [u64; 16] = [
        7, 11, 19, 23, 31, 41, 47, 59, 67, 73, 83, 97, 103, 109, 127, 131,
    ];
    SEEDS
        .into_iter()
        .map(|seed| PpscScenario::seeded(seed, 32).expect("retained seed must be valid"))
        .collect()
}

pub fn retained_provider_authority_scenarios() -> Vec<PpscScenario> {
    [211_u64, 223, 227]
        .into_iter()
        .map(|seed| {
            let tenant = format!("tenant-{seed}-hot");
            let mut steps = PpscScenario::seeded(seed, 32)
                .expect("provider base scenario must be valid")
                .steps;
            let shutdown = steps
                .pop()
                .expect("provider base scenario must have terminal shutdown");
            debug_assert!(matches!(shutdown.operation, PpscOperation::Quiesce));
            steps.extend([
                PpscStep::new(
                    PpscOperation::ExpireProviderLease {
                        tenant: tenant.clone(),
                    },
                    PpscExpectedOutcome::Observed,
                ),
                PpscStep::new(
                    PpscOperation::ProviderTakeover {
                        tenant: tenant.clone(),
                    },
                    PpscExpectedOutcome::Committed,
                ),
                PpscStep::new(
                    PpscOperation::AttemptStaleProviderWrite { tenant },
                    PpscExpectedOutcome::Fenced,
                ),
                shutdown,
            ]);
            PpscScenario::new(format!("provider-authority-{seed}"), seed, steps)
                .expect("provider retained seed must be valid")
        })
        .collect()
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn tenant(seed: u64, hot: bool) -> String {
    format!("tenant-{seed}-{}", if hot { "hot" } else { "peer" })
}

fn order(draw: u64) -> PpscCommitOrder {
    match draw % 6 {
        0 => PpscCommitOrder::QueuedDirectExecutionUnit,
        1 => PpscCommitOrder::QueuedExecutionUnitDirect,
        2 => PpscCommitOrder::DirectQueuedExecutionUnit,
        3 => PpscCommitOrder::DirectExecutionUnitQueued,
        4 => PpscCommitOrder::ExecutionUnitQueuedDirect,
        _ => PpscCommitOrder::ExecutionUnitDirectQueued,
    }
}

fn mutation(
    seed: u64,
    index: usize,
    draw: u64,
    route: PpscRoute,
    hot: bool,
    expected: PpscExpectedOutcome,
) -> PpscStep {
    PpscStep::new(
        PpscOperation::Mutation {
            tenant: tenant(seed, hot),
            route,
            key: format!("key-{}", draw % 11),
            value: i64::try_from(index).unwrap_or(i64::MAX),
        },
        expected,
    )
}

fn universal_step(seed: u64, index: usize, step_count: usize, draw: u64) -> PpscStep {
    if index + 1 == step_count {
        return PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown);
    }
    let hot = tenant(seed, true);
    match index % 31 {
        0 => mutation(
            seed,
            index,
            draw,
            PpscRoute::QueuedJournal,
            true,
            PpscExpectedOutcome::Committed,
        ),
        1 => mutation(
            seed,
            index,
            draw,
            PpscRoute::Direct,
            true,
            PpscExpectedOutcome::Committed,
        ),
        2 => mutation(
            seed,
            index,
            draw,
            PpscRoute::ExecutionUnit,
            true,
            PpscExpectedOutcome::Committed,
        ),
        3 => PpscStep::new(
            PpscOperation::CommitPermutation {
                tenant: hot,
                order: order(draw),
                value_base: i64::try_from(draw & 0x7fff).unwrap_or(i64::MAX),
            },
            PpscExpectedOutcome::Committed,
        ),
        4 => PpscStep::new(
            PpscOperation::SchemaSet {
                tenant: hot,
                revision: draw,
            },
            PpscExpectedOutcome::Committed,
        ),
        5 => PpscStep::new(
            PpscOperation::Schedule {
                tenant: hot,
                job: draw,
            },
            PpscExpectedOutcome::Committed,
        ),
        6 => PpscStep::new(
            PpscOperation::TriggerCursorAdvance {
                tenant: hot,
                through: (draw % 3) + 1,
            },
            PpscExpectedOutcome::Committed,
        ),
        7 => PpscStep::new(
            PpscOperation::ProjectionUpdate {
                tenant: hot,
                revision: draw,
            },
            PpscExpectedOutcome::Committed,
        ),
        8 => PpscStep::new(
            PpscOperation::ArmFault {
                tenant: hot,
                fault: PpscInjectedFault::AcknowledgementLoss,
            },
            PpscExpectedOutcome::Observed,
        ),
        9 => mutation(
            seed,
            index,
            draw,
            PpscRoute::QueuedJournal,
            true,
            PpscExpectedOutcome::AmbiguousRecovered,
        ),
        10 => PpscStep::new(
            PpscOperation::CancelNext {
                tenant: hot,
                route: PpscRoute::ExecutionUnit,
            },
            PpscExpectedOutcome::Cancelled,
        ),
        11 => PpscStep::new(
            PpscOperation::ForceOverload { tenant: hot },
            PpscExpectedOutcome::Overloaded,
        ),
        12 => mutation(
            seed,
            index,
            draw,
            PpscRoute::QueuedJournal,
            false,
            PpscExpectedOutcome::Committed,
        ),
        13 => PpscStep::new(
            PpscOperation::AdvanceWallClock {
                millis: draw % 10_000,
            },
            PpscExpectedOutcome::Observed,
        ),
        14 => PpscStep::new(
            PpscOperation::AdvanceMonotonicClock {
                millis: draw % 10_000,
            },
            PpscExpectedOutcome::Observed,
        ),
        15 => PpscStep::new(
            PpscOperation::Replay {
                tenant: hot,
                sequence: 2,
                identity: "same-content".to_string(),
            },
            PpscExpectedOutcome::Committed,
        ),
        16 => PpscStep::new(
            PpscOperation::Replay {
                tenant: hot,
                sequence: 2,
                identity: "different-content".to_string(),
            },
            PpscExpectedOutcome::DefinitiveRollback,
        ),
        17 => PpscStep::new(
            PpscOperation::RestoreImport {
                tenant: format!("tenant-{seed}-restore"),
                archive: draw,
            },
            PpscExpectedOutcome::Committed,
        ),
        18 => PpscStep::new(
            PpscOperation::SchemaDelete { tenant: hot },
            PpscExpectedOutcome::Committed,
        ),
        19 => PpscStep::new(
            PpscOperation::ArmFault {
                tenant: hot,
                fault: PpscInjectedFault::PublicationPredecessorHeld,
            },
            PpscExpectedOutcome::Observed,
        ),
        20 => mutation(
            seed,
            index,
            draw,
            PpscRoute::Direct,
            true,
            PpscExpectedOutcome::Committed,
        ),
        21 => PpscStep::new(
            PpscOperation::ReleaseFault {
                tenant: hot,
                fault: PpscInjectedFault::PublicationPredecessorHeld,
            },
            PpscExpectedOutcome::Observed,
        ),
        22 => PpscStep::new(PpscOperation::Crash, PpscExpectedOutcome::Observed),
        23 => PpscStep::new(PpscOperation::Reopen, PpscExpectedOutcome::Observed),
        24 => PpscStep::new(
            PpscOperation::ArmFault {
                tenant: hot,
                fault: PpscInjectedFault::ProviderTransient,
            },
            PpscExpectedOutcome::Observed,
        ),
        25 => mutation(
            seed,
            index,
            draw,
            PpscRoute::QueuedJournal,
            true,
            PpscExpectedOutcome::ProviderError,
        ),
        26 => PpscStep::new(
            PpscOperation::ReleaseFault {
                tenant: hot,
                fault: PpscInjectedFault::ProviderTransient,
            },
            PpscExpectedOutcome::Observed,
        ),
        27 => PpscStep::new(
            PpscOperation::ZeroWriteExecutionUnit { tenant: hot },
            PpscExpectedOutcome::Observed,
        ),
        28 => PpscStep::new(
            PpscOperation::ConflictRetry {
                tenant: hot,
                key: format!("conflict-{}", draw % 7),
                first: i64::try_from(draw & 0x7fff).unwrap_or(i64::MAX),
                second: i64::try_from((draw >> 16) & 0x7fff).unwrap_or(i64::MAX),
            },
            PpscExpectedOutcome::Committed,
        ),
        29 => mutation(
            seed,
            index,
            draw,
            PpscRoute::QueuedJournal,
            false,
            PpscExpectedOutcome::Committed,
        ),
        _ => mutation(
            seed,
            index,
            draw,
            PpscRoute::ExecutionUnit,
            true,
            PpscExpectedOutcome::Committed,
        ),
    }
}
