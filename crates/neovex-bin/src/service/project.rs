use std::fs;
use std::path::{Path, PathBuf};

use neovex::{Error, TenantId};
use neovex_sandbox::backends::krun::KrunSandboxBackendConfig;
use sha2::{Digest, Sha256};

use crate::service::compose::ComposeProjectPlan;

const SERVICES_CONTROL_ROOT: &str = "services";
const PROJECTS_CONTROL_ROOT: &str = "projects";
const BACKENDS_CONTROL_ROOT: &str = "backends";
const KRUN_BACKEND_ROOT: &str = "krun";
const LOCAL_SERVICE_TENANT_PREFIX: &str = "svc";
const PROJECT_KEY_HASH_HEX_LEN: usize = 12;
const PROJECT_KEY_SLUG_LEN: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposeProjectContext {
    pub(crate) plan: ComposeProjectPlan,
    pub(crate) control_plane: ComposeProjectControlPlane,
}

impl ComposeProjectContext {
    pub(crate) fn load(file: &Path, control_data_dir: &Path) -> Result<Self, Error> {
        let plan = ComposeProjectPlan::load(file)?;
        let control_plane = ComposeProjectControlPlane::from_plan(&plan, control_data_dir)?;
        Ok(Self {
            plan,
            control_plane,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposeProjectControlPlane {
    pub(crate) compose_file: PathBuf,
    pub(crate) compose_root: PathBuf,
    pub(crate) project_name: String,
    pub(crate) project_key: String,
    pub(crate) local_tenant_id: TenantId,
    pub(crate) project_root: PathBuf,
}

impl ComposeProjectControlPlane {
    pub(crate) fn from_plan(
        plan: &ComposeProjectPlan,
        control_data_dir: &Path,
    ) -> Result<Self, Error> {
        let compose_file = fs::canonicalize(&plan.source_file).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to canonicalize compose file {}: {error}",
                plan.source_file.display()
            ))
        })?;
        let compose_root = compose_file
            .parent()
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "compose file {} must have a parent directory",
                    compose_file.display()
                ))
            })?
            .to_path_buf();
        let project_key = derive_project_key(&plan.project_name, &compose_file);
        let local_tenant_id =
            TenantId::new(format!("{LOCAL_SERVICE_TENANT_PREFIX}-{project_key}"))?;
        let project_root = control_data_dir
            .join(SERVICES_CONTROL_ROOT)
            .join(PROJECTS_CONTROL_ROOT)
            .join(&project_key);

        Ok(Self {
            compose_file,
            compose_root,
            project_name: plan.project_name.clone(),
            project_key,
            local_tenant_id,
            project_root,
        })
    }

    pub(crate) fn krun_backend_root(&self) -> PathBuf {
        self.project_root
            .join(BACKENDS_CONTROL_ROOT)
            .join(KRUN_BACKEND_ROOT)
    }

    pub(crate) fn krun_backend_config(&self) -> KrunSandboxBackendConfig {
        KrunSandboxBackendConfig::under_root(self.krun_backend_root())
    }
}

fn derive_project_key(project_name: &str, compose_file: &Path) -> String {
    let slug = truncate_ascii(project_name, PROJECT_KEY_SLUG_LEN);
    let mut hasher = Sha256::new();
    hasher.update(compose_file.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let mut hash = String::with_capacity(PROJECT_KEY_HASH_HEX_LEN);
    for byte in digest.iter().take(PROJECT_KEY_HASH_HEX_LEN / 2) {
        hash.push_str(&format!("{byte:02x}"));
    }
    format!("{slug}-{hash}")
}

fn truncate_ascii(value: &str, max_len: usize) -> String {
    let mut truncated = String::with_capacity(value.len().min(max_len));
    for character in value.chars().take(max_len) {
        truncated.push(character);
    }
    if truncated.is_empty() {
        "neovex".to_owned()
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_compose_fixture(
        tempdir: &tempfile::TempDir,
        relative_path: &str,
        body: &str,
    ) -> PathBuf {
        let path = tempdir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should build");
        }
        fs::write(&path, body).expect("compose fixture should write");
        path
    }

    #[test]
    fn compose_project_context_derives_project_scoped_control_roots() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose_path = write_compose_fixture(
            &tempdir,
            "stack/compose.yaml",
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
"#,
        );
        let control_data_dir = tempdir.path().join("control");

        let context = ComposeProjectContext::load(&compose_path, &control_data_dir)
            .expect("context should load");

        assert_eq!(context.control_plane.project_name, "demo-app");
        assert!(context.control_plane.project_key.starts_with("demo-app-"));
        assert_eq!(
            context.control_plane.project_root,
            control_data_dir
                .join("services")
                .join("projects")
                .join(&context.control_plane.project_key)
        );
        assert_eq!(
            context.control_plane.krun_backend_root(),
            context
                .control_plane
                .project_root
                .join("backends")
                .join("krun")
        );

        let config = context.control_plane.krun_backend_config();
        assert_eq!(
            config.bundle_root,
            context
                .control_plane
                .project_root
                .join("backends")
                .join("krun")
                .join("bundles")
        );
        assert_eq!(
            config.state_root,
            context
                .control_plane
                .project_root
                .join("backends")
                .join("krun")
                .join("state")
        );
        assert_eq!(
            context.control_plane.local_tenant_id.as_str(),
            format!("svc-{}", context.control_plane.project_key)
        );
    }

    #[test]
    fn compose_project_key_disambiguates_same_project_name_in_different_roots() {
        let first = tempfile::tempdir().expect("first tempdir should build");
        let second = tempfile::tempdir().expect("second tempdir should build");
        let first_compose = write_compose_fixture(
            &first,
            "alpha/compose.yaml",
            "name: demo\nservices:\n  db:\n    image: busybox:latest\n",
        );
        let second_compose = write_compose_fixture(
            &second,
            "beta/compose.yaml",
            "name: demo\nservices:\n  db:\n    image: busybox:latest\n",
        );
        let control_root = tempfile::tempdir().expect("control tempdir should build");

        let first_context = ComposeProjectContext::load(&first_compose, control_root.path())
            .expect("first context should load");
        let second_context = ComposeProjectContext::load(&second_compose, control_root.path())
            .expect("second context should load");

        assert_eq!(first_context.control_plane.project_name, "demo");
        assert_eq!(second_context.control_plane.project_name, "demo");
        assert_ne!(
            first_context.control_plane.project_key,
            second_context.control_plane.project_key
        );
    }
}
