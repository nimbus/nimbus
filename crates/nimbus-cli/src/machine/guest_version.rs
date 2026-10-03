//! Host gate on the guest nimbus release.
//!
//! The default machine-os stream ships one guest image per CLI release. A
//! guest from another release can pass the machine API protocol label check
//! and still lack routes that this CLI calls, so the host refuses that guest
//! before it sends workload operations. An image outside the release stream is
//! an explicit operator override: the host warns and continues.

use nimbus::Error;
use nimbus_machine::api::MachineApiHealthResponse;
use semver::Version;

use super::record::MachineImageSource;
use super::{
    DEFAULT_MACHINE_NAME, DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY, describe_machine_image_source,
    machine_image_reference_repository,
};
use crate::cli_ux;

/// Refuse a release-stream guest whose nimbus version differs from this CLI,
/// and warn about an override guest with the same mismatch.
pub(super) fn require_matching_guest_nimbus_version(
    image_source: &MachineImageSource,
    health: &MachineApiHealthResponse,
) -> Result<(), Error> {
    match check_guest_nimbus_version(
        image_source,
        health.nimbus_version.as_deref(),
        env!("CARGO_PKG_VERSION"),
    ) {
        GuestNimbusVersionCheck::Matches => Ok(()),
        GuestNimbusVersionCheck::OverrideMismatch(message) => {
            let _ = cli_ux::write_stderr_prefixed_line("warning:", &message);
            Ok(())
        }
        GuestNimbusVersionCheck::ReleaseMismatch(message) => {
            Err(Error::PreconditionFailed(message))
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GuestNimbusVersionCheck {
    Matches,
    OverrideMismatch(String),
    ReleaseMismatch(String),
}

fn check_guest_nimbus_version(
    image_source: &MachineImageSource,
    guest_version: Option<&str>,
    host_version: &str,
) -> GuestNimbusVersionCheck {
    if guest_version == Some(host_version) {
        return GuestNimbusVersionCheck::Matches;
    }
    let image = describe_machine_image_source(image_source);
    let Some(release_tag) = release_stream_tag(image_source) else {
        let guest = guest_version.unwrap_or("an unreported version");
        return GuestNimbusVersionCheck::OverrideMismatch(format!(
            "machine '{DEFAULT_MACHINE_NAME}' guest runs nimbus {guest}, not this CLI's nimbus {host_version}; continuing because image '{image}' is an explicit override outside the {DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY} release stream"
        ));
    };
    let guest = match guest_version {
        Some(version) => format!("runs nimbus {version}"),
        None => format!(
            "does not report its nimbus version, so it predates this CLI (image release {release_tag})"
        ),
    };
    GuestNimbusVersionCheck::ReleaseMismatch(format!(
        "machine '{DEFAULT_MACHINE_NAME}' guest {guest}, but this CLI is nimbus {host_version}; image '{image}' does not match this release. Run `nimbus machine os upgrade --restart` to boot machine-os v{host_version}"
    ))
}

/// The release tag of an image in the default machine-os stream, or `None`
/// for any other source. Only a plain `v<major>.<minor>.<patch>` tag counts:
/// a candidate tag such as `v0.1.49-f45`, a digest-only reference, another
/// repository, an HTTP URL, or a local disk is an explicit override.
fn release_stream_tag(image_source: &MachineImageSource) -> Option<&str> {
    let MachineImageSource::OciReference { reference } = image_source else {
        return None;
    };
    if machine_image_reference_repository(reference) != DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY {
        return None;
    }
    let stripped = reference.trim_start_matches("docker://");
    let without_digest = stripped.split('@').next().unwrap_or(stripped);
    let last_component = without_digest.rsplit('/').next()?;
    let (_, tag) = last_component.rsplit_once(':')?;
    let version = Version::parse(tag.strip_prefix('v')?).ok()?;
    (version.pre.is_empty() && version.build.is_empty()).then_some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oci(reference: &str) -> MachineImageSource {
        MachineImageSource::OciReference {
            reference: reference.to_owned(),
        }
    }

    #[test]
    fn release_stream_guest_with_matching_version_passes() {
        assert_eq!(
            check_guest_nimbus_version(
                &oci("docker://ghcr.io/nimbus/machine-os:v0.1.49"),
                Some("0.1.49"),
                "0.1.49",
            ),
            GuestNimbusVersionCheck::Matches
        );
    }

    #[test]
    fn release_stream_guest_with_older_version_is_refused_with_both_versions() {
        let check = check_guest_nimbus_version(
            &oci("docker://ghcr.io/nimbus/machine-os:v0.1.45"),
            Some("0.1.45"),
            "0.1.49",
        );
        let GuestNimbusVersionCheck::ReleaseMismatch(message) = check else {
            panic!("a 0.1.45 release guest must be refused by a 0.1.49 CLI, got {check:?}");
        };
        assert!(message.contains("guest runs nimbus 0.1.45"), "{message}");
        assert!(message.contains("this CLI is nimbus 0.1.49"), "{message}");
        assert!(
            message.contains("nimbus machine os upgrade --restart"),
            "{message}"
        );
    }

    #[test]
    fn unreported_guest_from_pinned_release_image_is_refused_with_both_versions() {
        // A v0.1.45 guest predates version reporting and was pinned by digest.
        let check = check_guest_nimbus_version(
            &oci(
                "docker://ghcr.io/nimbus/machine-os:v0.1.45@sha256:e313a09b481b86de8cfe99cefdc1e9b631d65e96b3971eb300660f8ae92e1e9b",
            ),
            None,
            "0.1.49",
        );
        let GuestNimbusVersionCheck::ReleaseMismatch(message) = check else {
            panic!("an unreported release guest must be refused, got {check:?}");
        };
        assert!(
            message.contains("does not report its nimbus version"),
            "{message}"
        );
        assert!(message.contains("image release v0.1.45"), "{message}");
        assert!(message.contains("this CLI is nimbus 0.1.49"), "{message}");
    }

    #[test]
    fn explicit_override_images_warn_instead_of_refusing() {
        for reference in [
            "docker://ghcr.io/nimbus/machine-os:v0.1.49-f45",
            "docker://ghcr.io/nimbus/machine-os@sha256:abc123",
            "docker://ghcr.io/nimbus/machine-os:latest",
            "docker://quay.io/podman/machine-os:5.6",
            "docker://ghcr.io/example/machine-os:v0.1.45",
        ] {
            let check = check_guest_nimbus_version(&oci(reference), Some("0.1.45"), "0.1.49");
            let GuestNimbusVersionCheck::OverrideMismatch(message) = check else {
                panic!("{reference} must warn as an explicit override, got {check:?}");
            };
            assert!(message.contains("guest runs nimbus 0.1.45"), "{message}");
            assert!(message.contains("nimbus 0.1.49"), "{message}");
            assert!(message.contains(reference), "{message}");
        }
    }

    #[test]
    fn non_oci_override_with_unreported_version_warns() {
        let check = check_guest_nimbus_version(
            &MachineImageSource::LocalDisk {
                path: "/tmp/nimbus-machine.raw".into(),
            },
            None,
            "0.1.49",
        );
        let GuestNimbusVersionCheck::OverrideMismatch(message) = check else {
            panic!("a local disk override must warn, got {check:?}");
        };
        assert!(message.contains("an unreported version"), "{message}");
    }
}
