use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{PPSC_MAX_STEPS, PpscBackend, PpscScenario};

pub const PPSC_SEED_FARM_DEFAULT_STEPS: usize = 32;

const BACKEND_ENV: &str = "NIMBUS_PPSC_BACKEND";
const SEED_ENV: &str = "NIMBUS_PPSC_SEED";
const SEED_START_ENV: &str = "NIMBUS_PPSC_SEED_START";
const SEED_COUNT_ENV: &str = "NIMBUS_PPSC_SEED_COUNT";
const SHARD_INDEX_ENV: &str = "NIMBUS_PPSC_SHARD_INDEX";
const SHARD_COUNT_ENV: &str = "NIMBUS_PPSC_SHARD_COUNT";
const STEP_COUNT_ENV: &str = "NIMBUS_PPSC_STEP_COUNT";
const FAILURE_DIR_ENV: &str = "NIMBUS_PPSC_FAILURE_DIR";
const REVISION_ENV: &str = "NIMBUS_PPSC_REVISION";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpscSeedFarmConfig {
    pub backend: PpscBackend,
    pub seeds: Vec<u64>,
    pub seed_start: u64,
    pub seed_count: usize,
    pub shard_index: usize,
    pub shard_count: usize,
    pub step_count: usize,
    pub failure_dir: PathBuf,
    pub revision: String,
}

impl PpscSeedFarmConfig {
    pub fn from_environment() -> Result<Self, PpscSeedFarmError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, PpscSeedFarmError> {
        let backend_name = required(&lookup, BACKEND_ENV)?;
        let backend = match backend_name.as_str() {
            "redb" => PpscBackend::Redb,
            other => {
                return Err(PpscSeedFarmError::new(format!(
                    "{BACKEND_ENV}={other} is unsupported by the bulk farm; expected redb (live-provider differential lanes are separate)"
                )));
            }
        };
        let step_count =
            optional_usize(&lookup, STEP_COUNT_ENV)?.unwrap_or(PPSC_SEED_FARM_DEFAULT_STEPS);
        if step_count == 0 || step_count > PPSC_MAX_STEPS {
            return Err(PpscSeedFarmError::new(format!(
                "{STEP_COUNT_ENV} must be between 1 and {PPSC_MAX_STEPS} (got {step_count})"
            )));
        }
        let failure_dir = PathBuf::from(required(&lookup, FAILURE_DIR_ENV)?);
        let revision = required(&lookup, REVISION_ENV)?;

        let (seeds, seed_start, seed_count, shard_index, shard_count) =
            if let Some(single_seed) = optional_u64(&lookup, SEED_ENV)? {
                for range_name in [
                    SEED_START_ENV,
                    SEED_COUNT_ENV,
                    SHARD_INDEX_ENV,
                    SHARD_COUNT_ENV,
                ] {
                    if lookup(range_name).is_some() {
                        return Err(PpscSeedFarmError::new(format!(
                            "{SEED_ENV} cannot be combined with {range_name}"
                        )));
                    }
                }
                (vec![single_seed], single_seed, 1, 0, 1)
            } else {
                let start = required_u64(&lookup, SEED_START_ENV)?;
                let count = required_usize(&lookup, SEED_COUNT_ENV)?;
                let shard_index = required_usize(&lookup, SHARD_INDEX_ENV)?;
                let shard_count = required_usize(&lookup, SHARD_COUNT_ENV)?;
                let seeds = select_shard(start, count, shard_index, shard_count)?;
                (seeds, start, count, shard_index, shard_count)
            };

        Ok(Self {
            backend,
            seeds,
            seed_start,
            seed_count,
            shard_index,
            shard_count,
            step_count,
            failure_dir,
            revision,
        })
    }

    pub fn selected_count(&self) -> usize {
        self.seeds.len()
    }
}

pub fn select_shard(
    seed_start: u64,
    seed_count: usize,
    shard_index: usize,
    shard_count: usize,
) -> Result<Vec<u64>, PpscSeedFarmError> {
    if seed_count == 0 {
        return Err(PpscSeedFarmError::new(format!(
            "{SEED_COUNT_ENV} must be greater than zero"
        )));
    }
    if shard_count == 0 {
        return Err(PpscSeedFarmError::new(format!(
            "{SHARD_COUNT_ENV} must be greater than zero"
        )));
    }
    if shard_index >= shard_count {
        return Err(PpscSeedFarmError::new(format!(
            "{SHARD_INDEX_ENV} must be less than {SHARD_COUNT_ENV} (got {shard_index}/{shard_count})"
        )));
    }
    if shard_count > seed_count {
        return Err(PpscSeedFarmError::new(format!(
            "{SHARD_COUNT_ENV}={shard_count} would create empty shards for {SEED_COUNT_ENV}={seed_count}"
        )));
    }
    let final_offset = seed_count
        .checked_sub(1)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| PpscSeedFarmError::new("PPSC seed-farm range length does not fit in u64"))?;
    seed_start.checked_add(final_offset).ok_or_else(|| {
        PpscSeedFarmError::new("PPSC seed-farm range overflows the u64 seed space")
    })?;

    let base = seed_count / shard_count;
    let remainder = seed_count % shard_count;
    let leading_extra = shard_index.min(remainder);
    let offset = shard_index
        .checked_mul(base)
        .and_then(|value| value.checked_add(leading_extra))
        .ok_or_else(|| PpscSeedFarmError::new("PPSC seed-farm shard offset overflowed"))?;
    let shard_len = base + usize::from(shard_index < remainder);
    let first = seed_start
        .checked_add(u64::try_from(offset).map_err(|_| {
            PpscSeedFarmError::new("PPSC seed-farm shard offset does not fit in u64")
        })?)
        .ok_or_else(|| PpscSeedFarmError::new("PPSC seed-farm first seed overflowed"))?;
    let mut seeds = Vec::with_capacity(shard_len);
    for offset in 0..shard_len {
        seeds.push(
            first
                .checked_add(u64::try_from(offset).map_err(|_| {
                    PpscSeedFarmError::new("PPSC seed-farm seed offset does not fit in u64")
                })?)
                .ok_or_else(|| PpscSeedFarmError::new("PPSC seed-farm seed overflowed"))?,
        );
    }
    Ok(seeds)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PpscSeedFarmFailureKind {
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscSeedFarmFailureBundle {
    pub format_version: u32,
    pub kind: PpscSeedFarmFailureKind,
    pub revision: String,
    pub backend: PpscBackend,
    pub seed: u64,
    pub step_count: usize,
    pub shard_index: usize,
    pub shard_count: usize,
    pub replay_command: String,
    pub message: String,
    pub scenario: PpscScenario,
}

impl PpscSeedFarmFailureBundle {
    fn new(
        config: &PpscSeedFarmConfig,
        scenario: PpscScenario,
        kind: PpscSeedFarmFailureKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            format_version: 1,
            kind,
            revision: config.revision.clone(),
            backend: config.backend,
            seed: scenario.seed,
            step_count: config.step_count,
            shard_index: config.shard_index,
            shard_count: config.shard_count,
            replay_command: scenario.replay_command(config.backend),
            message: message.into(),
            scenario,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PpscSeedFarmSummary {
    pub format_version: u32,
    pub revision: String,
    pub backend: PpscBackend,
    pub seed_start: u64,
    pub seed_count: usize,
    pub shard_index: usize,
    pub shard_count: usize,
    pub selected: usize,
    pub executed: usize,
    pub passed: usize,
    pub failed: usize,
    pub retained: usize,
}

impl PpscSeedFarmSummary {
    pub fn is_complete_success(&self) -> bool {
        self.selected > 0
            && self.executed == self.selected
            && self.passed == self.selected
            && self.failed == 0
    }
}

pub struct PpscSeedFarmArtifacts {
    root: PathBuf,
}

impl PpscSeedFarmArtifacts {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, PpscSeedFarmError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|error| {
            PpscSeedFarmError::io("create PPSC seed-farm artifact directory", &root, error)
        })?;
        for entry in fs::read_dir(&root).map_err(|error| {
            PpscSeedFarmError::io("read PPSC seed-farm artifact directory", &root, error)
        })? {
            let entry = entry.map_err(|error| {
                PpscSeedFarmError::io("inspect PPSC seed-farm artifact directory", &root, error)
            })?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_owned_artifact_name(&name) {
                fs::remove_file(entry.path()).map_err(|error| {
                    PpscSeedFarmError::io(
                        "remove stale PPSC seed-farm artifact",
                        &entry.path(),
                        error,
                    )
                })?;
            }
        }
        Ok(Self { root })
    }

    pub fn begin_seed(
        &self,
        config: &PpscSeedFarmConfig,
        scenario: &PpscScenario,
    ) -> Result<PathBuf, PpscSeedFarmError> {
        let path = self.pending_path(scenario.seed);
        let bundle = PpscSeedFarmFailureBundle::new(
            config,
            scenario.clone(),
            PpscSeedFarmFailureKind::Interrupted,
            "seed execution was interrupted before the runner recorded completion",
        );
        write_json_atomically(&path, &bundle)?;
        Ok(path)
    }

    pub fn mark_seed_passed(&self, pending: &Path) -> Result<(), PpscSeedFarmError> {
        fs::remove_file(pending).map_err(|error| {
            PpscSeedFarmError::io("remove completed PPSC seed marker", pending, error)
        })
    }

    pub fn mark_seed_failed(
        &self,
        config: &PpscSeedFarmConfig,
        scenario: &PpscScenario,
        pending: &Path,
        message: impl Into<String>,
    ) -> Result<PathBuf, PpscSeedFarmError> {
        let path = self.failure_path(scenario.seed);
        let bundle = PpscSeedFarmFailureBundle::new(
            config,
            scenario.clone(),
            PpscSeedFarmFailureKind::Failed,
            message,
        );
        write_json_atomically(&path, &bundle)?;
        fs::remove_file(pending).map_err(|error| {
            PpscSeedFarmError::io("remove superseded PPSC interruption marker", pending, error)
        })?;
        Ok(path)
    }

    pub fn write_summary(
        &self,
        summary: &PpscSeedFarmSummary,
    ) -> Result<PathBuf, PpscSeedFarmError> {
        let path = self.root.join("summary.json");
        write_json_atomically(&path, summary)?;
        Ok(path)
    }

    fn pending_path(&self, seed: u64) -> PathBuf {
        self.root.join(format!("seed-{seed:020}-interrupted.json"))
    }

    fn failure_path(&self, seed: u64) -> PathBuf {
        self.root.join(format!("seed-{seed:020}-failure.json"))
    }
}

fn is_owned_artifact_name(name: &str) -> bool {
    if name == "summary.json" {
        return true;
    }
    if let Some(pid) = name.strip_prefix("summary.tmp-") {
        return !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit());
    }

    let Some(seed_and_kind) = name.strip_prefix("seed-") else {
        return false;
    };
    let Some((seed, kind)) = seed_and_kind.split_once('-') else {
        return false;
    };
    if seed.len() != 20 || !seed.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if matches!(kind, "interrupted.json" | "failure.json") {
        return true;
    }
    ["interrupted.tmp-", "failure.tmp-"]
        .into_iter()
        .any(|prefix| {
            kind.strip_prefix(prefix)
                .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<(), PpscSeedFarmError> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = File::create(&temporary).map_err(|error| {
        PpscSeedFarmError::io(
            "create temporary PPSC seed-farm artifact",
            &temporary,
            error,
        )
    })?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| {
        PpscSeedFarmError::new(format!(
            "serialize PPSC seed-farm artifact {}: {error}",
            temporary.display()
        ))
    })?;
    file.write_all(b"\n").map_err(|error| {
        PpscSeedFarmError::io("finish PPSC seed-farm artifact", &temporary, error)
    })?;
    file.sync_all().map_err(|error| {
        PpscSeedFarmError::io("sync PPSC seed-farm artifact", &temporary, error)
    })?;
    fs::rename(&temporary, path)
        .map_err(|error| PpscSeedFarmError::io("publish PPSC seed-farm artifact", path, error))
}

fn required(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &str,
) -> Result<String, PpscSeedFarmError> {
    lookup(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PpscSeedFarmError::new(format!("{name} must be set and non-empty")))
}

fn required_u64(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &str,
) -> Result<u64, PpscSeedFarmError> {
    parse_u64(name, &required(lookup, name)?)
}

fn required_usize(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &str,
) -> Result<usize, PpscSeedFarmError> {
    parse_usize(name, &required(lookup, name)?)
}

fn optional_u64(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &str,
) -> Result<Option<u64>, PpscSeedFarmError> {
    lookup(name)
        .filter(|value| !value.is_empty())
        .map(|value| parse_u64(name, &value))
        .transpose()
}

fn optional_usize(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &str,
) -> Result<Option<usize>, PpscSeedFarmError> {
    lookup(name)
        .filter(|value| !value.is_empty())
        .map(|value| parse_usize(name, &value))
        .transpose()
}

fn parse_u64(name: &str, value: &str) -> Result<u64, PpscSeedFarmError> {
    value.parse::<u64>().map_err(|error| {
        PpscSeedFarmError::new(format!("{name} must be an unsigned integer: {error}"))
    })
}

fn parse_usize(name: &str, value: &str) -> Result<usize, PpscSeedFarmError> {
    value.parse::<usize>().map_err(|error| {
        PpscSeedFarmError::new(format!("{name} must be an unsigned integer: {error}"))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpscSeedFarmError {
    message: String,
}

impl PpscSeedFarmError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn io(action: &str, path: &Path, error: io::Error) -> Self {
        Self::new(format!("{action} {}: {error}", path.display()))
    }
}

impl fmt::Display for PpscSeedFarmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PpscSeedFarmError {}
