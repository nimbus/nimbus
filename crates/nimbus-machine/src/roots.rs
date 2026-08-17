//! Machine root layout: the config/state/data/cache/runtime root directories
//! and the per-machine [`MachinePaths`] derived from them.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use nimbus_core::Error;
use serde::{Deserialize, Serialize};

use crate::paths::{
    MachinePaths, resolve_cache_root_with_env, resolve_config_root_with_env,
    resolve_data_root_with_env, resolve_runtime_root_with_env, resolve_state_root_with_env,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineRootLayout {
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    pub data_root: PathBuf,
    pub cache_root: PathBuf,
    pub runtime_root: PathBuf,
}

impl MachineRootLayout {
    pub fn resolve() -> Result<Self, Error> {
        Self::resolve_with_env(|name| env::var_os(name))
    }

    fn resolve_with_env(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Result<Self, Error> {
        Ok(Self {
            config_root: resolve_config_root_with_env(&mut lookup)?,
            state_root: resolve_state_root_with_env(&mut lookup)?,
            data_root: resolve_data_root_with_env(&mut lookup)?,
            cache_root: resolve_cache_root_with_env(&mut lookup)?,
            runtime_root: resolve_runtime_root_with_env(&mut lookup),
        })
    }

    pub fn guest_api_default(runtime_root: PathBuf) -> Self {
        Self {
            config_root: PathBuf::from("/var/lib/nimbus/machine/config"),
            state_root: PathBuf::from("/var/lib/nimbus/machine/state"),
            data_root: PathBuf::from("/var/lib/nimbus/machine/data"),
            cache_root: PathBuf::from("/var/lib/nimbus/machine/cache"),
            runtime_root,
        }
    }

    pub fn new(
        config_root: PathBuf,
        state_root: PathBuf,
        data_root: PathBuf,
        cache_root: PathBuf,
        runtime_root: PathBuf,
    ) -> Self {
        Self {
            config_root,
            state_root,
            data_root,
            cache_root,
            runtime_root,
        }
    }

    pub fn from_sibling_roots(
        config_root: PathBuf,
        state_root: PathBuf,
        runtime_root: PathBuf,
    ) -> Result<Self, Error> {
        let shared_parent = config_root
            .parent()
            .map(Path::to_path_buf)
            .and_then(|config_parent| {
                (state_root.parent() == Some(config_parent.as_path())
                    && runtime_root.parent() == Some(config_parent.as_path()))
                .then_some(config_parent)
            })
            .ok_or_else(|| {
                Error::InvalidInput(
                    "machine config, state, and runtime roots must share a parent when deriving data/cache roots"
                        .to_owned(),
                )
            })?;
        Ok(Self::new(
            config_root,
            state_root,
            shared_parent.join("data"),
            shared_parent.join("cache"),
            runtime_root,
        ))
    }

    #[doc(hidden)]
    pub fn test_sibling_roots(
        config_root: PathBuf,
        state_root: PathBuf,
        runtime_root: PathBuf,
    ) -> Self {
        Self::from_sibling_roots(config_root, state_root, runtime_root)
            .expect("machine test roots must share a parent")
    }

    pub fn lock_path(&self, name: &str) -> PathBuf {
        self.state_root.join(format!("{name}.lock"))
    }

    pub fn paths(&self, name: &str) -> MachinePaths {
        let config_dir = self.config_root.join(name);
        let state_dir = self.state_root.join(name);
        let data_dir = self.data_root.join(name);
        let runtime_dir = self.runtime_root.clone();
        MachinePaths {
            name: name.to_owned(),
            config_dir: config_dir.clone(),
            state_dir: state_dir.clone(),
            data_dir: data_dir.clone(),
            runtime_dir: runtime_dir.clone(),
            config_path: config_dir.join("config.json"),
            generated_ignition_path: config_dir.join("generated.ign"),
            state_path: state_dir.join("status.json"),
            guest_config_bundle_dir: state_dir.join("machine-config"),
            image_cache_dir: self.cache_root.join("images"),
            guest_binary_cache_dir: self.cache_root.join("guest-nimbus"),
            materialized_image_path: data_dir.join("images").join(format!("{name}.raw")),
            api_socket_path: runtime_dir.join(format!("{name}-api.sock")),
            ready_socket_path: runtime_dir.join(format!("{name}.sock")),
            ignition_socket_path: runtime_dir.join(format!("{name}-ignition.sock")),
            gvproxy_socket_path: runtime_dir.join(format!("{name}-gvproxy.sock")),
            vmm_endpoint_path: runtime_dir.join(format!("{name}-vmm.sock")),
            efi_variable_store_path: data_dir.join("efi-variable-store"),
            api_forward_pid_path: runtime_dir.join(format!("{name}-api-forward.pid")),
            gvproxy_pid_path: runtime_dir.join(format!("{name}-gvproxy.pid")),
            gvproxy_process_identity_path: runtime_dir.join(format!("{name}-gvproxy-process.json")),
            vmm_pid_path: runtime_dir.join(format!("{name}-vmm.pid")),
            api_forward_log_path: runtime_dir.join(format!("{name}-api-forward.log")),
            machine_log_path: runtime_dir.join(format!("{name}.log")),
            gvproxy_log_path: runtime_dir.join(format!("{name}-gvproxy.log")),
            vmm_log_path: runtime_dir.join(format!("{name}-vmm.log")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::MachineRootLayout;
    use crate::paths::{DEFAULT_MACHINE_RUNTIME_ROOT, MACHINE_RUNTIME_ROOT_ENV};

    #[test]
    fn machine_root_layout_new_uses_explicit_roots() {
        let layout = MachineRootLayout::new(
            PathBuf::from("root/config"),
            PathBuf::from("root/state"),
            PathBuf::from("root/data"),
            PathBuf::from("root/cache"),
            PathBuf::from("root/runtime"),
        );

        assert_eq!(layout.data_root, PathBuf::from("root/data"));
        assert_eq!(layout.cache_root, PathBuf::from("root/cache"));
        assert_eq!(
            serde_json::to_value(&layout).expect("machine roots should serialize"),
            serde_json::json!({
                "config_root": "root/config",
                "state_root": "root/state",
                "data_root": "root/data",
                "cache_root": "root/cache",
                "runtime_root": "root/runtime",
            }),
            "machine roots must contain artifacts only"
        );

        let mut obsolete_mixed_root =
            serde_json::to_value(&layout).expect("machine roots should serialize");
        obsolete_mixed_root
            .as_object_mut()
            .expect("machine roots wire should be an object")
            .insert(
                "network_state_root".to_owned(),
                serde_json::json!("root/state"),
            );
        assert!(
            serde_json::from_value::<MachineRootLayout>(obsolete_mixed_root).is_err(),
            "the removed mixed-root field must not be admitted as compatibility data"
        );
    }

    #[test]
    fn machine_root_layout_from_sibling_roots_derives_data_and_cache() {
        let layout = MachineRootLayout::from_sibling_roots(
            PathBuf::from("root/config"),
            PathBuf::from("root/state"),
            PathBuf::from("root/runtime"),
        )
        .expect("sibling roots should derive");

        assert_eq!(layout.data_root, PathBuf::from("root/data"));
        assert_eq!(layout.cache_root, PathBuf::from("root/cache"));
    }

    #[test]
    fn machine_root_layout_from_sibling_roots_rejects_unshared_roots() {
        let error = MachineRootLayout::from_sibling_roots(
            PathBuf::from("config-root/config"),
            PathBuf::from("state-root/state"),
            PathBuf::from("runtime-root/runtime"),
        )
        .expect_err("unshared roots should be rejected");

        assert!(
            error.to_string().contains("must share a parent"),
            "error should reject derived roots without falling back to /tmp: {error}"
        );
    }

    #[test]
    fn machine_root_layout_resolve_uses_injected_xdg_and_runtime_env() {
        let layout = MachineRootLayout::resolve_with_env(env_lookup(&[
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_STATE_HOME", "/xdg/state"),
            ("XDG_DATA_HOME", "/xdg/data"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
            (MACHINE_RUNTIME_ROOT_ENV, "/run/nimbus-machine"),
            ("NIMBUS_CONTROL_DATA_DIR", "/must/not/become/a/machine/root"),
        ]))
        .expect("xdg roots should resolve");

        assert_eq!(
            layout.config_root,
            PathBuf::from("/xdg/config/nimbus/machine")
        );
        assert_eq!(
            layout.state_root,
            PathBuf::from("/xdg/state/nimbus/machine")
        );
        assert_eq!(layout.data_root, PathBuf::from("/xdg/data/nimbus/machine"));
        assert_eq!(
            layout.cache_root,
            PathBuf::from("/xdg/cache/nimbus/machine")
        );
        assert_eq!(layout.runtime_root, PathBuf::from("/run/nimbus-machine"));
        assert!(
            !serde_json::to_string(&layout)
                .expect("machine roots should serialize")
                .contains("network"),
            "network authority must not enter the artifact-root record"
        );
    }

    #[test]
    fn machine_root_layout_resolve_falls_back_to_home_and_default_runtime() {
        let layout = MachineRootLayout::resolve_with_env(env_lookup(&[("HOME", "/home/alice")]))
            .expect("home fallback should resolve");

        assert_eq!(
            layout.config_root,
            PathBuf::from("/home/alice/.config/nimbus/machine")
        );
        assert_eq!(
            layout.state_root,
            PathBuf::from("/home/alice/.local/state/nimbus/machine")
        );
        assert_eq!(
            layout.data_root,
            PathBuf::from("/home/alice/.local/share/nimbus/machine")
        );
        assert_eq!(
            layout.cache_root,
            PathBuf::from("/home/alice/.cache/nimbus/machine")
        );
        assert_eq!(
            layout.runtime_root,
            PathBuf::from(DEFAULT_MACHINE_RUNTIME_ROOT)
        );
    }

    #[test]
    fn machine_root_layout_resolve_errors_without_home() {
        let error = MachineRootLayout::resolve_with_env(env_lookup(&[]))
            .expect_err("missing home should fail");

        assert!(error.to_string().contains("HOME is not set"), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn machine_root_layout_resolve_uses_windows_profile_fallback() {
        let layout =
            MachineRootLayout::resolve_with_env(env_lookup(&[("USERPROFILE", "C:\\Users\\Alice")]))
                .expect("windows profile fallback should resolve");

        assert_eq!(
            layout.config_root,
            PathBuf::from("C:\\Users\\Alice").join(".config/nimbus/machine")
        );
    }

    fn env_lookup(entries: &[(&str, &str)]) -> impl FnMut(&str) -> Option<OsString> {
        let values = entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
            .collect::<BTreeMap<_, _>>();
        move |name| values.get(name).cloned()
    }
}
