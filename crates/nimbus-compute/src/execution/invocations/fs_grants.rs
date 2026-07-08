//! Grant resolution for isolate filesystem construction.
//!
//! `nimbus-fs` owns capability *mechanics* (`FsCaps`, `FsMountCaps`, mount
//! gating); this module owns the *policy* choice of which grants apply to an
//! invocation and asks `nimbus-fs` to build the resulting `FileSystemRc`. It
//! must never implement rights logic itself — only compose `FsCaps` builders
//! and hand them to `nimbus_fs::file_system_for_grants` /
//! `nimbus_fs::deny_file_system`.

use std::io;

use nimbus_fs::{FileSystemRc, FsCaps, FsMountCaps};

/// The explicit launch policy: full read-write access rooted at `/`.
///
/// This is the same authority the old ungated default constructor granted
/// implicitly; expressing it as an explicit grant makes tightening it a
/// configuration change instead of a code change.
pub(crate) fn launch_default_grants() -> FsCaps {
    FsCaps::new().grant("/", FsMountCaps::read_write())
}

/// The single choke point for resolving which `FsCaps` grant set applies to
/// an invocation.
///
/// Today every invocation resolves the launch-default grant. Per-tenant or
/// per-profile grant sourcing (the profile-aware isolate runtime lane) plugs
/// in here without touching `runtime_for_host` or `nimbus-fs`.
pub(crate) fn resolve_fs_grants() -> Option<FsCaps> {
    Some(launch_default_grants())
}

/// Resolve `grants` into a `FileSystemRc`, fail-closed.
///
/// `None` (no grant resolved) always yields the deny filesystem; any error
/// building the gated filesystem propagates rather than falling back to an
/// ungated view.
pub(crate) fn resolved_file_system(grants: Option<FsCaps>) -> io::Result<FileSystemRc> {
    match grants {
        None => Ok(nimbus_fs::deny_file_system()),
        Some(grants) => nimbus_fs::file_system_for_grants(&grants),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_default_grants_root_read_write() {
        let grants = launch_default_grants();
        let root = grants
            .grant_for_path(std::path::Path::new("/"))
            .expect("launch default must grant the root prefix");
        assert_eq!(root, &FsMountCaps::read_write());
    }

    #[test]
    fn resolve_fs_grants_resolves_the_launch_default() {
        let resolved = resolve_fs_grants().expect("launch default must resolve a grant");
        let launch_default = launch_default_grants();
        assert_eq!(
            resolved.grant_for_path(std::path::Path::new("/anything")),
            launch_default.grant_for_path(std::path::Path::new("/anything")),
            "resolve_fs_grants must resolve to the launch-default grant"
        );
    }

    #[test]
    fn resolved_file_system_none_is_fail_closed_construction() {
        // The behavior of the resulting filesystem (every op denied) is
        // proven in nimbus-fs (`ungranted_substrate_gets_deny_filesystem`);
        // here we only own the policy choice that `None` routes to it.
        resolved_file_system(None).expect("deny filesystem construction must not fail");
    }

    #[test]
    fn resolved_file_system_some_constructs_from_grants() {
        let grants = FsCaps::new().grant("/", FsMountCaps::read_write());
        resolved_file_system(Some(grants)).expect("granted filesystem must construct");
    }
}
