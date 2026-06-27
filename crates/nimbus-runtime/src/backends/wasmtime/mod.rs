//! Wasmtime component backend scaffolding.
//!
//! W2 owns the engine configuration, `nimbus:host` WIT contract, and linker
//! mapping from typed component imports to the existing `HostBridge` request
//! envelope. Later phases attach this module to runtime backend dispatch,
//! bundle loading, fuel scheduling, and retained Store pooling.

pub(crate) mod host_linker;

pub(crate) fn component_linker_diagnostics() -> crate::Result<()> {
    host_linker::component_linker_diagnostics()
}
