//! Launch-spec resolution and sandbox identity helpers.

use ulid::Ulid;

use crate::backends::conmon::spec_resolve::{resolve_process_spec, resolve_root_spec, slugify};
use crate::backends::oci::buildah::OciImageLaunchDefaults;
use crate::error::Result;
use crate::instance::SandboxId;
use crate::spec::{SandboxSpec, resolve_process_without_image_defaults};

use super::manifest::{ContainerImageMetadata, ContainerResolvedLaunchSpec};

pub(super) fn next_sandbox_id(name: &str) -> SandboxId {
    SandboxId::new(format!(
        "{}-{}",
        slugify(name),
        Ulid::new().to_string().to_ascii_lowercase()
    ))
}

pub(super) fn hostname_for(spec: &SandboxSpec) -> String {
    let slug = slugify(spec.display_name());
    if slug.is_empty() {
        "nimbus-container".to_owned()
    } else {
        slug
    }
}

pub(super) fn resolve_start_spec(
    spec: &SandboxSpec,
    launch_defaults: Option<&OciImageLaunchDefaults>,
) -> Result<ContainerResolvedLaunchSpec> {
    let Some(launch_defaults) = launch_defaults else {
        let mut resolved_spec = spec.clone();
        resolved_spec.process = resolve_process_without_image_defaults(&spec.process)?;
        let process_user = resolved_spec.process.user.clone();
        return Ok(ContainerResolvedLaunchSpec {
            spec: resolved_spec,
            image_metadata: ContainerImageMetadata {
                user: process_user,
                ..ContainerImageMetadata::default()
            },
        });
    };

    let mut resolved_spec = spec.clone();
    resolved_spec.root = resolve_root_spec(&spec.root, &launch_defaults.rootfs);
    resolved_spec.process = resolve_process_spec(&spec.process, &launch_defaults.process);

    Ok(ContainerResolvedLaunchSpec {
        spec: resolved_spec,
        image_metadata: ContainerImageMetadata {
            user: launch_defaults.user.clone(),
            stop_signal: launch_defaults.stop_signal.clone(),
            healthcheck: launch_defaults.healthcheck.clone(),
            labels: launch_defaults.labels.clone(),
            exposed_ports: launch_defaults.exposed_ports.clone(),
        },
    })
}
