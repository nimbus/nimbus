use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::buildah::BuildahCli;
use super::command::CommandSpec;
use crate::instance::SandboxId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KrunConmonLayout {
    pub state_root: PathBuf,
    pub container_state_dir: PathBuf,
    pub exit_dir: PathBuf,
    pub persist_dir: PathBuf,
    pub ctr_log: PathBuf,
    pub oci_log: PathBuf,
    pub pidfile: PathBuf,
    pub conmon_pidfile: PathBuf,
    pub exit_status_file: PathBuf,
    pub manifest_path: PathBuf,
}

impl KrunConmonLayout {
    pub(crate) fn new(state_root: impl Into<PathBuf>, sandbox_id: &SandboxId) -> Self {
        let state_root = state_root.into();
        let container_state_dir = state_root.join("containers").join(sandbox_id.as_str());
        let exit_dir = state_root.join("exits");
        let persist_dir = state_root.join("persist").join(sandbox_id.as_str());
        Self {
            ctr_log: container_state_dir.join("ctr.log"),
            oci_log: container_state_dir.join("oci.log"),
            pidfile: container_state_dir.join("pidfile"),
            conmon_pidfile: container_state_dir.join("conmon.pid"),
            exit_status_file: exit_dir.join(sandbox_id.as_str()),
            manifest_path: container_state_dir.join("manifest.json"),
            state_root,
            container_state_dir,
            exit_dir,
            persist_dir,
        }
    }

    pub(crate) fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.container_state_dir)?;
        std::fs::create_dir_all(&self.exit_dir)?;
        std::fs::create_dir_all(&self.persist_dir)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KrunConmonConfig {
    pub conmon_path: PathBuf,
    pub runtime_path: PathBuf,
    pub buildah_path: PathBuf,
    pub use_buildah_unshare: bool,
    pub log_level: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KrunConmonLaunchPlan {
    pub create_command: CommandSpec,
    pub state_command: CommandSpec,
    pub start_command: CommandSpec,
}

pub(crate) fn build_launch_plan(
    config: &KrunConmonConfig,
    layout: &KrunConmonLayout,
    sandbox_id: &SandboxId,
    sandbox_name: &str,
    bundle_dir: &Path,
    buildah_container_name: Option<&str>,
    create_prelude: &[String],
) -> KrunConmonLaunchPlan {
    let create_command = CommandSpec::new(config.conmon_path.clone()).args([
        "--api-version".to_owned(),
        "1".to_owned(),
        "-c".to_owned(),
        sandbox_id.as_str().to_owned(),
        "-u".to_owned(),
        sandbox_id.as_str().to_owned(),
        "-r".to_owned(),
        config.runtime_path.to_string_lossy().into_owned(),
        "-b".to_owned(),
        bundle_dir.to_string_lossy().into_owned(),
        "-p".to_owned(),
        layout.pidfile.to_string_lossy().into_owned(),
        "-n".to_owned(),
        sandbox_name.to_owned(),
        "--exit-dir".to_owned(),
        layout.exit_dir.to_string_lossy().into_owned(),
        "--persist-dir".to_owned(),
        layout.persist_dir.to_string_lossy().into_owned(),
        "--full-attach".to_owned(),
        "-l".to_owned(),
        format!("k8s-file:{}", layout.ctr_log.display()),
        "--log-level".to_owned(),
        config.log_level.clone(),
        "--syslog".to_owned(),
        "--conmon-pidfile".to_owned(),
        layout.conmon_pidfile.to_string_lossy().into_owned(),
        "--runtime-arg".to_owned(),
        "--log-format=json".to_owned(),
        "--runtime-arg".to_owned(),
        "--log".to_owned(),
        "--runtime-arg".to_owned(),
        layout.oci_log.to_string_lossy().into_owned(),
    ]);

    let state_command = CommandSpec::new(config.runtime_path.clone())
        .arg("state")
        .arg(sandbox_id.as_str().to_owned());
    let start_command = CommandSpec::new(config.runtime_path.clone())
        .arg("start")
        .arg(sandbox_id.as_str().to_owned());

    let buildah =
        BuildahCli::new(config.buildah_path.clone()).with_unshare(config.use_buildah_unshare);

    // For image-backed sandboxes, the conmon create command must run inside the
    // same buildah unshare session that mounts the rootfs overlay, because crun
    // needs the rootfs accessible during the create phase.
    //
    // The state and start commands must NOT run inside buildah unshare because
    // the crun state directory was created by the conmon create session's user
    // namespace.  A fresh buildah unshare session creates a different UID mapping,
    // which cannot read or signal the existing container state.  Running them as
    // the real host user (no unshare) works because crun stores state under the
    // real user's XDG_RUNTIME_DIR.
    KrunConmonLaunchPlan {
        create_command: buildah.maybe_wrap_with_mount_prelude(
            create_command,
            buildah_container_name,
            create_prelude,
        ),
        state_command: if buildah_container_name.is_some() {
            state_command
        } else {
            buildah.maybe_wrap(state_command)
        },
        start_command: if buildah_container_name.is_some() {
            start_command
        } else {
            buildah.maybe_wrap(start_command)
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{KrunConmonConfig, KrunConmonLayout, build_launch_plan};
    use crate::instance::SandboxId;

    #[test]
    fn conmon_launch_plan_uses_private_runtime_and_buildah_unshare() {
        let sandbox_id = SandboxId::new("db-01");
        let layout = KrunConmonLayout::new("/tmp/neovex-sandbox-state", &sandbox_id);
        let config = KrunConmonConfig {
            conmon_path: Path::new("/usr/bin/conmon").into(),
            runtime_path: Path::new("/usr/libexec/neovex/crun").into(),
            buildah_path: Path::new("/usr/bin/buildah").into(),
            use_buildah_unshare: true,
            log_level: "debug".to_owned(),
        };

        let launch_plan = build_launch_plan(
            &config,
            &layout,
            &sandbox_id,
            "db",
            Path::new("/tmp/neovex-bundles/db-01"),
            None,
            &[],
        );

        assert_eq!(
            launch_plan.create_command.program,
            PathBuf::from("/usr/bin/buildah")
        );
        assert_eq!(
            launch_plan.create_command.args.first().map(String::as_str),
            Some("unshare")
        );
        assert!(
            launch_plan
                .create_command
                .args
                .iter()
                .any(|arg| arg == "/usr/libexec/neovex/crun"),
            "create command should retain the private neovex crun path"
        );
        assert_eq!(
            launch_plan.start_command.program,
            PathBuf::from("/usr/bin/buildah")
        );
    }

    #[test]
    fn conmon_launch_plan_injects_mount_prelude_for_image_backed_sandboxes() {
        let sandbox_id = SandboxId::new("db-02");
        let layout = KrunConmonLayout::new("/tmp/neovex-sandbox-state", &sandbox_id);
        let config = KrunConmonConfig {
            conmon_path: Path::new("/usr/bin/conmon").into(),
            runtime_path: Path::new("/usr/libexec/neovex/crun").into(),
            buildah_path: Path::new("/usr/bin/buildah").into(),
            use_buildah_unshare: true,
            log_level: "debug".to_owned(),
        };

        let launch_plan = build_launch_plan(
            &config,
            &layout,
            &sandbox_id,
            "db",
            Path::new("/tmp/neovex-bundles/db-02"),
            Some("db-02-image"),
            &[r#"printf '%s' '{"cpus":2,"ram_mib":256}' > /tmp/rootfs/.krun_vm.json"#.to_owned()],
        );

        let script = launch_plan
            .create_command
            .args
            .last()
            .expect("buildah unshare launch should carry a shell script");
        assert!(
            script.contains("buildah mount db-02-image >/dev/null"),
            "expected buildah mount inside the unshare shell script: {script}"
        );
        assert!(
            script.contains(".krun_vm.json"),
            "expected krun vm config prelude inside the unshare shell script: {script}"
        );
        assert!(
            script.contains("/usr/bin/conmon"),
            "expected conmon launch to remain the final command in the shell script: {script}"
        );
    }
}
