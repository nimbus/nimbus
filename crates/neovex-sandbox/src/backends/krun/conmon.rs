use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::buildah::BuildahBinary;
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

    if config.use_buildah_unshare {
        let buildah = BuildahBinary::new(config.buildah_path.clone());
        return KrunConmonLaunchPlan {
            create_command: buildah.wrap_unshare(&create_command),
            state_command: buildah.wrap_unshare(&state_command),
            start_command: buildah.wrap_unshare(&start_command),
        };
    }

    KrunConmonLaunchPlan {
        create_command,
        state_command,
        start_command,
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
}
