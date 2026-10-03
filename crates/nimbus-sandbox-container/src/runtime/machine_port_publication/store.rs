//! Atomic durable storage and cross-process serialization for machine port publication.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use nimbus_durable_record::{
    CleanupError, CleanupStep, StagedWrite, WriteCheckpoint, WriteError, WriteFailure, WriteStep,
    sync_directory,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::MachinePortPublicationRecord;
use nimbus_sandbox::{Result, SandboxError};

const CURRENT_MACHINE_PORT_EVIDENCE_ENVELOPE_VERSION: u32 = 1;
pub(super) const MACHINE_PORT_EVIDENCE_FILE: &str = ".nimbus-machine-port-evidence.json";
pub(super) const MACHINE_PORT_EVIDENCE_STAGE_FILE: &str = ".nimbus-machine-port-evidence.stage";
pub(super) const MACHINE_PORT_EVIDENCE_LOCK_FILE: &str = ".nimbus-machine-port-evidence.lock";
#[cfg(not(test))]
const MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT: Duration = Duration::from_millis(100);
const MACHINE_PORT_EVIDENCE_LOCK_RETRY: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MachinePortEvidenceLockError {
    Timeout { path: PathBuf },
    Failed { message: String },
}

impl MachinePortEvidenceLockError {
    pub(super) fn into_sandbox_error(self) -> SandboxError {
        match self {
            Self::Timeout { path } => SandboxError::OperationFailed {
                message: format!(
                    "timed out acquiring machine port evidence lock {}; canonical observation \
                     remains unchanged",
                    path.display()
                ),
            },
            Self::Failed { message } => SandboxError::OperationFailed { message },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MachinePortEvidenceStoreCheckpoint {
    StageDurable,
    CanonicalRenamed,
}

pub(super) trait MachinePortEvidenceStoreObserver {
    fn checkpoint(&mut self, checkpoint: MachinePortEvidenceStoreCheckpoint) -> Result<()>;
}

struct NoopMachinePortEvidenceStoreObserver;

impl MachinePortEvidenceStoreObserver for NoopMachinePortEvidenceStoreObserver {
    fn checkpoint(&mut self, _checkpoint: MachinePortEvidenceStoreCheckpoint) -> Result<()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachinePortEvidenceEnvelope {
    version: u32,
    record_sha256: String,
    record: MachinePortPublicationRecord,
}

impl MachinePortEvidenceEnvelope {
    fn new(record: MachinePortPublicationRecord) -> Result<Self> {
        Ok(Self {
            version: CURRENT_MACHINE_PORT_EVIDENCE_ENVELOPE_VERSION,
            record_sha256: record_sha256(&record)?,
            record,
        })
    }

    fn authenticate(self, path: &Path) -> Result<MachinePortPublicationRecord> {
        if self.version != CURRENT_MACHINE_PORT_EVIDENCE_ENVELOPE_VERSION
            || self.record_sha256 != record_sha256(&self.record)?
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port evidence {} has an unsupported version or failed SHA-256 \
                     integrity authentication; provider effects remain fenced",
                    path.display()
                ),
            });
        }
        Ok(self.record)
    }
}

#[cfg(test)]
pub(super) fn publish_record(
    state_root: &Path,
    state_dir: &Path,
    record: MachinePortPublicationRecord,
) -> Result<()> {
    nimbus_sandbox::backends::oci::durable_directory::establish_durable_directory_chain_with(
        state_root,
        state_dir,
        "machine port publication",
        sync_directory,
    )?;
    let _guard = lock_publication(state_dir)?;
    remove_stale_stage(state_dir)?;
    publish_record_locked(state_dir, &record)
}

#[cfg(test)]
pub(super) fn publish_record_with_observer(
    state_root: &Path,
    state_dir: &Path,
    record: MachinePortPublicationRecord,
    observer: &mut impl MachinePortEvidenceStoreObserver,
) -> Result<()> {
    nimbus_sandbox::backends::oci::durable_directory::establish_durable_directory_chain_with(
        state_root,
        state_dir,
        "machine port publication",
        sync_directory,
    )?;
    let _guard = lock_publication(state_dir)?;
    remove_stale_stage(state_dir)?;
    publish_record_locked_with_observer(state_dir, &record, observer)
}

pub(super) fn publish_record_locked(
    state_dir: &Path,
    record: &MachinePortPublicationRecord,
) -> Result<()> {
    publish_record_locked_with_observer(
        state_dir,
        record,
        &mut NoopMachinePortEvidenceStoreObserver,
    )
}

fn publish_record_locked_with_observer(
    state_dir: &Path,
    record: &MachinePortPublicationRecord,
    observer: &mut impl MachinePortEvidenceStoreObserver,
) -> Result<()> {
    let envelope = MachinePortEvidenceEnvelope::new(record.clone())?;
    let mut rendered =
        serde_json::to_vec_pretty(&envelope).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize machine port publication: {error}"),
        })?;
    rendered.push(b'\n');
    let stage_path = state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE);
    let evidence_path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    let mut checkpoint = |checkpoint: WriteCheckpoint| {
        observer.checkpoint(match checkpoint {
            WriteCheckpoint::StageDurable => MachinePortEvidenceStoreCheckpoint::StageDurable,
            WriteCheckpoint::Committed => MachinePortEvidenceStoreCheckpoint::CanonicalRenamed,
        })
    };
    StagedWrite::new(&stage_path, &evidence_path)
        .observer(&mut checkpoint)
        .write(&rendered)
        .map_err(|error| publication_error(&stage_path, &evidence_path, error))
}

fn publication_error(
    stage_path: &Path,
    evidence_path: &Path,
    error: WriteError<SandboxError>,
) -> SandboxError {
    let primary = match error.failure {
        WriteFailure::Io {
            step: WriteStep::CreateStage,
            source,
            ..
        } => SandboxError::OperationFailed {
            message: format!(
                "failed to create staged machine port evidence {}: {source}",
                stage_path.display()
            ),
        },
        WriteFailure::Io {
            step: WriteStep::WriteStage | WriteStep::SyncStage,
            source,
            ..
        } => SandboxError::OperationFailed {
            message: format!(
                "failed to durably stage machine port evidence {}: {source}",
                stage_path.display()
            ),
        },
        WriteFailure::Io {
            step: WriteStep::Commit,
            source,
            ..
        } => SandboxError::OperationFailed {
            message: format!(
                "failed to atomically publish machine port evidence {}: {source}",
                evidence_path.display()
            ),
        },
        WriteFailure::Io {
            step: WriteStep::SyncDirectory,
            source,
            ..
        } => SandboxError::OperationFailed {
            message: format!(
                "machine port evidence {} reached its commit point but directory sync failed; \
                 publication outcome is ambiguous: {source}",
                evidence_path.display()
            ),
        },
        WriteFailure::Observer { error, .. } => error,
    };
    let cleanup = match error.cleanup {
        None => return primary,
        Some(CleanupError {
            step: CleanupStep::RemoveStage,
            source,
            ..
        }) => format!(
            "failed to remove machine port evidence {}: {source}",
            stage_path.display()
        ),
        Some(CleanupError {
            step: CleanupStep::SyncDirectory,
            source,
            ..
        }) => format!(
            "failed to durably clean staged machine port evidence {}: {source}",
            stage_path.display()
        ),
    };
    SandboxError::OperationFailed {
        message: format!("{primary}; staged machine port evidence cleanup also failed: {cleanup}"),
    }
}

pub(super) fn read_record(state_dir: &Path) -> Result<MachinePortPublicationRecord> {
    let path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    let metadata = fs::symlink_metadata(&path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to inspect machine port evidence {}: {error}",
            path.display()
        ),
    })?;
    if !metadata.file_type().is_file() {
        return Err(non_regular_entry(&path));
    }
    let bytes = fs::read(&path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read machine port evidence {}: {error}",
            path.display()
        ),
    })?;
    let envelope: MachinePortEvidenceEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse strict machine port evidence {}: {error}",
                path.display()
            ),
        })?;
    envelope.authenticate(&path)
}

pub(super) fn read_record_if_present(
    state_dir: &Path,
) -> Result<Option<MachinePortPublicationRecord>> {
    let path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => read_record(state_dir).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to inspect machine port evidence {}: {error}",
                path.display()
            ),
        }),
    }
}

pub(super) fn lock_publication(state_dir: &Path) -> Result<MachinePortEvidenceGuard> {
    lock_publication_typed(state_dir).map_err(MachinePortEvidenceLockError::into_sandbox_error)
}

#[cfg(test)]
pub(super) fn lock_publication_for_test(
    state_dir: &Path,
) -> std::result::Result<MachinePortEvidenceGuard, MachinePortEvidenceLockError> {
    lock_publication_typed(state_dir)
}

fn lock_publication_typed(
    state_dir: &Path,
) -> std::result::Result<MachinePortEvidenceGuard, MachinePortEvidenceLockError> {
    let path = state_dir.join(MACHINE_PORT_EVIDENCE_LOCK_FILE);
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && !metadata.file_type().is_file()
    {
        return Err(MachinePortEvidenceLockError::Failed {
            message: non_regular_entry(&path).to_string(),
        });
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| MachinePortEvidenceLockError::Failed {
            message: format!(
                "failed to open machine port evidence lock {}: {error}",
                path.display()
            ),
        })?;
    if !lock
        .metadata()
        .map_err(|error| MachinePortEvidenceLockError::Failed {
            message: format!(
                "failed to inspect machine port evidence lock {}: {error}",
                path.display()
            ),
        })?
        .is_file()
    {
        return Err(MachinePortEvidenceLockError::Failed {
            message: non_regular_entry(&path).to_string(),
        });
    }
    let deadline = Instant::now() + MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT;
    loop {
        match lock.try_lock().map_err(std::io::Error::from) {
            Ok(()) => return Ok(MachinePortEvidenceGuard { _lock: lock }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(MachinePortEvidenceLockError::Timeout { path });
                }
                thread::sleep(MACHINE_PORT_EVIDENCE_LOCK_RETRY);
            }
            Err(error) => {
                return Err(MachinePortEvidenceLockError::Failed {
                    message: format!(
                        "failed to acquire machine port evidence lock {}: {error}",
                        path.display()
                    ),
                });
            }
        }
    }
}

pub(super) fn remove_stale_stage(state_dir: &Path) -> Result<()> {
    let stage_path = state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE);
    if remove_regular_file_if_present(&stage_path)? {
        sync_directory(state_dir).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to durably remove stale machine port evidence stage {}: {error}",
                stage_path.display()
            ),
        })?;
    }
    Ok(())
}

fn remove_regular_file_if_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => Err(non_regular_entry(path)),
        Ok(_) => {
            fs::remove_file(path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to remove machine port evidence {}: {error}",
                    path.display()
                ),
            })?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to inspect machine port evidence entry {}: {error}",
                path.display()
            ),
        }),
    }
}

fn non_regular_entry(path: &Path) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "machine port evidence entry {} is not a regular file; observation remains fenced",
            path.display()
        ),
    }
}

fn record_sha256(record: &MachinePortPublicationRecord) -> Result<String> {
    let bytes = serde_json::to_vec(record).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to serialize machine port publication for integrity: {error}"),
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(Debug)]
pub(super) struct MachinePortEvidenceGuard {
    _lock: File,
}
