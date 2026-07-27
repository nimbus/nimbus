use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nimbus_core::TenantId;

use crate::instance::SandboxId;

const TENANTS_DIR: &str = "tenants";
const SANDBOXES_DIR: &str = "sandboxes";
const BUNDLE_DIR: &str = "bundle";
const ROOTFS_DIR: &str = "rootfs";
const STATE_DIR: &str = "state";
const VOLUMES_DIR: &str = "volumes";
const CONTAINERS_DIR: &str = "containers";
const MANIFEST_FILE: &str = "manifest.json";

pub(crate) fn tenant_root(root: &Path, tenant_id: &TenantId) -> PathBuf {
    root.join(TENANTS_DIR).join(tenant_id.as_str())
}

pub(crate) fn tenant_sandbox_root(
    root: &Path,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
) -> PathBuf {
    tenant_root(root, tenant_id)
        .join(SANDBOXES_DIR)
        .join(sandbox_id.as_str())
}

pub(crate) fn bundle_dir(root: &Path, tenant_id: &TenantId, sandbox_id: &SandboxId) -> PathBuf {
    tenant_sandbox_root(root, tenant_id, sandbox_id).join(BUNDLE_DIR)
}

pub(crate) fn state_root(root: &Path, tenant_id: &TenantId, sandbox_id: &SandboxId) -> PathBuf {
    tenant_sandbox_root(root, tenant_id, sandbox_id).join(STATE_DIR)
}

pub(crate) fn rootfs_root(root: &Path, tenant_id: &TenantId, sandbox_id: &SandboxId) -> PathBuf {
    tenant_sandbox_root(root, tenant_id, sandbox_id).join(ROOTFS_DIR)
}

pub(crate) fn tenant_volume_dir(root: &Path, tenant_id: &TenantId, volume_name: &str) -> PathBuf {
    tenant_root(root, tenant_id)
        .join(VOLUMES_DIR)
        .join(volume_name)
}

pub(crate) fn remove_tenant_root(root: &Path, tenant_id: &TenantId) -> io::Result<()> {
    let path = tenant_root(root, tenant_id);
    if try_path_exists(&path, "tenant artifact root")? {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub(crate) fn manifest_path(
    state_root: &Path,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
) -> PathBuf {
    self::state_root(state_root, tenant_id, sandbox_id)
        .join(CONTAINERS_DIR)
        .join(sandbox_id.as_str())
        .join(MANIFEST_FILE)
}

pub(crate) fn all_manifest_paths(state_root: &Path) -> io::Result<Vec<PathBuf>> {
    manifest_paths_from_container_state_dirs(all_container_state_dirs(state_root)?)
}

/// Find every canonical per-sandbox container state directory, including a
/// first manifest publication that crashed after creating only a stage file.
pub(crate) fn all_container_state_dirs(state_root: &Path) -> io::Result<Vec<PathBuf>> {
    let tenants_root = state_root.join(TENANTS_DIR);
    if !try_path_exists(&tenants_root, "tenants artifact root")? {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for tenant_entry in fs::read_dir(&tenants_root)? {
        let tenant_entry = tenant_entry?;
        paths.extend(container_state_dirs_under_tenant_root(tenant_entry.path())?);
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn manifest_paths_for_tenant(
    state_root: &Path,
    tenant_id: &TenantId,
) -> io::Result<Vec<PathBuf>> {
    let mut paths = manifest_paths_under_tenant_root(tenant_root(state_root, tenant_id))?;
    paths.sort();
    Ok(paths)
}

pub(crate) fn manifest_path_for_sandbox_id(
    state_root: &Path,
    sandbox_id: &SandboxId,
) -> io::Result<Option<PathBuf>> {
    let tenants_root = state_root.join(TENANTS_DIR);
    if !try_path_exists(&tenants_root, "tenants artifact root")? {
        return Ok(None);
    }

    let mut selected = None;
    for tenant_entry in fs::read_dir(&tenants_root)? {
        let tenant_entry = tenant_entry?;
        let manifest_path = tenant_entry
            .path()
            .join(SANDBOXES_DIR)
            .join(sandbox_id.as_str())
            .join(STATE_DIR)
            .join(CONTAINERS_DIR)
            .join(sandbox_id.as_str())
            .join(MANIFEST_FILE);
        if !try_path_exists(&manifest_path, "sandbox manifest")? {
            continue;
        }
        if selected.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "sandbox id {} has manifests in multiple tenant roots",
                    sandbox_id.as_str()
                ),
            ));
        }
        selected = Some(manifest_path);
    }
    Ok(selected)
}

fn manifest_paths_under_tenant_root(tenant_root: PathBuf) -> io::Result<Vec<PathBuf>> {
    manifest_paths_from_container_state_dirs(container_state_dirs_under_tenant_root(tenant_root)?)
}

fn manifest_paths_from_container_state_dirs(
    container_state_dirs: Vec<PathBuf>,
) -> io::Result<Vec<PathBuf>> {
    let mut manifest_paths = Vec::new();
    for container_state_dir in container_state_dirs {
        let manifest_path = container_state_dir.join(MANIFEST_FILE);
        if try_path_exists(&manifest_path, "sandbox manifest")? {
            manifest_paths.push(manifest_path);
        }
    }
    Ok(manifest_paths)
}

fn container_state_dirs_under_tenant_root(tenant_root: PathBuf) -> io::Result<Vec<PathBuf>> {
    let sandboxes_root = tenant_root.join(SANDBOXES_DIR);
    if !try_path_exists(&sandboxes_root, "tenant sandboxes root")? {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for sandbox_entry in fs::read_dir(&sandboxes_root)? {
        let sandbox_entry = sandbox_entry?;
        let sandbox_name = sandbox_entry.file_name();
        let container_state_dir = sandbox_entry
            .path()
            .join(STATE_DIR)
            .join(CONTAINERS_DIR)
            .join(sandbox_name);
        if try_path_exists(&container_state_dir, "sandbox container state directory")? {
            paths.push(container_state_dir);
        }
    }
    Ok(paths)
}

fn try_path_exists(path: &Path, artifact_kind: &str) -> io::Result<bool> {
    path.try_exists().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot inspect {artifact_kind} {}: {error}", path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn state_directory_discovery_surfaces_metadata_errors() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let sandbox_root = temp_dir
            .path()
            .join(super::TENANTS_DIR)
            .join("tenant")
            .join(super::SANDBOXES_DIR)
            .join("sandbox");
        std::fs::create_dir_all(&sandbox_root).expect("sandbox directory should exist");
        symlink("state", sandbox_root.join(super::STATE_DIR))
            .expect("self-referential state symlink should exist");

        let error = super::all_container_state_dirs(temp_dir.path())
            .expect_err("metadata failure must not be classified as an absent state directory");
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(
            error
                .to_string()
                .contains("sandbox container state directory")
                && error.to_string().contains("state"),
            "the error should identify the state path that could not be inspected: {error}"
        );
    }
}
