//! Machine port evidence envelope over the durable record protocol.
//!
//! `nimbus-durable-record` owns the lock, the staged write, and stale-stage
//! removal. This module owns the envelope, its integrity authentication, and
//! the fence wording of each failure.

use std::fs;
use std::path::Path;
use std::time::Duration;

use nimbus_durable_record::{EntryError, RecordLock, StagedWrite, WriteCheckpoint, WriteError};
use nimbus_durable_record::{WriteFailure, remove_stale_stage as remove_stage};
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

/// Observes each durable checkpoint of a publication. Tests inject faults here.
pub(super) type StoreObserver<'a> = &'a mut dyn FnMut(WriteCheckpoint) -> Result<()>;

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
    publish_record_with_observer(state_root, state_dir, record, &mut |_| Ok(()))
}

#[cfg(test)]
pub(super) fn publish_record_with_observer(
    state_root: &Path,
    state_dir: &Path,
    record: MachinePortPublicationRecord,
    observer: StoreObserver<'_>,
) -> Result<()> {
    nimbus_sandbox::durable_directory::establish_durable_directory_chain_with(
        state_root,
        state_dir,
        "machine port publication",
        nimbus_durable_record::sync_directory,
    )?;
    let _guard = lock_publication(state_dir)?;
    remove_stale_stage(state_dir)?;
    publish_locked(state_dir, &record, observer)
}

pub(super) fn publish_record_locked(
    state_dir: &Path,
    record: &MachinePortPublicationRecord,
) -> Result<()> {
    publish_locked(state_dir, record, &mut |_| Ok(()))
}

fn publish_locked(
    state_dir: &Path,
    record: &MachinePortPublicationRecord,
    observer: StoreObserver<'_>,
) -> Result<()> {
    let envelope = MachinePortEvidenceEnvelope::new(record.clone())?;
    let mut rendered =
        serde_json::to_vec_pretty(&envelope).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize machine port publication: {error}"),
        })?;
    rendered.push(b'\n');
    let stage_path = state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE);
    let evidence_path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    StagedWrite::new(&stage_path, &evidence_path)
        .observer(observer)
        .write(&rendered)
        .map_err(|error| match error {
            // An injected acknowledgement loss surfaces unchanged.
            WriteError {
                failure: WriteFailure::Observer { error, .. },
                cleanup: None,
            } => error,
            error => SandboxError::OperationFailed {
                message: format!("failed to publish machine port evidence: {error}"),
            },
        })
}

pub(super) fn read_record(state_dir: &Path) -> Result<MachinePortPublicationRecord> {
    read_record_if_present(state_dir)?.ok_or_else(|| SandboxError::OperationFailed {
        message: format!(
            "machine port evidence {} does not exist",
            state_dir.join(MACHINE_PORT_EVIDENCE_FILE).display()
        ),
    })
}

pub(super) fn read_record_if_present(
    state_dir: &Path,
) -> Result<Option<MachinePortPublicationRecord>> {
    let path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    let io_error = |operation: &str, error: std::io::Error| SandboxError::OperationFailed {
        message: format!(
            "failed to {operation} machine port evidence {}: {error}",
            path.display()
        ),
    };
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_file() => return Err(non_regular_entry(&path)),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("inspect", error)),
    }
    let bytes = fs::read(&path).map_err(|error| io_error("read", error))?;
    let envelope: MachinePortEvidenceEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse strict machine port evidence {}: {error}",
                path.display()
            ),
        })?;
    envelope.authenticate(&path).map(Some)
}

pub(super) fn lock_publication(state_dir: &Path) -> Result<RecordLock> {
    acquire_lock(state_dir).map_err(entry_error)
}

pub(super) fn acquire_lock(state_dir: &Path) -> std::result::Result<RecordLock, EntryError> {
    RecordLock::acquire(
        &state_dir.join(MACHINE_PORT_EVIDENCE_LOCK_FILE),
        MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT,
    )
}

pub(super) fn remove_stale_stage(state_dir: &Path) -> Result<()> {
    remove_stage(&state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE))
        .map(drop)
        .map_err(entry_error)
}

pub(super) fn entry_error(error: EntryError) -> SandboxError {
    match &error {
        EntryError::LockTimeout { path, .. } => SandboxError::OperationFailed {
            message: format!(
                "timed out acquiring machine port evidence lock {}; canonical observation \
                 remains unchanged",
                path.display()
            ),
        },
        EntryError::NotRegular { path } => non_regular_entry(path),
        EntryError::Io { .. } => SandboxError::OperationFailed {
            message: format!("machine port evidence store failed: {error}"),
        },
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
