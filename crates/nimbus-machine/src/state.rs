//! Machine runtime state: lifecycle, manager phase, and resolved runtime facts.

use std::path::PathBuf;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRuntimeState {
    pub helper_binaries: MachineHelperBinaryPaths,
    pub image_path: PathBuf,
    pub efi_variable_store_path: PathBuf,
    #[serde(default)]
    pub machine_image_source: String,
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
