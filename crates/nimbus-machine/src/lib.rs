//! Shared machine record and provider contracts.
//!
//! This crate owns the render-independent machine model used by the CLI today
//! and by the server control plane as machine lifecycle endpoints move out of
//! `nimbus-bin`.

pub mod api;

mod image_source;
mod paths;
mod provider;
mod roots;
mod state;

pub use image_source::{
    CURRENT_MACHINE_CONFIG_VERSION, MachineConfigRecord, MachineGuestConfig,
    MachineGuestProvisioning, MachineImageSource, MachineResources, MachineVolume,
};
pub use paths::{
    DEFAULT_MACHINE_RUNTIME_ROOT, MACHINE_RUNTIME_ROOT_ENV, MachinePaths, resolve_runtime_root,
};
pub use provider::{
    MachineBootstrapMode, MachineImageFormat, MachineProvider, MachineProviderCapabilities,
};
pub use roots::{DEFAULT_NETWORK_STATE_ROOT, MachineRootLayout, NETWORK_STATE_ROOT_ENV};
pub use state::{
    CURRENT_MACHINE_STATE_VERSION, MachineHelperBinaryPaths, MachineLifecycle, MachineManagerState,
    MachineRuntimeState, MachineStateRecord,
};
