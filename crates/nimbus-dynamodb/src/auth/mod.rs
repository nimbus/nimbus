//! Authentication surface for the DynamoDB adapter.
//!
//! Hosts the vendored AWS SigV4 verification module (`sigv4`). Access-key →
//! tenant resolution and the strict/lookup `auth_mode` toggle land in
//! D0.5 / D0.8 / D7.

// `sigv4` is vendored from ExtendDB (see the per-file SPDX headers + NOTICE).
// It is held to upstream's lint baseline: Nimbus's stricter workspace clippy
// config is relaxed here rather than restructuring the verbatim copy.
#[allow(clippy::collapsible_if)]
pub mod sigv4;
