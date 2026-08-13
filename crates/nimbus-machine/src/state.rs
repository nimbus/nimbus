//! Machine runtime state: lifecycle, manager phase, and resolved runtime facts.

use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

use nimbus_network::{
    ListenerId, NetworkLeaseEpoch, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceGeneration, PortLeaseId,
};
use serde::{Deserialize, Serialize};

// The machine state schema (`status.json`) is at its first version. The
// `krunkit` -> `vmm` runtime-helper rename -- one provider-neutral VMM slot per
// machine, with the matching `*-krunkit.{pid,sock}` -> `*-vmm.*` runtime-file
// scheme -- was made directly as a pre-launch breaking change, not a migration.
// A `status.json` written before the rename simply lacks the now-required
// `runtime.helper_binaries.vmm` field; the loader rebuilds that unparseable
// record into a clean Stopped/Stale state (see
// `files::load_machine_state_if_exists`) rather than stranding the machine, so
// the rename needs no version bump. State is rebuildable runtime data, not
// durable user data. The version gate (probe + newer/older rebuild arms) stays
// so the first post-launch schema change can bump to 2 and route pre-existing
// files through the rebuild arm with an explicit "schema evolved" reason;
// pre-launch there is no shipped older version to account for, so the schema
// starts at 1.
pub const CURRENT_MACHINE_STATE_VERSION: u32 = 1;
pub const CURRENT_MACHINE_BOOT_AUTHORITY_VERSION: u32 = 1;
const MACHINE_SSH_PROVIDER_KEY: &str = "nimbus-cli.machine-gvproxy-ssh";
const MACHINE_SSH_RESOURCE_GENERATION: NetworkResourceGeneration =
    NetworkResourceGeneration::new(1);
const MACHINE_SSH_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);

/// Pure stable identity and fence for one managed machine's SSH listener.
///
/// The CLI remains the gvproxy effect owner. This value only prevents the
/// request builder and observed system projection from duplicating identity,
/// generation, epoch, or provider-registration constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineSshPortLeaseIdentity {
    listener_id: ListenerId,
    port_lease_id: PortLeaseId,
    generation: NetworkResourceGeneration,
    lease_epoch: NetworkLeaseEpoch,
    provider_id: NetworkProviderId,
}

impl MachineSshPortLeaseIdentity {
    pub fn for_listener(listener_id: &ListenerId) -> Self {
        Self {
            listener_id: listener_id.clone(),
            port_lease_id: PortLeaseId::for_listener(listener_id),
            generation: MACHINE_SSH_RESOURCE_GENERATION,
            lease_epoch: MACHINE_SSH_LEASE_EPOCH,
            provider_id: NetworkProviderId::for_registration_key(MACHINE_SSH_PROVIDER_KEY),
        }
    }

    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    pub fn port_lease_id(&self) -> &PortLeaseId {
        &self.port_lease_id
    }

    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    pub const fn lease_epoch(&self) -> NetworkLeaseEpoch {
        self.lease_epoch
    }

    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineStateRecord {
    pub version: u32,
    pub lifecycle: MachineLifecycle,
    pub manager: MachineManagerState,
    pub runtime: Option<MachineRuntimeState>,
    pub last_error: Option<String>,
}

impl MachineStateRecord {
    pub fn initialized() -> Self {
        Self {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Unconfigured,
            runtime: None,
            last_error: None,
        }
    }

    pub fn rebuilt(reason: impl Into<String>) -> Self {
        Self {
            version: CURRENT_MACHINE_STATE_VERSION,
            lifecycle: MachineLifecycle::Stopped,
            manager: MachineManagerState::Stale,
            runtime: None,
            last_error: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineLifecycle {
    Uninitialized,
    Stopped,
    Starting,
    Running,
    Failed,
}

impl MachineLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineManagerState {
    Unconfigured,
    HelpersResolved,
    Launching,
    Ready,
    Failed,
    Stale,
}

impl MachineManagerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured",
            Self::HelpersResolved => "helpers-resolved",
            Self::Launching => "launching",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stale => "stale",
        }
    }
}

/// Exact parent-issued gvproxy incarnation authenticated by Machine API
/// mutations and echoed by their typed evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineForwarderAuthority {
    provider_instance: NetworkProviderHandle,
    generation: NetworkResourceGeneration,
}

impl MachineForwarderAuthority {
    pub fn new(
        provider_instance: NetworkProviderHandle,
        generation: NetworkResourceGeneration,
    ) -> Self {
        Self {
            provider_instance,
            generation,
        }
    }

    /// Parent-issued provider identity. Guest boot or node facts never mint it.
    pub fn provider_instance(&self) -> &NetworkProviderHandle {
        &self.provider_instance
    }

    /// Monotonic generation of the launched machine provider incarnation.
    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }

    /// Authenticate exact provider and generation equality without revealing
    /// opaque provider material in the error.
    pub fn authenticate(&self, presented: &Self) -> Result<(), MachineForwarderAuthorityMismatch> {
        if self.provider_instance != presented.provider_instance {
            return Err(MachineForwarderAuthorityMismatch::ProviderInstance);
        }
        if self.generation != presented.generation {
            return Err(MachineForwarderAuthorityMismatch::Generation {
                expected: self.generation,
                presented: presented.generation,
            });
        }
        Ok(())
    }
}

/// Stable, redaction-safe reason a Machine API mutation did not authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineForwarderAuthorityMismatch {
    ProviderInstance,
    Generation {
        expected: NetworkResourceGeneration,
        presented: NetworkResourceGeneration,
    },
}

impl Display for MachineForwarderAuthorityMismatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderInstance => {
                formatter.write_str("machine forwarder provider instance does not match")
            }
            Self::Generation {
                expected,
                presented,
            } => write!(
                formatter,
                "machine forwarder generation does not match: expected {}, presented {}",
                expected.as_u64(),
                presented.as_u64()
            ),
        }
    }
}

impl std::error::Error for MachineForwarderAuthorityMismatch {}

/// Strict parent-issued authority evidence installed in each guest before its
/// machine API may claim network state or perform effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineBootAuthorityEvidence {
    version: u32,
    machine_id: String,
    forwarder_authority: MachineForwarderAuthority,
}

impl MachineBootAuthorityEvidence {
    pub fn new(
        machine_id: impl Into<String>,
        forwarder_authority: MachineForwarderAuthority,
    ) -> Result<Self, MachineBootAuthorityEvidenceError> {
        let evidence = Self {
            version: CURRENT_MACHINE_BOOT_AUTHORITY_VERSION,
            machine_id: machine_id.into(),
            forwarder_authority,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), MachineBootAuthorityEvidenceError> {
        if self.version != CURRENT_MACHINE_BOOT_AUTHORITY_VERSION {
            return Err(MachineBootAuthorityEvidenceError::UnsupportedVersion {
                attempted: self.version,
                supported: CURRENT_MACHINE_BOOT_AUTHORITY_VERSION,
            });
        }
        if self.machine_id.trim().is_empty() {
            return Err(MachineBootAuthorityEvidenceError::EmptyMachineId);
        }
        Ok(())
    }

    pub const fn version(&self) -> u32 {
        self.version
    }

    pub fn machine_id(&self) -> &str {
        &self.machine_id
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineBootAuthorityEvidenceError {
    UnsupportedVersion { attempted: u32, supported: u32 },
    EmptyMachineId,
}

impl Display for MachineBootAuthorityEvidenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion {
                attempted,
                supported,
            } => write!(
                formatter,
                "machine boot authority uses unsupported version {attempted}; expected {supported}"
            ),
            Self::EmptyMachineId => {
                formatter.write_str("machine boot authority machine identity cannot be empty")
            }
        }
    }
}

impl std::error::Error for MachineBootAuthorityEvidenceError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRuntimeState {
    pub helper_binaries: MachineHelperBinaryPaths,
    pub image_path: PathBuf,
    pub efi_variable_store_path: PathBuf,
    #[serde(default)]
    pub machine_image_source: String,
    /// Address-independent identity of the host SSH listener lease.
    pub ssh_listener_id: ListenerId,
    /// Exact parent-issued gvproxy incarnation used by Machine API mutations.
    pub forwarder_authority: MachineForwarderAuthority,
    pub ssh_port: u16,
    pub rest_uri: String,
    pub ready_vsock_port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineHelperBinaryPaths {
    /// The resolved VMM binary for the machine's provider (krunkit or vfkit).
    /// One VMM runs per machine, so this is a single provider-neutral slot.
    pub vmm: PathBuf,
    pub gvproxy: PathBuf,
}

#[cfg(test)]
mod tests {
    use nimbus_network::{NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration};

    use super::{
        CURRENT_MACHINE_BOOT_AUTHORITY_VERSION, MachineBootAuthorityEvidence,
        MachineBootAuthorityEvidenceError, MachineForwarderAuthority,
    };

    fn provider_instance(registration: &str, value: &str) -> NetworkProviderHandle {
        NetworkProviderHandle::new(NetworkProviderId::for_registration_key(registration), value)
            .expect("provider fixture should validate")
    }

    #[test]
    fn forwarder_authority_authenticates_only_the_exact_incarnation() {
        let active = MachineForwarderAuthority::new(
            provider_instance("machine-gvproxy", "machine-config-01"),
            NetworkResourceGeneration::new(7),
        );
        let exact = active.clone();
        let stale = MachineForwarderAuthority::new(
            provider_instance("machine-gvproxy", "machine-config-01"),
            NetworkResourceGeneration::new(6),
        );
        let crossed = MachineForwarderAuthority::new(
            provider_instance("machine-gvproxy", "machine-config-02"),
            NetworkResourceGeneration::new(7),
        );

        assert_eq!(
            active.provider_instance(),
            &provider_instance("machine-gvproxy", "machine-config-01")
        );
        assert_eq!(active.generation(), NetworkResourceGeneration::new(7));
        assert_eq!(active.authenticate(&exact), Ok(()));
        assert!(
            active.authenticate(&stale).is_err(),
            "a stale generation must not authenticate"
        );
        assert!(
            active.authenticate(&crossed).is_err(),
            "a different provider instance must not authenticate"
        );
    }

    #[test]
    fn forwarder_authority_wire_is_strict_and_round_trips() {
        let authority = MachineForwarderAuthority::new(
            provider_instance("machine-gvproxy", "machine-config-01"),
            NetworkResourceGeneration::new(9),
        );
        let value = serde_json::to_value(&authority).expect("authority should serialize");

        assert_eq!(
            serde_json::from_value::<MachineForwarderAuthority>(value.clone())
                .expect("authority should deserialize"),
            authority
        );

        let mut unknown = value.clone();
        unknown
            .as_object_mut()
            .expect("authority wire should be an object")
            .insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(
            serde_json::from_value::<MachineForwarderAuthority>(unknown).is_err(),
            "unknown forwarder-authority fields must fail closed"
        );

        let mut missing = value;
        missing
            .as_object_mut()
            .expect("authority wire should be an object")
            .remove("generation");
        assert!(
            serde_json::from_value::<MachineForwarderAuthority>(missing).is_err(),
            "missing forwarder-authority fields must fail closed"
        );
    }

    #[test]
    fn machine_boot_authority_wire_is_strict_and_validated() {
        let authority = MachineForwarderAuthority::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key("machine-gvproxy"),
                "opaque-machine",
            )
            .expect("provider handle should validate"),
            NetworkResourceGeneration::new(9),
        );
        let evidence = MachineBootAuthorityEvidence::new("default", authority.clone())
            .expect("boot authority should validate");
        let mut wire = serde_json::to_value(&evidence).expect("boot authority should serialize");
        assert_eq!(evidence.version(), CURRENT_MACHINE_BOOT_AUTHORITY_VERSION);
        assert_eq!(evidence.machine_id(), "default");
        assert_eq!(evidence.forwarder_authority(), &authority);
        assert_eq!(
            serde_json::from_value::<MachineBootAuthorityEvidence>(wire.clone())
                .expect("strict wire should round-trip"),
            evidence
        );

        wire.as_object_mut()
            .expect("boot authority wire should be an object")
            .insert("guest_boot_id".to_owned(), serde_json::json!("forbidden"));
        assert!(
            serde_json::from_value::<MachineBootAuthorityEvidence>(wire).is_err(),
            "guest boot facts must not enter parent-issued authority evidence"
        );

        let empty = MachineBootAuthorityEvidence::new("", authority)
            .expect_err("empty machine identity must fail");
        assert_eq!(empty, MachineBootAuthorityEvidenceError::EmptyMachineId);
    }
}
