//! Transport-free compute surface extracted from `nimbus-server` (CP1):
//! runtime bundle execution, artifact/provenance admission, machine
//! lifecycle, and service manager wiring. This crate carries no HTTP/
//! WebSocket transport framework on its own surface — the server crate
//! mounts it and re-exports the pieces its adapters still need.

pub mod artifact_verifier_effects;
pub mod execution;
pub mod machine_lifecycle;
pub mod service_manager;
