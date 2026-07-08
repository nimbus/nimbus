//! Machine provider selection and per-provider capability contracts.

use nimbus_core::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineProvider {
    Krunkit,
    Vfkit,
    Wsl2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineImageFormat {
    Raw,
    Tar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineBootstrapMode {
    Ignition,
    BootcMachineConfig,
    ShellScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineProviderCapabilities {
    pub uses_provider_networking: bool,
    pub requires_exclusive_active: bool,
    pub image_format: MachineImageFormat,
    pub bootstrap_mode: MachineBootstrapMode,
    pub oci_artifact_disk_type: &'static str,
}

const KRUNKIT_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: false,
    requires_exclusive_active: true,
    image_format: MachineImageFormat::Raw,
    bootstrap_mode: MachineBootstrapMode::Ignition,
    oci_artifact_disk_type: "applehv",
};

// vfkit is the second macOS VMM. Like krunkit it boots the Nimbus-managed
// `applehv` disk over EFI, bootstraps via an Ignition vsock, and relies on an
// external gvproxy userspace network stack — so its capabilities mirror
// krunkit's. The two differ only in the VMM binary and the on-VMM net-device
// syntax, both of which are owned by the per-provider `MachineVmmBackend`.
const VFKIT_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: false,
    requires_exclusive_active: true,
    image_format: MachineImageFormat::Raw,
    bootstrap_mode: MachineBootstrapMode::Ignition,
    oci_artifact_disk_type: "applehv",
};

const WSL2_PROVIDER_CAPABILITIES: MachineProviderCapabilities = MachineProviderCapabilities {
    uses_provider_networking: true,
    requires_exclusive_active: false,
    image_format: MachineImageFormat::Tar,
    bootstrap_mode: MachineBootstrapMode::ShellScript,
    oci_artifact_disk_type: "wsl",
};

impl MachineProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Krunkit => "krunkit",
            Self::Vfkit => "vfkit",
            Self::Wsl2 => "wsl2",
        }
    }

    /// Parse a provider selection token (config field or `NIMBUS_MACHINE_PROVIDER`
    /// value). Matching is case-insensitive and ignores surrounding whitespace.
    /// Returns `None` for unknown tokens so callers can surface a clear error.
    pub fn from_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "krunkit" => Some(Self::Krunkit),
            "vfkit" => Some(Self::Vfkit),
            "wsl2" => Some(Self::Wsl2),
            _ => None,
        }
    }

    /// Whether this provider runs the Nimbus-managed macOS `applehv` guest that
    /// needs host↔guest binary sync and a forwarded machine API over SSH. Both
    /// macOS microVM backends (krunkit and vfkit) qualify; WSL2 owns its own
    /// guest plumbing.
    pub fn uses_managed_applehv_guest(self) -> bool {
        matches!(self, Self::Krunkit | Self::Vfkit)
    }

    pub fn capabilities(self) -> MachineProviderCapabilities {
        match self {
            Self::Krunkit => KRUNKIT_PROVIDER_CAPABILITIES,
            Self::Vfkit => VFKIT_PROVIDER_CAPABILITIES,
            Self::Wsl2 => WSL2_PROVIDER_CAPABILITIES,
        }
    }

    pub fn uses_provider_networking(self) -> bool {
        self.capabilities().uses_provider_networking
    }

    pub fn requires_exclusive_active(self) -> bool {
        self.capabilities().requires_exclusive_active
    }

    pub fn image_format(self) -> MachineImageFormat {
        self.capabilities().image_format
    }

    pub fn bootstrap_mode(self) -> MachineBootstrapMode {
        self.capabilities().bootstrap_mode
    }

    pub fn oci_artifact_disk_type(self) -> &'static str {
        self.capabilities().oci_artifact_disk_type
    }

    /// The canonical "this provider has no backend on this host yet" error.
    ///
    /// Both the start path (`vmm_backend`) and the stop path
    /// (`stop_provider_machine`) reject not-yet-implemented providers, and both
    /// must reject with the *same* message so selection stays a deliberate,
    /// fail-closed opt-in rather than a silent no-op. Owning the text here keeps
    /// the two gates from drifting apart. The provider name is upper-cased so the
    /// message reads as a proper noun (e.g. `WSL2`).
    pub fn unavailable_error(self) -> Error {
        Error::InvalidInput(format!(
            "the {} machine provider is not available on this host yet",
            self.as_str().to_ascii_uppercase()
        ))
    }
}
