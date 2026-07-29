//! Durable observed evidence for provider-managed machine port publication.
//!
//! The container manifest remains desired/lifecycle state. This sibling
//! artifact is the sole guest-side observation commit point for a complete
//! gvproxy receipt batch. A partial batch is never published.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use nimbus_core::TenantId;
use nimbus_network::{NetworkProviderHandle, NetworkResourceGeneration};
use serde::{Deserialize, Serialize};

use crate::backends::oci::network::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, OciMachinePortForwarderConfig,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

use super::{ContainerSandboxBackend, ContainerSandboxManifest};

const CURRENT_MACHINE_PORT_EVIDENCE_VERSION: u32 = 1;
const MACHINE_PORT_EVIDENCE_FILE: &str = ".nimbus-machine-port-evidence.json";
const MACHINE_PORT_EVIDENCE_STAGE_FILE: &str = ".nimbus-machine-port-evidence.stage";
const MACHINE_PORT_EVIDENCE_LOCK_FILE: &str = ".nimbus-machine-port-evidence.lock";
#[cfg(not(test))]
const MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT: Duration = Duration::from_millis(100);
const MACHINE_PORT_EVIDENCE_LOCK_RETRY: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MachinePortEvidencePhase {
    Exposed,
    Absent,
}

impl MachinePortEvidencePhase {
    fn accepts(self, outcome: MachinePortForwardOutcome) -> bool {
        match self {
            Self::Exposed => outcome == MachinePortForwardOutcome::Exposed,
            Self::Absent => matches!(
                outcome,
                MachinePortForwardOutcome::Withdrawn
                    | MachinePortForwardOutcome::ExactAlreadyAbsent
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachinePortEvidenceRecord {
    version: u32,
    phase: MachinePortEvidencePhase,
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    receipts: Vec<MachinePortForwardReceipt>,
}

/// Complete durable provider observation for an absent machine publication.
///
/// Header identity remains available when the exact binding set is empty and
/// therefore has no per-binding receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachinePortAbsenceEvidence {
    pub tenant_id: TenantId,
    pub sandbox_id: SandboxId,
    pub receipts: Vec<MachinePortForwardReceipt>,
}

struct MachinePortEvidenceExpectation<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    bindings: &'a [SandboxPortBinding],
    provider_instance: &'a NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
}

impl<'a> MachinePortEvidenceExpectation<'a> {
    fn from_manifest(
        manifest: &'a ContainerSandboxManifest,
    ) -> Result<MachinePortEvidenceExpectation<'a>> {
        let forwarder = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {} has no machine forwarder authority; observed machine \
                     port evidence is unavailable",
                    manifest.handle.id
                ),
            })?;
        Ok(Self::new(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            forwarder,
        ))
    }

    fn new(
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        bindings: &'a [SandboxPortBinding],
        forwarder: &'a OciMachinePortForwarderConfig,
    ) -> Self {
        Self {
            tenant_id,
            sandbox_id,
            bindings,
            provider_instance: forwarder.provider_instance(),
            provider_generation: forwarder.provider_generation(),
        }
    }

    fn validate(
        &self,
        phase: MachinePortEvidencePhase,
        receipts: &[MachinePortForwardReceipt],
    ) -> Result<()> {
        if receipts.len() != self.bindings.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port evidence for tenant {} sandbox {} is partial: {} receipts do \
                     not cover {} desired bindings",
                    self.tenant_id,
                    self.sandbox_id,
                    receipts.len(),
                    self.bindings.len()
                ),
            });
        }
        for (index, (receipt, binding)) in receipts.iter().zip(self.bindings.iter()).enumerate() {
            if receipt.tenant_id != *self.tenant_id
                || receipt.sandbox_id != *self.sandbox_id
                || receipt.binding != *binding
                || receipt.provider_instance != *self.provider_instance
                || receipt.provider_generation != self.provider_generation
                || !phase.accepts(receipt.outcome)
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "machine port evidence member {index} does not authenticate the exact \
                         tenant, sandbox, provider generation, canonical binding order, and \
                         {phase:?} outcome for tenant {} sandbox {}; the batch remains fenced",
                        self.tenant_id, self.sandbox_id
                    ),
                });
            }
        }
        Ok(())
    }
}

impl ContainerSandboxBackend {
    pub(super) fn persist_exposed_machine_port_receipts(
        &self,
        manifest: &ContainerSandboxManifest,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> Result<()> {
        self.persist_machine_port_receipts(manifest, MachinePortEvidencePhase::Exposed, receipts)
    }

    pub(super) fn persist_absent_machine_port_receipts(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        forwarder: &OciMachinePortForwarderConfig,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> Result<()> {
        let manifest =
            self.read_manifest(sandbox_id)?
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "cannot persist machine port absence evidence because sandbox manifest \
                     {sandbox_id} is missing"
                    ),
                })?;
        if manifest.spec.tenant_id != *tenant_id || manifest.spec.port_bindings != bindings {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port absence evidence does not match the current durable manifest \
                     for tenant {tenant_id} sandbox {sandbox_id}"
                ),
            });
        }
        let expectation = MachinePortEvidenceExpectation::from_manifest(&manifest)?;
        authenticate_forwarder_authority(&expectation, forwarder)?;
        expectation.validate(MachinePortEvidencePhase::Absent, &receipts)?;
        publish_record(
            &self.config.workload_state_root,
            &manifest.conmon_layout.container_state_dir,
            record(MachinePortEvidencePhase::Absent, expectation, receipts),
        )
    }

    /// Read the exact complete exposed receipt batch without mutating provider
    /// or lease authority.
    pub fn exposed_machine_port_receipts(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        self.read_machine_port_receipts(sandbox_id, MachinePortEvidencePhase::Exposed)
    }

    /// Read the exact complete withdrawn/already-absent receipt batch without
    /// mutating provider or lease authority.
    pub fn absent_machine_port_receipts(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        self.read_machine_port_receipts(sandbox_id, MachinePortEvidencePhase::Absent)
    }

    /// Read exact durable absence, including header identity for an empty
    /// binding set. Missing evidence is distinct from malformed or stale
    /// evidence so retry callers preserve not-found semantics.
    pub fn absent_machine_port_evidence(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Option<MachinePortAbsenceEvidence>> {
        let forwarder = self.config.machine_port_forwarder.as_ref().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {sandbox_id} has no machine forwarder authority; \
                         observed machine port evidence is unavailable"
                ),
            }
        })?;
        let record = match self.read_manifest(sandbox_id)? {
            Some(manifest) => {
                let expectation = MachinePortEvidenceExpectation::from_manifest(&manifest)?;
                authenticate_forwarder_authority(&expectation, forwarder)?;
                let record = read_record(&manifest.conmon_layout.container_state_dir)?;
                authenticate_record(&record, MachinePortEvidencePhase::Absent, &expectation)?;
                record
            }
            None => {
                let Some(record) =
                    find_record_without_manifest(&self.config.workload_state_root, sandbox_id)?
                else {
                    return Ok(None);
                };
                authenticate_detached_record(
                    &record,
                    MachinePortEvidencePhase::Absent,
                    sandbox_id,
                    forwarder,
                )?;
                record
            }
        };
        Ok(Some(MachinePortAbsenceEvidence {
            tenant_id: record.tenant_id,
            sandbox_id: record.sandbox_id,
            receipts: record.receipts,
        }))
    }

    /// Side-effect-free fence used before any provider withdrawal attempt.
    pub(super) fn authenticate_machine_port_forwarder(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        forwarder: &OciMachinePortForwarderConfig,
    ) -> Result<()> {
        let manifest =
            self.read_manifest(sandbox_id)?
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "cannot authenticate machine port forwarder because sandbox manifest \
                     {sandbox_id} is missing"
                    ),
                })?;
        if manifest.spec.tenant_id != *tenant_id || manifest.spec.port_bindings != bindings {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port forwarder authority does not match the current durable manifest \
                     for tenant {tenant_id} sandbox {sandbox_id}"
                ),
            });
        }
        let expectation = MachinePortEvidenceExpectation::from_manifest(&manifest)?;
        authenticate_forwarder_authority(&expectation, forwarder)
    }

    fn persist_machine_port_receipts(
        &self,
        manifest: &ContainerSandboxManifest,
        phase: MachinePortEvidencePhase,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> Result<()> {
        self.validate_manifest_execution_context(manifest)?;
        let expectation = MachinePortEvidenceExpectation::from_manifest(manifest)?;
        expectation.validate(phase, &receipts)?;
        publish_record(
            &self.config.workload_state_root,
            &manifest.conmon_layout.container_state_dir,
            record(phase, expectation, receipts),
        )
    }

    fn read_machine_port_receipts(
        &self,
        sandbox_id: &SandboxId,
        phase: MachinePortEvidencePhase,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        let manifest = self.read_manifest(sandbox_id)?.ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "cannot read machine port evidence because sandbox manifest {sandbox_id} is \
                     missing"
                ),
            }
        })?;
        let expectation = MachinePortEvidenceExpectation::from_manifest(&manifest)?;
        let record = read_record(&manifest.conmon_layout.container_state_dir)?;
        authenticate_record(&record, phase, &expectation)?;
        Ok(record.receipts)
    }
}

fn authenticate_forwarder_authority(
    expectation: &MachinePortEvidenceExpectation<'_>,
    forwarder: &OciMachinePortForwarderConfig,
) -> Result<()> {
    if forwarder.provider_instance() != expectation.provider_instance
        || forwarder.provider_generation() != expectation.provider_generation
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "machine port forwarder authority is crossed or stale for tenant {} sandbox {}; \
                 provider effects and evidence publication remain fenced",
                expectation.tenant_id, expectation.sandbox_id
            ),
        });
    }
    Ok(())
}

fn authenticate_record(
    record: &MachinePortEvidenceRecord,
    phase: MachinePortEvidencePhase,
    expectation: &MachinePortEvidenceExpectation<'_>,
) -> Result<()> {
    if record.version != CURRENT_MACHINE_PORT_EVIDENCE_VERSION || record.phase != phase {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "machine port evidence for tenant {} sandbox {} is not a complete {phase:?} \
                 batch at the current version",
                expectation.tenant_id, expectation.sandbox_id
            ),
        });
    }
    if record.tenant_id != *expectation.tenant_id
        || record.sandbox_id != *expectation.sandbox_id
        || record.provider_instance != *expectation.provider_instance
        || record.provider_generation != expectation.provider_generation
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "machine port evidence identity does not match the current durable manifest for \
                 tenant {} sandbox {}; the observation remains fenced",
                expectation.tenant_id, expectation.sandbox_id
            ),
        });
    }
    expectation.validate(phase, &record.receipts)
}

fn authenticate_detached_record(
    record: &MachinePortEvidenceRecord,
    phase: MachinePortEvidencePhase,
    sandbox_id: &SandboxId,
    forwarder: &OciMachinePortForwarderConfig,
) -> Result<()> {
    if record.version != CURRENT_MACHINE_PORT_EVIDENCE_VERSION
        || record.phase != phase
        || record.sandbox_id != *sandbox_id
        || record.provider_instance != *forwarder.provider_instance()
        || record.provider_generation != forwarder.provider_generation()
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "detached machine port evidence does not authenticate the exact sandbox and \
                 current provider generation for {sandbox_id}; retry remains fenced"
            ),
        });
    }
    for (index, receipt) in record.receipts.iter().enumerate() {
        if receipt.tenant_id != record.tenant_id
            || receipt.sandbox_id != record.sandbox_id
            || receipt.provider_instance != record.provider_instance
            || receipt.provider_generation != record.provider_generation
            || !phase.accepts(receipt.outcome)
            || record.receipts[..index]
                .iter()
                .any(|prior| prior.binding == receipt.binding)
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "detached machine port evidence member {index} is crossed, stale, or duplicate \
                     for tenant {} sandbox {}; retry remains fenced",
                    record.tenant_id, record.sandbox_id
                ),
            });
        }
    }
    Ok(())
}

fn find_record_without_manifest(
    state_root: &Path,
    sandbox_id: &SandboxId,
) -> Result<Option<MachinePortEvidenceRecord>> {
    let state_dirs =
        crate::artifact_paths::all_container_state_dirs(state_root).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to find detached machine port evidence for {sandbox_id} under {}: \
                     {error}",
                    state_root.display()
                ),
            }
        })?;
    let mut selected = None;
    for state_dir in state_dirs {
        if state_dir.file_name() != Some(std::ffi::OsStr::new(sandbox_id.as_str())) {
            continue;
        }
        let Some(record) = read_record_if_present(&state_dir)? else {
            continue;
        };
        if selected.is_some() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "detached machine port evidence for sandbox {sandbox_id} exists in multiple \
                     tenant roots; retry remains fenced"
                ),
            });
        }
        selected = Some(record);
    }
    Ok(selected)
}

fn record(
    phase: MachinePortEvidencePhase,
    expectation: MachinePortEvidenceExpectation<'_>,
    receipts: Vec<MachinePortForwardReceipt>,
) -> MachinePortEvidenceRecord {
    MachinePortEvidenceRecord {
        version: CURRENT_MACHINE_PORT_EVIDENCE_VERSION,
        phase,
        tenant_id: expectation.tenant_id.clone(),
        sandbox_id: expectation.sandbox_id.clone(),
        provider_instance: expectation.provider_instance.clone(),
        provider_generation: expectation.provider_generation,
        receipts,
    }
}

fn publish_record(
    state_root: &Path,
    state_dir: &Path,
    record: MachinePortEvidenceRecord,
) -> Result<()> {
    let mut rendered =
        serde_json::to_vec_pretty(&record).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize machine port evidence: {error}"),
        })?;
    rendered.push(b'\n');
    crate::backends::oci::durable_directory::establish_durable_directory_chain_with(
        state_root,
        state_dir,
        "machine port evidence",
        sync_directory,
    )?;
    let _guard = lock_publication(state_dir)?;
    remove_stale_stage(state_dir)?;
    let stage_path = state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE);
    let evidence_path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    let publication = (|| -> Result<()> {
        let mut stage = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create staged machine port evidence {}: {error}",
                    stage_path.display()
                ),
            })?;
        stage
            .write_all(&rendered)
            .and_then(|()| stage.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably stage machine port evidence {}: {error}",
                    stage_path.display()
                ),
            })?;
        fs::rename(&stage_path, &evidence_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to atomically publish machine port evidence {}: {error}",
                evidence_path.display()
            ),
        })?;
        sync_directory(state_dir).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "machine port evidence {} reached its commit point but directory sync failed; \
                 publication outcome is ambiguous: {error}",
                evidence_path.display()
            ),
        })
    })();
    if let Err(primary) = publication {
        let cleanup = match remove_regular_file_if_present(&stage_path) {
            Ok(true) => sync_directory(state_dir).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably clean staged machine port evidence {}: {error}",
                    stage_path.display()
                ),
            }),
            Ok(false) => Ok(()),
            Err(error) => Err(error),
        };
        return match cleanup {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(SandboxError::OperationFailed {
                message: format!(
                    "{primary}; staged machine port evidence cleanup also failed: {cleanup}"
                ),
            }),
        };
    }
    Ok(())
}

fn read_record(state_dir: &Path) -> Result<MachinePortEvidenceRecord> {
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
    serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to parse strict machine port evidence {}: {error}",
            path.display()
        ),
    })
}

fn read_record_if_present(state_dir: &Path) -> Result<Option<MachinePortEvidenceRecord>> {
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

fn lock_publication(state_dir: &Path) -> Result<MachinePortEvidenceGuard> {
    let path = state_dir.join(MACHINE_PORT_EVIDENCE_LOCK_FILE);
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && !metadata.file_type().is_file()
    {
        return Err(non_regular_entry(&path));
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open machine port evidence lock {}: {error}",
                path.display()
            ),
        })?;
    if !lock
        .metadata()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to inspect machine port evidence lock {}: {error}",
                path.display()
            ),
        })?
        .is_file()
    {
        return Err(non_regular_entry(&path));
    }
    let deadline = Instant::now() + MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT;
    loop {
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) => return Ok(MachinePortEvidenceGuard { _lock: lock }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "timed out acquiring machine port evidence lock {}; canonical \
                             observation remains unchanged",
                            path.display()
                        ),
                    });
                }
                thread::sleep(MACHINE_PORT_EVIDENCE_LOCK_RETRY);
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to acquire machine port evidence lock {}: {error}",
                        path.display()
                    ),
                });
            }
        }
    }
}

fn remove_stale_stage(state_dir: &Path) -> Result<()> {
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

fn sync_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

struct MachinePortEvidenceGuard {
    _lock: File,
}

#[cfg(test)]
mod tests;
