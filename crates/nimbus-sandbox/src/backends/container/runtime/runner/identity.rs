//! Immutable prepared-runner identity and authority fingerprints.
//!
//! This child owns only canonical identity projection and hashing. It performs
//! no I/O, holds no lifecycle lock, and interprets no provider observation.

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{Result, SandboxError};

use super::super::config::ContainerStartMode;
use super::super::manifest::{
    ContainerLifecycleCoordinator, ContainerRunnerExecutionConfig, ContainerSandboxManifest,
};

pub(super) fn prepared_manifest_sha256(manifest: &ContainerSandboxManifest) -> Result<String> {
    let mut prepared = manifest.clone();
    prepared.start_mode = ContainerStartMode::PlanOnly;
    prepared.runner_handoff_id = None;
    manifest_sha256(&prepared, "prepared container runner")
}

pub(super) fn result_manifest_sha256(manifest: &ContainerSandboxManifest) -> Result<String> {
    manifest_sha256(manifest, "container runner effect result")
}

#[derive(Serialize)]
struct RunnerExecutionSpecIdentity<'a> {
    tenant_id: &'a nimbus_core::TenantId,
    owner: &'a crate::spec::SandboxOwnerSpec,
    backend: &'a crate::backend::SandboxBackendKind,
    root: &'a crate::spec::SandboxRootSpec,
    process: &'a crate::spec::SandboxProcessSpec,
    resources: &'a crate::spec::SandboxResourceLimits,
    lifecycle: &'a crate::spec::SandboxLifecycleSpec,
    port_bindings: &'a [crate::spec::SandboxPortBinding],
    mounts: &'a [crate::spec::SandboxMountSpec],
}

/// Allowlisted immutable authority for one prepared runner execution.
///
/// Mutable desired state (`spec.egress`), observed endpoint/status projections,
/// cleanup progress, and restart bookkeeping are intentionally absent.
#[derive(Serialize)]
struct RunnerExecutionIdentity<'a> {
    version: u32,
    handle_tenant_id: &'a nimbus_core::TenantId,
    sandbox_id: &'a crate::instance::SandboxId,
    sandbox_name: &'a str,
    sandbox_backend: &'a crate::backend::SandboxBackendKind,
    spec: RunnerExecutionSpecIdentity<'a>,
    image_metadata: &'a super::super::manifest::ContainerImageMetadata,
    bundle_layout: &'a crate::backends::container::bundle::ContainerBundleLayout,
    conmon_layout: &'a crate::backends::oci::conmon::OciConmonLayout,
    network_layout: &'a crate::backends::oci::network::OciNetworkLayout,
    network_config: Option<&'a crate::backends::oci::network::OciNetworkConfig>,
    requested_port_bindings: &'a [crate::spec::SandboxPortBinding],
    port_leases: &'a [nimbus_network::PortLeaseRequest],
    egress_proxy: Option<&'a crate::backends::oci::egress::EgressProxyAssignment>,
    conmon_launch: &'a crate::backends::oci::conmon::OciConmonLaunchPlan,
    runner_config: &'a ContainerRunnerExecutionConfig,
    lifecycle_coordinator: ContainerLifecycleCoordinator,
}

#[derive(Serialize)]
struct RunnerPreEffectAuthority<'a> {
    version: u32,
    execution: RunnerExecutionIdentity<'a>,
    desired_egress: &'a nimbus_egress::EgressPolicy,
}

/// Hash the immutable execution authority that survives normal lifecycle
/// progress after the effect fence. The full prepared-manifest hash still
/// authenticates acquisition and cancellation.
pub(super) fn execution_identity_sha256(manifest: &ContainerSandboxManifest) -> Result<String> {
    serialized_sha256(
        &runner_execution_identity(manifest),
        "container runner execution identity",
    )
}

/// Hash authority that must remain immutable until the first provider effect.
///
/// Terminal no-effect cleanup legitimately changes status, artifact, and
/// reservation fields, so its validator cannot compare the full prepared
/// manifest. Desired egress is still immutable at this phase and is included
/// here even though ordinary post-publication reloads intentionally exclude it
/// from [`execution_identity_sha256`].
pub(super) fn pre_effect_authority_sha256(manifest: &ContainerSandboxManifest) -> Result<String> {
    serialized_sha256(
        &RunnerPreEffectAuthority {
            version: 1,
            execution: runner_execution_identity(manifest),
            desired_egress: &manifest.spec.egress,
        },
        "container runner pre-effect authority",
    )
}

fn runner_execution_identity(manifest: &ContainerSandboxManifest) -> RunnerExecutionIdentity<'_> {
    RunnerExecutionIdentity {
        version: 1,
        handle_tenant_id: &manifest.handle.tenant_id,
        sandbox_id: &manifest.handle.id,
        sandbox_name: &manifest.handle.name,
        sandbox_backend: &manifest.handle.backend,
        spec: RunnerExecutionSpecIdentity {
            tenant_id: &manifest.spec.tenant_id,
            owner: &manifest.spec.owner,
            backend: &manifest.spec.backend,
            root: &manifest.spec.root,
            process: &manifest.spec.process,
            resources: &manifest.spec.resources,
            lifecycle: &manifest.spec.lifecycle,
            port_bindings: &manifest.spec.port_bindings,
            mounts: &manifest.spec.mounts,
        },
        image_metadata: &manifest.image_metadata,
        bundle_layout: &manifest.bundle_layout,
        conmon_layout: &manifest.conmon_layout,
        network_layout: &manifest.network_layout,
        network_config: manifest.network_config.as_ref(),
        requested_port_bindings: &manifest.requested_port_bindings,
        port_leases: &manifest.port_leases,
        egress_proxy: manifest.egress_proxy.as_ref(),
        conmon_launch: &manifest.conmon_launch,
        runner_config: &manifest.runner_config,
        lifecycle_coordinator: manifest.lifecycle_coordinator,
    }
}

fn manifest_sha256(manifest: &ContainerSandboxManifest, subject: &str) -> Result<String> {
    serialized_sha256(manifest, subject)
}

pub(super) fn serialized_sha256(value: &impl Serialize, subject: &str) -> Result<String> {
    let rendered = serde_json::to_vec(value).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to serialize {subject} fingerprint: {error}"),
    })?;
    let digest = Sha256::digest(rendered);
    let mut encoded = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    Ok(encoded)
}
