//! Crash-safe node-local receipt store for exact Systemd teardown operations.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use nimbus_core::{Error, Result};
use nimbus_workloads::WorkloadOwnerEvidenceDigest;
use serde::{Deserialize, Serialize};

use super::teardown::SystemdTeardownState;

const STORE_VERSION: u32 = 1;
const STATE_FILE: &str = "systemd-teardown-state.json";
const LOCK_FILE: &str = "systemd-teardown-state.lock";
const TEMP_FILE: &str = ".systemd-teardown-state.tmp";

#[derive(Debug)]
pub(super) struct SystemdTeardownStore {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StateEnvelope {
    version: u32,
    checksum: WorkloadOwnerEvidenceDigest,
    payload: SystemdTeardownState,
}

impl SystemdTeardownStore {
    pub(super) fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|error| store_io("create state root", error))?;
        let store = Self { root };
        store.with_locked_state(|_| Ok(()))?;
        Ok(store)
    }

    /// Load, mutate, and durably replace the complete receipt set under one
    /// host-global cross-process lock.
    pub(super) fn transact<T>(
        &self,
        update: impl FnOnce(&mut SystemdTeardownState) -> Result<T>,
    ) -> Result<T> {
        self.with_locked_state(|state| {
            let result = update(state)?;
            self.write_state(state)?;
            Ok(result)
        })
    }

    fn with_locked_state<T>(
        &self,
        operation: impl FnOnce(&mut SystemdTeardownState) -> Result<T>,
    ) -> Result<T> {
        let lock_path = self.root.join(LOCK_FILE);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|error| store_io("open state lock", error))?;
        lock.lock_exclusive()
            .map_err(|error| store_io("lock state", error))?;
        let result = (|| {
            let mut state = self.read_state()?;
            operation(&mut state)
        })();
        let unlock_result = FileExt::unlock(&lock).map_err(|error| store_io("unlock state", error));
        match (result, unlock_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }

    fn read_state(&self) -> Result<SystemdTeardownState> {
        let path = self.root.join(STATE_FILE);
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(SystemdTeardownState::default());
            }
            Err(error) => return Err(store_io("open state", error)),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| store_io("read state", error))?;
        let envelope: StateEnvelope =
            serde_json::from_slice(&bytes).map_err(|error| store_codec("decode state", error))?;
        if envelope.version != STORE_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported systemd teardown state version {}",
                envelope.version
            )));
        }
        let payload = serde_json::to_vec(&envelope.payload)
            .map_err(|error| store_codec("encode state payload", error))?;
        let expected = WorkloadOwnerEvidenceDigest::sha256(payload);
        if envelope.checksum != expected {
            return Err(Error::InvalidInput(
                "systemd teardown state checksum does not match its payload".to_owned(),
            ));
        }
        Ok(envelope.payload)
    }

    fn write_state(&self, state: &SystemdTeardownState) -> Result<()> {
        let payload = serde_json::to_vec(state)
            .map_err(|error| store_codec("encode state payload", error))?;
        let envelope = StateEnvelope {
            version: STORE_VERSION,
            checksum: WorkloadOwnerEvidenceDigest::sha256(payload),
            payload: state.clone(),
        };
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|error| store_codec("encode state envelope", error))?;
        let temporary = self.root.join(TEMP_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| store_io("open temporary state", error))?;
        file.write_all(&bytes)
            .map_err(|error| store_io("write temporary state", error))?;
        file.sync_all()
            .map_err(|error| store_io("sync temporary state", error))?;
        fs::rename(&temporary, self.root.join(STATE_FILE))
            .map_err(|error| store_io("replace state", error))?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| store_io("sync state directory", error))
    }
}

fn store_io(action: &'static str, error: std::io::Error) -> Error {
    Error::Internal(format!("failed to {action} for systemd teardown: {error}"))
}

fn store_codec(action: &'static str, error: serde_json::Error) -> Error {
    Error::InvalidInput(format!("failed to {action} for systemd teardown: {error}"))
}

#[cfg(test)]
#[path = "teardown_store/tests.rs"]
mod tests;
