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

pub(crate) fn remove_tenant_root(root: &Path, tenant_id: &TenantId) -> io::Result<()> {
    let path = tenant_root(root, tenant_id);
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
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
    let tenants_root = state_root.join(TENANTS_DIR);
    if !tenants_root.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for tenant_entry in fs::read_dir(&tenants_root)? {
        let tenant_entry = tenant_entry?;
        paths.extend(manifest_paths_under_tenant_root(tenant_entry.path())?);
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
    if !tenants_root.exists() {
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
        if !manifest_path.exists() {
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
    let sandboxes_root = tenant_root.join(SANDBOXES_DIR);
    if !sandboxes_root.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for sandbox_entry in fs::read_dir(&sandboxes_root)? {
        let sandbox_entry = sandbox_entry?;
        let sandbox_name = sandbox_entry.file_name();
        let manifest_path = sandbox_entry
            .path()
            .join(STATE_DIR)
            .join(CONTAINERS_DIR)
            .join(sandbox_name)
            .join(MANIFEST_FILE);
        if manifest_path.exists() {
            paths.push(manifest_path);
        }
    }
    Ok(paths)
}
