use std::collections::BTreeMap;
use std::future::Future;

use nimbus_core::{Error, Result};

use super::generated::{GeneratedTaskHistory, GeneratedTaskHistoryStep, GeneratedTaskRecord};

pub const VERIFICATION_CASE_FILTER_ENV: &str = "NIMBUS_VERIFY_CASE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationHarnessMode {
    Required,
    Nightly,
}

impl VerificationHarnessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
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

const REQUIRED_GENERATED_TASK_HISTORY_CASES: [GeneratedTaskHistorySeedCase; 2] = [
    GeneratedTaskHistorySeedCase::new(
        "smoke-storage-baseline-31",
        31,
        24,
        false,
        "baseline smoke seed for cross-surface generated-history replay",
        VerificationHarnessMode::Required,
    ),
    GeneratedTaskHistorySeedCase::new(
        "regression-two-page-pagination-41",
        41,
        48,
        true,
        "regression seed that guarantees multi-page query and pagination coverage",
        VerificationHarnessMode::Required,
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
        VerificationHarnessMode::Required => &REQUIRED_GENERATED_TASK_HISTORY_CASES,
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
