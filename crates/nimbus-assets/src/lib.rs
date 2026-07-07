//! Embedded production asset catalog for Nimbus distribution, UI, and templates.
//!
//! This crate owns byte/text catalog access only. Behavior such as routing,
//! auth policy, package provisioning, prompts, and filesystem writes stays in
//! the consuming crates.

#![forbid(unsafe_code)]

#[cfg(feature = "js-packages")]
pub mod js_packages;

#[cfg(feature = "templates")]
pub mod templates;

#[cfg(feature = "ui")]
pub mod ui;
