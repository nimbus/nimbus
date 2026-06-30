//! Shared OCI mount-namespace hardening for every sandbox backend.
//!
//! The container backend and the krun microVM backend both render an OCI
//! runtime `config.json`. Two construction-time hardening sets are identical
//! across them and security-critical, so they live here once instead of being
//! forked per backend (the same "shared seam in `oci`" pattern as
//! [`super::egress`]):
//!
//! * `maskedPaths` — the OCI default-spec set of sensitive `/proc` and `/sys`
//!   entries that must read as empty/inaccessible inside the workload mount
//!   namespace (`/proc/kcore`, `/proc/keys`, `/proc/timer_list`,
//!   `/sys/firmware`, ...). Masking closes host-kernel info-leak channels and
//!   the `/proc/sysrq-trigger`-style abuse surface.
//! * `readonlyPaths` — the OCI default-spec set of `/proc` control surfaces
//!   that must be read-only (`/proc/sys`, `/proc/sysrq-trigger`, `/proc/bus`,
//!   `/proc/fs`, `/proc/irq`).
//!
//! These are the runc/containerd default-spec sets. Keeping them in one place
//! means a backend can never silently drift to a weaker mount-namespace
//! posture, and a single always-on test pins the contents.

use serde_json::{Value, json};

/// OCI default-spec `linux.maskedPaths`: sensitive host-kernel surfaces that
/// must read as empty/inaccessible inside the workload mount namespace.
pub(crate) const DEFAULT_MASKED_PATHS: &[&str] = &[
    "/proc/acpi",
    "/proc/asound",
    "/proc/kcore",
    "/proc/keys",
    "/proc/latency_stats",
    "/proc/timer_list",
    "/proc/timer_stats",
    "/proc/sched_debug",
    "/proc/scsi",
    "/sys/firmware",
    "/sys/devices/virtual/powercap",
];

/// OCI default-spec `linux.readonlyPaths`: `/proc` control surfaces that must
/// be read-only inside the workload mount namespace.
pub(crate) const DEFAULT_READONLY_PATHS: &[&str] = &[
    "/proc/bus",
    "/proc/fs",
    "/proc/irq",
    "/proc/sys",
    "/proc/sysrq-trigger",
];

/// `linux.maskedPaths` value for an OCI bundle.
pub(crate) fn masked_paths_json() -> Value {
    json!(DEFAULT_MASKED_PATHS)
}

/// `linux.readonlyPaths` value for an OCI bundle.
pub(crate) fn readonly_paths_json() -> Value {
    json!(DEFAULT_READONLY_PATHS)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MASKED_PATHS, DEFAULT_READONLY_PATHS, masked_paths_json, readonly_paths_json,
    };

    #[test]
    fn masked_paths_cover_the_sensitive_oci_default_set() {
        for required in [
            "/proc/kcore",
            "/proc/keys",
            "/proc/timer_list",
            "/proc/latency_stats",
            "/proc/sched_debug",
            "/sys/firmware",
        ] {
            assert!(
                DEFAULT_MASKED_PATHS.contains(&required),
                "masked paths must include the sensitive host-kernel surface {required}"
            );
        }
    }

    #[test]
    fn readonly_paths_cover_the_proc_control_surfaces() {
        for required in [
            "/proc/sys",
            "/proc/sysrq-trigger",
            "/proc/bus",
            "/proc/fs",
            "/proc/irq",
        ] {
            assert!(
                DEFAULT_READONLY_PATHS.contains(&required),
                "read-only paths must include the /proc control surface {required}"
            );
        }
    }

    #[test]
    fn masked_and_readonly_sets_are_disjoint_and_deduplicated() {
        for (label, set) in [
            ("masked", DEFAULT_MASKED_PATHS),
            ("read-only", DEFAULT_READONLY_PATHS),
        ] {
            let mut seen = std::collections::BTreeSet::new();
            for path in set {
                assert!(
                    seen.insert(*path),
                    "{label} path {path} is duplicated in the hardening set"
                );
            }
        }
        assert!(
            DEFAULT_MASKED_PATHS
                .iter()
                .all(|masked| !DEFAULT_READONLY_PATHS.contains(masked)),
            "a path must not be both masked and merely read-only"
        );
    }

    #[test]
    fn json_helpers_render_string_arrays() {
        for value in [masked_paths_json(), readonly_paths_json()] {
            let entries = value
                .as_array()
                .expect("hardening helper must render a JSON array");
            assert!(!entries.is_empty(), "hardening set must not be empty");
            assert!(
                entries.iter().all(|entry| entry.is_string()),
                "every hardening path must render as a JSON string: {value}"
            );
        }
    }
}
