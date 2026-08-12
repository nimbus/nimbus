//! Durable parent-host intent for machine-backed port publication.
//!
//! The host-global port-lease store remains the sole reservation and provider
//! lifecycle authority. This sibling store records only the parent adapter's
//! exact Machine API intent and its write barrier: `Staged` proves no request
//! byte was sent, while `Committed` means the remote outcome may be
//! ambiguous. That distinction makes crash recovery conservative without
//! teaching `nimbus-network` about machines or transports.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::net::{IpAddr, Ipv6Addr};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use nimbus::{Error, SandboxId, SandboxPortBinding, TenantId};
use nimbus_machine::MachineForwarderAuthority;
use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkPlanId, NetworkProviderHandle,
    NetworkResourceId, PortBindRealm, PortBindTarget, PortBindingSpec, PortExposure,
    PortIpv6Overlap, PortLeaseAccounting, PortLeaseFence, PortLeasePhase, PortLeaseRecoveryGuard,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

const STORE_DIRECTORY: &str = "machine-publications";
const STATE_FILE: &str = "intents.json";
const LOCK_FILE: &str = "authority.lock";
const STAGE_FILE: &str = ".intents.stage";
const FORMAT_MAGIC: &str = "nimbus-machine-publication-intents";
const FORMAT_VERSION: u32 = 1;
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_RETRY: Duration = Duration::from_millis(10);
#[cfg(unix)]
const OWNER_DIRECTORY_MODE: u32 = 0o700;
#[cfg(unix)]
const OWNER_FILE_MODE: u32 = 0o600;

mod confirmed;

pub(crate) use confirmed::{
    ConfirmedMachineDesireAdmissionGuard, ConfirmedMachinePublicationJournal,
    ConfirmedMachinePublicationMember, ConfirmedMachinePublicationObservation,
    ConfirmedMachinePublicationRetirement, ConfirmedMachinePublicationRetirementPhase,
    ConfirmedMachineStopBarrierAuthority, canonical_machine_publication_members,
    canonical_machine_restart_publication_members,
};

#[derive(Clone)]
pub(super) struct MachinePublicationIntentStore {
    root: PathBuf,
    state_path: PathBuf,
    lock_path: PathBuf,
    stage_path: PathBuf,
}

impl MachinePublicationIntentStore {
    pub(super) fn open(parent_network_state_root: &Path) -> Result<Self, Error> {
        let root = parent_network_state_root
            .join("networks")
            .join(STORE_DIRECTORY);
        create_owner_directory(&root)?;
        let store = Self {
            state_path: root.join(STATE_FILE),
            lock_path: root.join(LOCK_FILE),
            stage_path: root.join(STAGE_FILE),
            root,
        };
        store.with_locked_body(|_| Ok(()))?;
        Ok(store)
    }

    /// Return the existing nonterminal service attempt, or durably stage a new
    /// attempt before any port reservation or Machine API I/O may occur.
    #[cfg(test)]
    pub(super) fn stage_service_attempt(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        authority: &MachineForwarderAuthority,
        bindings: &[SandboxPortBinding],
    ) -> Result<MachinePublicationIntent, Error> {
        if service_name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "machine publication service identity must not be empty".to_owned(),
            ));
        }
        self.mutate(|body| {
            let existing = body
                .intents
                .iter()
                .filter(|intent| {
                    intent.tenant_id == *tenant_id
                        && intent.service_name == service_name
                        && !intent.phase.is_terminal()
                })
                .collect::<Vec<_>>();
            if existing.len() > 1 {
                return Err(corrupt_error(format!(
                    "tenant {} service {} has {} nonterminal publication attempts",
                    tenant_id,
                    service_name,
                    existing.len()
                )));
            }
            if let Some(existing) = existing.first() {
                if existing.forwarder_authority != *authority || existing.bindings != bindings {
                    return Err(Error::conflict(format!(
                        "tenant {tenant_id} service {service_name} already has a fenced machine \
                         publication attempt with different provider generation or bindings"
                    )));
                }
                return Ok((*existing).clone());
            }

            let ordinal = body
                .intents
                .iter()
                .filter(|intent| {
                    intent.tenant_id == *tenant_id && intent.service_name == service_name
                })
                .count()
                .checked_add(1)
                .ok_or_else(|| {
                    Error::ResourceExhausted(
                        "machine publication test-fixture ordinal overflowed".to_owned(),
                    )
                })?;
            let attempt_id = Ulid::from(ordinal as u128).to_string().to_ascii_lowercase();
            let workload_incarnation_key = format!("{service_name}:{attempt_id}");
            let plan_id =
                NetworkPlanId::for_tenant_workload_plan(tenant_id, &workload_incarnation_key);
            let intent = MachinePublicationIntent {
                plan_id: plan_id.clone(),
                sandbox_id: SandboxId::new(format!("machine-api:{plan_id}")),
                tenant_id: tenant_id.clone(),
                service_name: service_name.to_owned(),
                attempt_id,
                forwarder_authority: authority.clone(),
                bindings: bindings.to_vec(),
                phase: MachinePublicationIntentPhase::Staged,
            };
            body.intents.push(intent.clone());
            body.intents
                .sort_by(|left, right| left.plan_id.cmp(&right.plan_id));
            Ok(intent)
        })
    }

    /// Cross the durable request barrier after the complete port batch is
    /// reserved. Once committed, recovery must assume Machine API I/O may have
    /// occurred even if the caller crashes before its first write.
    #[cfg(test)]
    pub(super) fn commit_before_machine_api(
        &self,
        plan_id: &NetworkPlanId,
    ) -> Result<MachinePublicationIntent, Error> {
        self.mutate(|body| {
            let intent = exact_intent_mut(body, plan_id)?;
            match intent.phase {
                MachinePublicationIntentPhase::Staged => {
                    intent.phase = MachinePublicationIntentPhase::Committed;
                }
                MachinePublicationIntentPhase::Committed => {}
                MachinePublicationIntentPhase::Terminal => {
                    return Err(Error::conflict(format!(
                        "machine publication plan {plan_id} is already terminal"
                    )));
                }
            }
            Ok(intent.clone())
        })
    }

    pub(super) fn mark_terminal(
        &self,
        plan_id: &NetworkPlanId,
    ) -> Result<MachinePublicationIntent, Error> {
        self.mutate(|body| {
            let intent = exact_intent_mut(body, plan_id)?;
            intent.phase = MachinePublicationIntentPhase::Terminal;
            Ok(intent.clone())
        })
    }

    /// Read one exact durable attempt without changing store bytes or phase.
    #[cfg(test)]
    pub(super) fn load_plan(
        &self,
        plan_id: &NetworkPlanId,
    ) -> Result<Option<MachinePublicationIntent>, Error> {
        self.with_locked_body(|body| {
            Ok(body
                .intents
                .iter()
                .find(|intent| intent.plan_id == *plan_id)
                .cloned())
        })
    }

    pub(super) fn nonterminal_for_authority(
        &self,
        authority: &MachineForwarderAuthority,
    ) -> Result<Vec<MachinePublicationIntent>, Error> {
        self.with_locked_body(|body| {
            Ok(body
                .intents
                .iter()
                .filter(|intent| {
                    !intent.phase.is_terminal() && intent.forwarder_authority == *authority
                })
                .cloned()
                .collect())
        })
    }

    pub(super) fn nonterminal_for_provider(
        &self,
        provider_instance: &NetworkProviderHandle,
    ) -> Result<Vec<MachinePublicationIntent>, Error> {
        self.with_locked_body(|body| {
            Ok(body
                .intents
                .iter()
                .filter(|intent| {
                    !intent.phase.is_terminal()
                        && intent.forwarder_authority.provider_instance() == provider_instance
                })
                .cloned()
                .collect())
        })
    }

    fn mutate<T>(
        &self,
        operation: impl FnOnce(&mut MachinePublicationIntentBody) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let lock = self.acquire_lock()?;
        self.validate_directory_entries()?;
        remove_file_if_exists(&self.stage_path)?;
        let envelope = self.load_envelope()?;
        let mut body = envelope.body.clone();
        validate_body(&body)?;
        let output = operation(&mut body)?;
        validate_body(&body)?;
        if body != envelope.body {
            let revision = envelope.revision.checked_add(1).ok_or_else(|| {
                corrupt_error("machine publication intent revision exhausted".to_owned())
            })?;
            self.publish(revision, &body)?;
        }
        drop(lock);
        Ok(output)
    }

    fn with_locked_body<T>(
        &self,
        operation: impl FnOnce(&MachinePublicationIntentBody) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let lock = self.acquire_lock()?;
        self.validate_directory_entries()?;
        remove_file_if_exists(&self.stage_path)?;
        let envelope = self.load_envelope()?;
        validate_body(&envelope.body)?;
        let output = operation(&envelope.body)?;
        drop(lock);
        Ok(output)
    }

    fn acquire_lock(&self) -> Result<MachinePublicationStoreLock, Error> {
        let file = open_owner_file(&self.lock_path, true)?;
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(MachinePublicationStoreLock { file }),
                Err(error) if lock_is_contended(&error) && Instant::now() < deadline => {
                    thread::sleep(LOCK_RETRY);
                }
                Err(error) if lock_is_contended(&error) => {
                    return Err(Error::ResourceExhausted(format!(
                        "timed out acquiring machine publication authority lock {}",
                        self.lock_path.display()
                    )));
                }
                Err(error) => {
                    return Err(io_error(
                        "lock machine publication authority",
                        &self.lock_path,
                        error,
                    ));
                }
            }
        }
    }

    fn validate_directory_entries(&self) -> Result<(), Error> {
        for entry in fs::read_dir(&self.root).map_err(|error| {
            io_error(
                "read machine publication authority directory",
                &self.root,
                error,
            )
        })? {
            let entry = entry.map_err(|error| {
                io_error(
                    "read machine publication authority entry",
                    &self.root,
                    error,
                )
            })?;
            let name = entry.file_name();
            if name != STATE_FILE && name != LOCK_FILE && name != STAGE_FILE {
                return Err(corrupt_error(format!(
                    "machine publication authority directory {} contains an unknown entry",
                    self.root.display()
                )));
            }
        }
        Ok(())
    }

    fn load_envelope(&self) -> Result<MachinePublicationIntentEnvelope, Error> {
        let bytes = match fs::read(&self.state_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(MachinePublicationIntentEnvelope::empty());
            }
            Err(error) => {
                return Err(io_error(
                    "read machine publication authority",
                    &self.state_path,
                    error,
                ));
            }
        };
        let envelope: MachinePublicationIntentEnvelope =
            serde_json::from_slice(&bytes).map_err(|error| {
                corrupt_error(format!(
                    "machine publication authority {} is not a strict envelope: {error}",
                    self.state_path.display()
                ))
            })?;
        envelope.validate(&self.state_path)?;
        Ok(envelope)
    }

    fn publish(&self, revision: u64, body: &MachinePublicationIntentBody) -> Result<(), Error> {
        let envelope = MachinePublicationIntentEnvelope::new(revision, body.clone())?;
        let bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| {
            Error::Internal(format!(
                "failed to encode machine publication authority: {error}"
            ))
        })?;
        remove_file_if_exists(&self.stage_path)?;
        let mut stage = open_owner_file(&self.stage_path, false)?;
        stage.write_all(&bytes).map_err(|error| {
            io_error(
                "write machine publication authority stage",
                &self.stage_path,
                error,
            )
        })?;
        stage.sync_all().map_err(|error| {
            io_error(
                "sync machine publication authority stage",
                &self.stage_path,
                error,
            )
        })?;
        fs::rename(&self.stage_path, &self.state_path).map_err(|error| {
            io_error(
                "replace machine publication authority",
                &self.state_path,
                error,
            )
        })?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                io_error(
                    "sync machine publication authority directory",
                    &self.root,
                    error,
                )
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MachinePublicationIntent {
    pub(super) plan_id: NetworkPlanId,
    pub(super) sandbox_id: SandboxId,
    pub(super) tenant_id: TenantId,
    pub(super) service_name: String,
    attempt_id: String,
    pub(super) forwarder_authority: MachineForwarderAuthority,
    pub(super) bindings: Vec<SandboxPortBinding>,
    pub(super) phase: MachinePublicationIntentPhase,
}

impl MachinePublicationIntent {
    pub(super) fn requests(&self) -> Result<Vec<PortLeaseRequest>, Error> {
        self.bindings
            .iter()
            .map(|binding| self.request(binding))
            .collect()
    }

    fn request(&self, binding: &SandboxPortBinding) -> Result<PortLeaseRequest, Error> {
        let host_port = NonZeroU16::new(binding.host_port).ok_or_else(|| {
            Error::InvalidInput(format!(
                "machine publication {} binding {} requests host port zero",
                self.plan_id, binding.name
            ))
        })?;
        let listener = ListenerId::for_tenant_workload_listener(
            &self.tenant_id,
            self.sandbox_id.as_str(),
            &binding.name,
        );
        Ok(PortLeaseRequest::new(
            nimbus_network::PortLeaseId::for_listener(&listener),
            NetworkResourceId::Listener(listener),
            Some(self.tenant_id.clone()),
            PortLeaseFence::new(
                self.forwarder_authority.generation(),
                NetworkLeaseEpoch::new(1),
            ),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(binding.host_address),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                machine_host_bind_target(binding.host_address)?,
                machine_host_exposure(binding.host_address),
                PortRequestMode::Exact(host_port),
            ),
        )
        .with_plan_id(self.plan_id.clone()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MachinePublicationIntentPhase {
    Staged,
    Committed,
    Terminal,
}

impl MachinePublicationIntentPhase {
    pub(super) const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachinePublicationIntentBody {
    intents: Vec<MachinePublicationIntent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachinePublicationIntentEnvelope {
    magic: String,
    format_version: u32,
    revision: u64,
    checksum: String,
    body: MachinePublicationIntentBody,
}

impl MachinePublicationIntentEnvelope {
    fn empty() -> Self {
        Self {
            magic: FORMAT_MAGIC.to_owned(),
            format_version: FORMAT_VERSION,
            revision: 0,
            checksum: checksum(&MachinePublicationIntentBody::default())
                .expect("default publication intent body is serializable"),
            body: MachinePublicationIntentBody::default(),
        }
    }

    fn new(revision: u64, body: MachinePublicationIntentBody) -> Result<Self, Error> {
        Ok(Self {
            magic: FORMAT_MAGIC.to_owned(),
            format_version: FORMAT_VERSION,
            revision,
            checksum: checksum(&body)?,
            body,
        })
    }

    fn validate(&self, path: &Path) -> Result<(), Error> {
        if self.magic != FORMAT_MAGIC || self.format_version != FORMAT_VERSION {
            return Err(corrupt_error(format!(
                "machine publication authority {} has unsupported format identity",
                path.display()
            )));
        }
        if self.checksum != checksum(&self.body)? {
            return Err(corrupt_error(format!(
                "machine publication authority {} failed checksum validation",
                path.display()
            )));
        }
        Ok(())
    }
}

fn validate_body(body: &MachinePublicationIntentBody) -> Result<(), Error> {
    for (index, intent) in body.intents.iter().enumerate() {
        let expected_sandbox_id = format!("machine-api:{}", intent.plan_id);
        if intent.sandbox_id.as_str() != expected_sandbox_id
            || intent.service_name.trim().is_empty()
            || Ulid::from_string(&intent.attempt_id).is_err()
        {
            return Err(corrupt_error(format!(
                "machine publication intent member {index} has invalid stable identity"
            )));
        }
        if body.intents[..index]
            .iter()
            .any(|existing| existing.plan_id == intent.plan_id)
        {
            return Err(corrupt_error(format!(
                "machine publication plan {} is duplicated",
                intent.plan_id
            )));
        }
    }
    Ok(())
}

fn exact_intent_mut<'a>(
    body: &'a mut MachinePublicationIntentBody,
    plan_id: &NetworkPlanId,
) -> Result<&'a mut MachinePublicationIntent, Error> {
    body.intents
        .iter_mut()
        .find(|intent| intent.plan_id == *plan_id)
        .ok_or_else(|| {
            Error::NotFound(format!(
                "machine publication plan {plan_id} is not durably recorded"
            ))
        })
}

fn checksum(body: &MachinePublicationIntentBody) -> Result<String, Error> {
    let bytes = serde_json::to_vec(body).map_err(|error| {
        Error::Internal(format!(
            "failed to encode machine publication checksum body: {error}"
        ))
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn create_owner_directory(path: &Path) -> Result<(), Error> {
    fs::create_dir_all(path)
        .map_err(|error| io_error("create machine publication authority", path, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(OWNER_DIRECTORY_MODE))
            .map_err(|error| io_error("secure machine publication authority", path, error))?;
    }
    Ok(())
}

fn open_owner_file(path: &Path, preserve: bool) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    if !preserve {
        options.truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(OWNER_FILE_MODE);
    }
    let file = options
        .open(path)
        .map_err(|error| io_error("open machine publication authority file", path, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(OWNER_FILE_MODE))
            .map_err(|error| io_error("secure machine publication authority file", path, error))?;
    }
    Ok(file)
}

fn remove_file_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(
            "remove stale machine publication stage",
            path,
            error,
        )),
    }
}

fn lock_is_contended(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::PermissionDenied
    ) || error.raw_os_error() == Some(33)
}

fn io_error(operation: &str, path: &Path, source: io::Error) -> Error {
    Error::Internal(format!("{operation} {}: {source}", path.display()))
}

fn corrupt_error(reason: String) -> Error {
    Error::PreconditionFailed(reason)
}

pub(super) struct MachinePublicationCleanup {
    store: MachinePublicationIntentStore,
    authority: LocalPortLeaseAuthority,
    plans: Vec<MachinePublicationPlanCleanup>,
}

impl MachinePublicationCleanup {
    pub(super) fn release_after_confirmed_provider_stop(self) -> Result<(), Error> {
        for plan in self.plans {
            if !plan.requests.is_empty() {
                self.authority
                    .release_provider_managed_batch_after_confirmed_stop(
                        &plan.requests,
                        &plan.recoveries,
                    )
                    .map_err(port_authority_error)?;
            }
            self.store.mark_terminal(&plan.intent.plan_id)?;
        }
        Ok(())
    }
}

struct MachinePublicationPlanCleanup {
    intent: MachinePublicationIntent,
    requests: Vec<PortLeaseRequest>,
    recoveries: Vec<PortLeaseRecoveryGuard>,
}

pub(super) fn withdraw_machine_publications(
    store: MachinePublicationIntentStore,
    authority: LocalPortLeaseAuthority,
    forwarder_authority: &MachineForwarderAuthority,
) -> Result<MachinePublicationCleanup, Error> {
    let intents = store.nonterminal_for_authority(forwarder_authority)?;
    let mut prepared = Vec::with_capacity(intents.len());
    for intent in intents {
        let requests = intent.requests()?;
        let records = authority
            .list_plan(&intent.plan_id)
            .map_err(port_authority_error)?;
        if records.is_empty() {
            if intent.phase == MachinePublicationIntentPhase::Staged {
                store.mark_terminal(&intent.plan_id)?;
                continue;
            }
            if !intent.bindings.is_empty() {
                return Err(Error::PreconditionFailed(format!(
                    "machine publication plan {} has durable intent but no parent lease batch",
                    intent.plan_id
                )));
            }
            prepared.push(MachinePublicationPlanCleanup {
                intent,
                requests,
                recoveries: Vec::new(),
            });
            continue;
        }
        authenticate_exact_durable_plan(&requests, &records)?;
        if records.iter().all(|record| {
            matches!(
                record.phase(),
                PortLeasePhase::Released | PortLeasePhase::Failed
            )
        }) {
            store.mark_terminal(&intent.plan_id)?;
            continue;
        }
        let recoveries = recover_dead_batch(&authority, &requests)?;
        prepared.push(MachinePublicationPlanCleanup {
            intent,
            requests,
            recoveries,
        });
    }
    for plan in &prepared {
        if !plan.requests.is_empty() {
            authority
                .mark_cleanup_pending_batch_after_owner_death(&plan.requests, &plan.recoveries)
                .map_err(port_authority_error)?;
        }
    }
    Ok(MachinePublicationCleanup {
        store,
        authority,
        plans: prepared,
    })
}

pub(super) fn ensure_no_fenced_machine_publications(
    store: &MachinePublicationIntentStore,
    provider_instance: &NetworkProviderHandle,
) -> Result<(), Error> {
    let intents = store.nonterminal_for_provider(provider_instance)?;
    if intents.is_empty() {
        return Ok(());
    }
    let plans = intents
        .iter()
        .map(|intent| intent.plan_id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(Error::conflict(format!(
        "machine forwarder provider {} retains nonterminal parent publication plans [{plans}]; \
         reconcile exact provider absence before starting a new generation",
        provider_instance
    )))
}

pub(super) fn authenticate_exact_durable_plan(
    expected: &[PortLeaseRequest],
    records: &[nimbus_network::PortLeaseRecord],
) -> Result<(), Error> {
    let expected = expected
        .iter()
        .map(|request| (request.lease_id(), request))
        .collect::<std::collections::BTreeMap<_, _>>();
    let durable = records
        .iter()
        .map(|record| (record.request().lease_id(), record.request()))
        .collect::<std::collections::BTreeMap<_, _>>();
    if expected != durable {
        return Err(Error::PreconditionFailed(
            "durable machine publication membership does not match its exact parent intent"
                .to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn recover_dead_batch(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
) -> Result<Vec<PortLeaseRecoveryGuard>, Error> {
    authority
        .recover_dead_lifetimes(requests)
        .map_err(port_authority_error)
}

pub(super) fn port_authority_error(error: impl std::fmt::Display) -> Error {
    Error::PreconditionFailed(format!(
        "parent machine publication authority rejected: {error}"
    ))
}

pub(super) fn machine_host_bind_target(address: IpAddr) -> Result<PortBindTarget, Error> {
    match address {
        IpAddr::V4(address) if address.is_unspecified() => Ok(PortBindTarget::ipv4_wildcard()),
        IpAddr::V4(address) => Ok(PortBindTarget::ipv4_specific(address)),
        IpAddr::V6(address) if address.is_unspecified() => {
            Ok(PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown))
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "machine publication uses an invalid IPv6 host address: {error}"
                ))
            }),
    }
}

pub(super) fn machine_host_exposure(address: IpAddr) -> PortExposure {
    if address.is_loopback() {
        return PortExposure::Loopback;
    }
    match address {
        IpAddr::V4(address) if address.is_private() => PortExposure::Private,
        IpAddr::V6(address) if ipv6_is_unique_local(address) => PortExposure::Private,
        _ => PortExposure::Public,
    }
}

fn ipv6_is_unique_local(address: Ipv6Addr) -> bool {
    address.octets()[0] & 0xfe == 0xfc
}

struct MachinePublicationStoreLock {
    file: File,
}

impl Drop for MachinePublicationStoreLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use nimbus_network::{NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn intent_write_barrier_replays_and_replaces_only_after_terminal() {
        let temp = TempDir::new().expect("temporary root should exist");
        let store = MachinePublicationIntentStore::open(temp.path())
            .expect("publication store should open");
        let tenant = tenant();
        let authority = authority();
        let bindings = [SandboxPortBinding::tcp("http", 18_080, 8_080)];

        let staged = store
            .stage_service_attempt(&tenant, "api", &authority, &bindings)
            .expect("attempt should stage");
        assert_eq!(staged.phase, MachinePublicationIntentPhase::Staged);
        let replay = store
            .stage_service_attempt(&tenant, "api", &authority, &bindings)
            .expect("exact staged attempt should replay");
        assert_eq!(replay, staged);

        let committed = store
            .commit_before_machine_api(&staged.plan_id)
            .expect("barrier should commit");
        assert_eq!(committed.phase, MachinePublicationIntentPhase::Committed);
        assert_eq!(
            store
                .stage_service_attempt(&tenant, "api", &authority, &bindings)
                .expect("committed attempt should remain the sole replay")
                .plan_id,
            staged.plan_id
        );

        store
            .mark_terminal(&staged.plan_id)
            .expect("exact stop should make the attempt terminal");
        let replacement = store
            .stage_service_attempt(&tenant, "api", &authority, &bindings)
            .expect("terminal attempt should permit a new incarnation");
        assert_ne!(replacement.plan_id, staged.plan_id);
        assert_eq!(replacement.tenant_id, staged.tenant_id);
        assert_eq!(replacement.service_name, staged.service_name);
    }

    #[test]
    fn checksum_corruption_fails_closed_without_rewriting_authority() {
        let temp = TempDir::new().expect("temporary root should exist");
        let store = MachinePublicationIntentStore::open(temp.path())
            .expect("publication store should open");
        let intent = store
            .stage_service_attempt(
                &tenant(),
                "api",
                &authority(),
                &[SandboxPortBinding::tcp("http", 18_080, 8_080)],
            )
            .expect("attempt should stage");
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(&store.state_path).expect("state should read"))
                .expect("state should decode");
        envelope["body"]["intents"][0]["service_name"] = serde_json::json!("tampered");
        let tampered = serde_json::to_vec_pretty(&envelope).expect("tamper should encode");
        fs::write(&store.state_path, &tampered).expect("tamper should write");

        let error = store
            .load_plan(&intent.plan_id)
            .expect_err("checksum mismatch must fail closed");

        assert!(error.to_string().contains("checksum"), "{error}");
        assert_eq!(
            fs::read(&store.state_path).expect("tampered state should remain"),
            tampered,
            "a failed read must not repair or rewrite corrupt authority"
        );
    }

    #[test]
    fn concurrent_open_handles_stage_one_service_attempt() {
        let temp = TempDir::new().expect("temporary root should exist");
        let first = MachinePublicationIntentStore::open(temp.path())
            .expect("first publication store should open");
        let second = MachinePublicationIntentStore::open(temp.path())
            .expect("second publication store should open");
        let barrier = Arc::new(Barrier::new(3));
        let tenant = tenant();
        let authority = authority();

        let workers = [first, second].map(|store| {
            let barrier = Arc::clone(&barrier);
            let tenant = tenant.clone();
            let authority = authority.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .stage_service_attempt(
                        &tenant,
                        "api",
                        &authority,
                        &[SandboxPortBinding::tcp("http", 18_080, 8_080)],
                    )
                    .expect("concurrent attempt should linearize")
            })
        });
        barrier.wait();
        let [first, second] = workers.map(|worker| worker.join().expect("worker should join"));

        assert_eq!(first.plan_id, second.plan_id);
        assert_eq!(first.sandbox_id, second.sandbox_id);
    }

    #[test]
    fn legacy_service_intent_cannot_represent_canonical_command_identity() {
        let temp = TempDir::new().expect("temporary root should exist");
        let store = MachinePublicationIntentStore::open(temp.path())
            .expect("legacy publication store should open");
        store
            .stage_service_attempt(
                &tenant(),
                "api",
                &authority(),
                &[SandboxPortBinding::tcp("http", 18_080, 8_080)],
            )
            .expect("legacy attempt should stage");

        let durable = fs::read_to_string(&store.state_path)
            .expect("legacy publication authority should be readable");
        for absent in [
            "command_id",
            "dispatch_epoch",
            "provider_target",
            "desired_digest",
            "source_digest",
            "network_plan_digest",
        ] {
            assert!(
                !durable.contains(absent),
                "legacy service intent unexpectedly represents canonical field {absent}"
            );
        }
    }

    #[test]
    fn unknown_store_entry_fails_closed() {
        let temp = TempDir::new().expect("temporary root should exist");
        let store = MachinePublicationIntentStore::open(temp.path())
            .expect("publication store should open");
        fs::write(store.root.join("unexpected.json"), b"{}").expect("unknown fixture should write");

        let error = store
            .stage_service_attempt(&tenant(), "api", &authority(), &[])
            .expect_err("unknown state must fail closed");

        assert!(error.to_string().contains("unknown entry"), "{error}");
    }

    fn tenant() -> TenantId {
        TenantId::new("tenant-machine-publication").expect("tenant fixture should validate")
    }

    fn authority() -> MachineForwarderAuthority {
        MachineForwarderAuthority::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key("machine-publication-test"),
                "machine-publication-provider",
            )
            .expect("provider fixture should validate"),
            NetworkResourceGeneration::new(7),
        )
    }
}
