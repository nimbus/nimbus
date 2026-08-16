use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use nimbus::{Error, SandboxBackendKind, TenantId};
use nimbus_sandbox::backends::krun::KrunSandboxBackendConfig;
use sha2::{Digest, Sha256};

use crate::compose::discovery::ResolvedComposeSelection;
use crate::compose::file::ComposeProjectPlan;
use crate::provider_binaries::{
    apply_resolved_krun_runtime_paths, default_container_provider_binary_dirs,
};

const SERVICES_CONTROL_ROOT: &str = "services";
const PROJECTS_CONTROL_ROOT: &str = "projects";
const BACKENDS_CONTROL_ROOT: &str = "backends";
const CONTAINER_BACKEND_ROOT: &str = "container";
const KRUN_BACKEND_ROOT: &str = "krun";
const LOCAL_SERVICE_TENANT_PREFIX: &str = "svc";
const PROJECT_KEY_HASH_HEX_LEN: usize = 12;
const PROJECT_KEY_SLUG_LEN: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposeProjectContext {
    pub(crate) selection: ResolvedComposeSelection,
    pub(crate) plan: ComposeProjectPlan,
    pub(crate) control_plane: ComposeProjectControlPlane,
}

impl ComposeProjectContext {
    #[cfg(test)]
    pub(crate) fn load(file: &Path, control_data_dir: &Path) -> Result<Self, Error> {
        Self::load_selection(
            &ResolvedComposeSelection::explicit(file.to_path_buf()),
            control_data_dir,
        )
    }

    pub(crate) fn load_selection(
        selection: &ResolvedComposeSelection,
        control_data_dir: &Path,
    ) -> Result<Self, Error> {
        Self::load_selection_with_admission(
            selection,
            control_data_dir,
            crate::compose::file::ComposeAdmissionMode::LocalDevelopment,
        )
    }

    pub(crate) fn load_selection_with_admission(
        selection: &ResolvedComposeSelection,
        control_data_dir: &Path,
        admission_mode: crate::compose::file::ComposeAdmissionMode,
    ) -> Result<Self, Error> {
        let plan = ComposeProjectPlan::load_selection_with_admission(selection, admission_mode)?;
        let control_plane = ComposeProjectControlPlane::from_plan(&plan, control_data_dir)?;
        Ok(Self {
            selection: selection.clone(),
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

    pub(crate) fn backend_root(&self, backend: SandboxBackendKind) -> PathBuf {
        self.project_root
            .join(BACKENDS_CONTROL_ROOT)
            .join(backend_root_name(backend))
    }

    pub(crate) fn krun_backend_root(&self) -> PathBuf {
        self.backend_root(SandboxBackendKind::Krun)
    }

    /// Explicit test-only reconstruction for isolated state-view fixtures.
    #[cfg(test)]
    pub(crate) fn reconstruct_direct_krun_backend_config(&self) -> KrunSandboxBackendConfig {
        KrunSandboxBackendConfig::under_root(self.krun_backend_root())
    }

    pub(crate) fn krun_backend_config_with_network_authority(
        &self,
        network_state_root: &Path,
    ) -> KrunSandboxBackendConfig {
        let path_env = std::env::var_os("PATH");
        let helper_binary_dirs = default_container_provider_binary_dirs();
        self.krun_backend_config_with_provider_search(
            network_state_root,
            path_env.as_deref(),
            &helper_binary_dirs,
        )
    }

    fn krun_backend_config_with_provider_search(
        &self,
        network_state_root: &Path,
        path_env: Option<&OsStr>,
        helper_binary_dirs: &[PathBuf],
    ) -> KrunSandboxBackendConfig {
        let mut config = KrunSandboxBackendConfig::under_root(self.krun_backend_root())
            .with_network_state_root(network_state_root);
        apply_resolved_krun_runtime_paths(&mut config, path_env, helper_binary_dirs);
        config
    }
}

fn backend_root_name(backend: SandboxBackendKind) -> &'static str {
    match backend {
        SandboxBackendKind::Container => CONTAINER_BACKEND_ROOT,
        SandboxBackendKind::Krun => KRUN_BACKEND_ROOT,
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
        "nimbus".to_owned()
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::discovery::{ResolvedComposeSelection, resolve_compose_selection};
    use crate::test_support::with_current_dir;

    #[cfg(unix)]
    fn write_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, "#!/bin/sh\nexit 0\n").expect("provider helper should write");
        let mut permissions = fs::metadata(path)
            .expect("provider helper metadata should read")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("provider helper should be executable");
    }

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
        assert_eq!(
            context
                .control_plane
                .backend_root(SandboxBackendKind::Container),
            context
                .control_plane
                .project_root
                .join("backends")
                .join("container")
        );

        let network_state_root = tempdir.path().join("logical-node-network");
        let config = context
            .control_plane
            .krun_backend_config_with_network_authority(&network_state_root);
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
            config.workload_state_root,
            context
                .control_plane
                .project_root
                .join("backends")
                .join("krun")
                .join("state")
        );
        assert_eq!(config.network_state_root, network_state_root);
        assert_ne!(config.network_state_root, config.workload_state_root);
        assert_eq!(
            context.control_plane.local_tenant_id.as_str(),
            format!("svc-{}", context.control_plane.project_key)
        );
    }

    #[cfg(unix)]
    #[test]
    fn direct_compose_krun_resolves_network_helpers_outside_path() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose_path = write_compose_fixture(
            &tempdir,
            "stack/compose.yaml",
            "name: demo\nservices:\n  api:\n    image: busybox:latest\n",
        );
        let context = ComposeProjectContext::load(&compose_path, &tempdir.path().join("control"))
            .expect("context should load");
        let helper_dir = tempdir.path().join("provider-helpers");
        fs::create_dir_all(&helper_dir).expect("provider helper directory should build");
        let netavark = helper_dir.join("netavark");
        let aardvark_dns = helper_dir.join("aardvark-dns");
        write_executable(&netavark);
        write_executable(&aardvark_dns);

        let config = context
            .control_plane
            .krun_backend_config_with_provider_search(
                &tempdir.path().join("network"),
                Some(OsStr::new("/usr/bin:/bin")),
                &[helper_dir],
            );

        assert_eq!(config.netavark_path, netavark);
        assert_eq!(config.aardvark_dns_path, aardvark_dns);
    }

    #[cfg(unix)]
    #[test]
    fn direct_compose_krun_preserves_private_runtime_when_path_has_stock_crun() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose_path = write_compose_fixture(
            &tempdir,
            "stack/compose.yaml",
            "name: demo\nservices:\n  api:\n    image: busybox:latest\n",
        );
        let context = ComposeProjectContext::load(&compose_path, &tempdir.path().join("control"))
            .expect("context should load");
        let stock_binary_dir = tempdir.path().join("stock-bin");
        fs::create_dir_all(&stock_binary_dir).expect("stock binary directory should build");
        write_executable(&stock_binary_dir.join("crun"));
        let expected_runtime =
            KrunSandboxBackendConfig::under_root(context.control_plane.krun_backend_root())
                .runtime_path;

        let config = context
            .control_plane
            .krun_backend_config_with_provider_search(
                &tempdir.path().join("network"),
                Some(stock_binary_dir.as_os_str()),
                &[],
            );

        assert_eq!(config.runtime_path, expected_runtime);
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

    #[test]
    fn compose_project_context_load_selection_keeps_primary_file_identity() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let base = write_compose_fixture(
            &tempdir,
            "stack/compose.yaml",
            "name: demo\nservices:\n  api:\n    image: busybox:latest\n",
        );
        let override_file = write_compose_fixture(
            &tempdir,
            "stack/compose.override.yaml",
            "services:\n  worker:\n    image: redis:7\n",
        );
        let control_data_dir = tempdir.path().join("control");
        let auto_selection = resolve_compose_selection(&[], &tempdir.path().join("stack"))
            .expect("selection should resolve")
            .expect("selection should exist");
        let explicit_selection = ResolvedComposeSelection::explicit(base.clone());

        let auto_context =
            ComposeProjectContext::load_selection(&auto_selection, &control_data_dir)
                .expect("auto context should load");
        let explicit_context =
            ComposeProjectContext::load_selection(&explicit_selection, &control_data_dir)
                .expect("explicit context should load");

        assert_eq!(
            auto_context.selection.files,
            vec![base.clone(), override_file]
        );
        assert_eq!(
            auto_context.control_plane.compose_file,
            fs::canonicalize(&base).unwrap()
        );
        assert_eq!(
            auto_context.control_plane.project_key,
            explicit_context.control_plane.project_key
        );
        assert_eq!(
            auto_context.control_plane.local_tenant_id,
            explicit_context.control_plane.local_tenant_id
        );
    }

    #[test]
    fn compose_command_auto_discovers_compose_project() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let stack = tempdir.path().join("stack");
        let nested = stack.join("app");
        fs::create_dir_all(&nested).expect("nested directory should build");
        let compose = write_compose_fixture(
            &tempdir,
            "stack/compose.yaml",
            "name: demo\nservices:\n  db:\n    image: busybox:latest\n",
        );

        let selection = with_current_dir(&nested, || {
            crate::compose::resolve_required_compose_selection(&[])
        })
        .expect("compose command should auto-discover project");

        assert_eq!(
            fs::canonicalize(selection.primary_file()).unwrap(),
            fs::canonicalize(compose).unwrap()
        );
        assert_eq!(
            fs::canonicalize(selection.project_root).unwrap(),
            fs::canonicalize(stack).unwrap()
        );
    }
}
