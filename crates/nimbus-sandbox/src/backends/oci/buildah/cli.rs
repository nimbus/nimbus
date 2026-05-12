#[cfg(test)]
use super::inspect::parse_inspect_output;
#[cfg(test)]
use super::render::localhost_image_reference;
use super::render::{display_command, render_command_failure, shell_escape};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildahCli {
    pub(super) path: PathBuf,
    pub(super) launcher_args: Vec<String>,
    pub(super) use_unshare: bool,
}

impl BuildahCli {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            launcher_args: Vec::new(),
            use_unshare: false,
        }
    }

    #[cfg(test)]
    pub fn with_launcher_args<I, S>(mut self, launcher_args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.launcher_args = launcher_args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_unshare(mut self, use_unshare: bool) -> Self {
        self.use_unshare = use_unshare;
        self
    }

    fn launcher_command(&self) -> CommandSpec {
        CommandSpec::new(self.path.clone()).args(self.launcher_args.iter().cloned())
    }

    fn launcher_command_prefix(&self) -> String {
        std::iter::once(shell_escape(&self.path.to_string_lossy()))
            .chain(self.launcher_args.iter().map(|arg| shell_escape(arg)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn wrap_unshare(&self, command: &CommandSpec) -> CommandSpec {
        let program = command.program.to_string_lossy().into_owned();
        self.launcher_command()
            .arg("unshare")
            .arg("--")
            .arg(program)
            .args(command.args.iter().cloned())
    }

    pub fn maybe_wrap(&self, command: CommandSpec) -> CommandSpec {
        if self.use_unshare {
            self.wrap_unshare(&command)
        } else {
            command
        }
    }

    /// Wrap a command in `buildah unshare -- sh -c 'buildah mount <container> >/dev/null && <command>'`.
    /// This ensures the buildah overlay rootfs mount exists in the same user
    /// namespace session that runs the wrapped command.
    #[cfg(test)]
    pub fn wrap_unshare_with_mount(
        &self,
        container_name: &str,
        command: &CommandSpec,
    ) -> CommandSpec {
        self.wrap_unshare_with_mount_prelude(container_name, &[], command)
    }

    /// Like `wrap_unshare_with_mount`, but inserts shell-safe prelude commands
    /// between the mount step and the wrapped program.
    pub fn wrap_unshare_with_mount_prelude(
        &self,
        container_name: &str,
        prelude_commands: &[String],
        command: &CommandSpec,
    ) -> CommandSpec {
        let mount_cmd = format!(
            "{} mount {} >/dev/null",
            self.launcher_command_prefix(),
            shell_escape(container_name),
        );
        let wrapped_command = format!(
            "{} {}",
            shell_escape(&command.program.to_string_lossy()),
            command
                .args
                .iter()
                .map(|arg| shell_escape(arg))
                .collect::<Vec<_>>()
                .join(" "),
        );
        let mut inner_steps = Vec::with_capacity(prelude_commands.len() + 2);
        inner_steps.push(mount_cmd);
        inner_steps.extend(prelude_commands.iter().cloned());
        inner_steps.push(wrapped_command);
        let inner_cmd = inner_steps.join(" && ");
        self.launcher_command()
            .arg("unshare")
            .arg("--")
            .arg("sh")
            .arg("-c")
            .arg(inner_cmd)
    }

    pub fn maybe_wrap_with_mount_prelude(
        &self,
        command: CommandSpec,
        container_name: Option<&str>,
        prelude_commands: &[String],
    ) -> CommandSpec {
        match (self.use_unshare, container_name) {
            (true, Some(name)) => {
                self.wrap_unshare_with_mount_prelude(name, prelude_commands, &command)
            }
            (true, None) => self.wrap_unshare(&command),
            (false, _) => command,
        }
    }

    #[cfg(test)]
    pub fn create_from_image_command(
        &self,
        container_name: &str,
        image_reference: &str,
    ) -> CommandSpec {
        self.launcher_command()
            .arg("from")
            .arg("--name")
            .arg(container_name.to_owned())
            .arg(image_reference.to_owned())
    }

    #[cfg(test)]
    pub fn mount_command(&self, container_name: &str) -> CommandSpec {
        self.launcher_command()
            .arg("mount")
            .arg(container_name.to_owned())
    }

    #[cfg(test)]
    pub fn inspect_command(&self, container_name: &str) -> CommandSpec {
        self.launcher_command()
            .arg("inspect")
            .arg("--type")
            .arg("container")
            .arg(container_name.to_owned())
    }

    pub fn unmount_command(&self, container_name: &str) -> CommandSpec {
        self.launcher_command()
            .arg("umount")
            .arg(container_name.to_owned())
    }

    pub fn remove_command(&self, container_name: &str) -> CommandSpec {
        self.launcher_command()
            .arg("rm")
            .arg(container_name.to_owned())
    }

    #[cfg(test)]
    pub fn build_command(
        &self,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> CommandSpec {
        self.launcher_command()
            .arg("bud")
            .arg("-t")
            .arg(image_name.to_owned())
            .arg("-f")
            .arg(dockerfile_path.to_string_lossy().into_owned())
            .arg(context_path.to_string_lossy().into_owned())
    }

    #[cfg(test)]
    pub fn pull(&self, session_name: &str, image_reference: &str) -> Result<MountedRootfsSession> {
        self.run_checked(
            &self.create_from_image_command(session_name, image_reference),
            false,
            "create a working container from an image",
        )?;
        Ok(MountedRootfsSession {
            session_name: session_name.to_owned(),
            image_reference: image_reference.to_owned(),
        })
    }

    #[cfg(test)]
    pub fn build(
        &self,
        image_name: &str,
        session_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> Result<MountedRootfsSession> {
        self.run_checked(
            &self.build_command(image_name, dockerfile_path, context_path),
            false,
            "build an OCI image from a Dockerfile",
        )?;
        self.pull(session_name, &localhost_image_reference(image_name))
    }

    #[cfg(test)]
    pub fn prepare_image_launch(
        &self,
        session_name: &str,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMountedImageLaunch> {
        let mount_session = self.pull(session_name, image_reference)?;
        self.prepare_launch_for_mount_session(mount_session, overrides)
    }

    #[cfg(test)]
    pub fn prepare_built_image_launch(
        &self,
        image_name: &str,
        session_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMountedImageLaunch> {
        let mount_session = self.build(image_name, session_name, dockerfile_path, context_path)?;
        self.prepare_launch_for_mount_session(mount_session, overrides)
    }

    #[cfg(test)]
    pub fn mount_rootfs_session(&self, session_name: &str) -> Result<PathBuf> {
        let stdout = self.run_capture_stdout(
            &self.mount_command(session_name),
            self.use_unshare,
            "mount a working container rootfs",
        )?;
        let mount_path = stdout
            .lines()
            .find_map(|line| {
                let trimmed = line.trim();
                (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
            })
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("buildah mount for session {session_name} produced no mount path"),
            })?;
        Ok(mount_path)
    }

    #[cfg(test)]
    pub fn inspect_rootfs_session(&self, session_name: &str) -> Result<OciImageConfig> {
        let stdout = self.run_capture_stdout(
            &self.inspect_command(session_name),
            false,
            "inspect a working container image config",
        )?;
        parse_inspect_output(stdout.as_bytes())
    }

    pub fn unmount_rootfs_session(&self, session_name: &str) -> Result<()> {
        self.run_checked(
            &self.unmount_command(session_name),
            self.use_unshare,
            "unmount a working container rootfs",
        )
    }

    pub fn remove_rootfs_session(&self, session_name: &str) -> Result<()> {
        self.run_checked(
            &self.remove_command(session_name),
            self.use_unshare,
            "remove a working container",
        )
    }

    pub fn cleanup_rootfs_session(&self, session_name: &str) -> Result<()> {
        self.unmount_rootfs_session(session_name)?;
        self.remove_rootfs_session(session_name)
    }

    #[cfg(test)]
    fn prepare_launch_for_mount_session(
        &self,
        mount_session: MountedRootfsSession,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMountedImageLaunch> {
        let rootfs = self.mount_rootfs_session(&mount_session.session_name)?;
        let image_config = self.inspect_rootfs_session(&mount_session.session_name)?;

        // Resolve any named user (e.g., "www-data") to numeric uid:gid while
        // the rootfs overlay mount is still accessible.  In rootless mode, the
        // mount only exists inside a `buildah unshare` session, so we read
        // /etc/passwd and /etc/group from inside that session here, before the
        // mount disappears.
        let resolved_user = self.resolve_image_user(
            &mount_session.session_name,
            overrides.user.as_deref().or(image_config.user.as_deref()),
            &rootfs,
        )?;

        let mut config_with_resolved_user = image_config;
        config_with_resolved_user.user = resolved_user;
        let mut process_overrides = overrides.clone();
        process_overrides.user = None;

        let launch_defaults =
            config_with_resolved_user.resolve_launch_defaults(rootfs, &process_overrides)?;
        Ok(PreparedMountedImageLaunch {
            mount_session,
            launch_defaults,
        })
    }

    fn run_checked(&self, command: &CommandSpec, needs_unshare: bool, action: &str) -> Result<()> {
        let output = self.run_output(command, needs_unshare, action)?;
        if output.status.success() {
            return Ok(());
        }

        Err(SandboxError::OperationFailed {
            message: format!(
                "failed to {action} via {}: {}",
                display_command(command, needs_unshare, self),
                render_command_failure(&output.stdout, &output.stderr)
            ),
        })
    }

    #[cfg(test)]
    fn run_capture_stdout(
        &self,
        command: &CommandSpec,
        needs_unshare: bool,
        action: &str,
    ) -> Result<String> {
        let output = self.run_output(command, needs_unshare, action)?;
        if !output.status.success() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to {action} via {}: {}",
                    display_command(command, needs_unshare, self),
                    render_command_failure(&output.stdout, &output.stderr)
                ),
            });
        }

        String::from_utf8(output.stdout).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to decode stdout from {}: {error}",
                display_command(command, needs_unshare, self)
            ),
        })
    }

    fn run_output(
        &self,
        command: &CommandSpec,
        needs_unshare: bool,
        action: &str,
    ) -> Result<std::process::Output> {
        let command = if needs_unshare {
            self.wrap_unshare(command)
        } else {
            command.clone()
        };
        command
            .as_command()
            .output()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to {action} via {}: {error}",
                    display_command(&command, false, self)
                ),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountedRootfsSession {
    pub session_name: String,
    pub image_reference: String,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedMountedImageLaunch {
    pub mount_session: MountedRootfsSession,
    pub launch_defaults: OciImageLaunchDefaults,
}
