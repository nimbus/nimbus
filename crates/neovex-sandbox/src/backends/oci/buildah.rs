use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::command::CommandSpec;
use crate::error::{Result, SandboxError};
use crate::spec::{SandboxFilesystemSpec, SandboxImageProcessOverrides, SandboxProcessSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildahCli {
    path: PathBuf,
    use_unshare: bool,
}

impl BuildahCli {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            use_unshare: false,
        }
    }

    pub fn with_unshare(mut self, use_unshare: bool) -> Self {
        self.use_unshare = use_unshare;
        self
    }

    pub fn wrap_unshare(&self, command: &CommandSpec) -> CommandSpec {
        let program = command.program.to_string_lossy().into_owned();
        CommandSpec::new(self.path.clone())
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
            shell_escape(&self.path.to_string_lossy()),
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
        CommandSpec::new(self.path.clone())
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

    pub fn create_from_image_command(
        &self,
        container_name: &str,
        image_reference: &str,
    ) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("from")
            .arg("--name")
            .arg(container_name.to_owned())
            .arg(image_reference.to_owned())
    }

    pub fn mount_command(&self, container_name: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("mount")
            .arg(container_name.to_owned())
    }

    pub fn inspect_command(&self, container_name: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("inspect")
            .arg("--type")
            .arg("container")
            .arg(container_name.to_owned())
    }

    pub fn unmount_command(&self, container_name: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("umount")
            .arg(container_name.to_owned())
    }

    pub fn remove_command(&self, container_name: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("rm")
            .arg(container_name.to_owned())
    }

    pub fn build_command(
        &self,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("bud")
            .arg("-t")
            .arg(image_name.to_owned())
            .arg("-f")
            .arg(dockerfile_path.to_string_lossy().into_owned())
            .arg(context_path.to_string_lossy().into_owned())
    }

    pub fn pull(&self, container_name: &str, image_reference: &str) -> Result<BuildahContainer> {
        self.run_checked(
            &self.create_from_image_command(container_name, image_reference),
            false,
            "create a working container from an image",
        )?;
        Ok(BuildahContainer {
            container_name: container_name.to_owned(),
            image_reference: image_reference.to_owned(),
        })
    }

    pub fn build(
        &self,
        image_name: &str,
        container_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> Result<BuildahContainer> {
        self.run_checked(
            &self.build_command(image_name, dockerfile_path, context_path),
            false,
            "build an OCI image from a Dockerfile",
        )?;
        self.pull(container_name, &localhost_image_reference(image_name))
    }

    pub fn prepare_image_launch(
        &self,
        container_name: &str,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedImageLaunch> {
        let container = self.pull(container_name, image_reference)?;
        self.prepare_launch_for_container(container, overrides)
    }

    pub fn prepare_built_image_launch(
        &self,
        image_name: &str,
        container_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedImageLaunch> {
        let container = self.build(image_name, container_name, dockerfile_path, context_path)?;
        self.prepare_launch_for_container(container, overrides)
    }

    pub fn mount_container(&self, container_name: &str) -> Result<PathBuf> {
        let stdout = self.run_capture_stdout(
            &self.mount_command(container_name),
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
                message: format!(
                    "buildah mount for container {container_name} produced no mount path"
                ),
            })?;
        Ok(mount_path)
    }

    pub fn inspect_container(&self, container_name: &str) -> Result<OciImageConfig> {
        let stdout = self.run_capture_stdout(
            &self.inspect_command(container_name),
            false,
            "inspect a working container image config",
        )?;
        parse_inspect_output(stdout.as_bytes())
    }

    pub fn unmount_container(&self, container_name: &str) -> Result<()> {
        self.run_checked(
            &self.unmount_command(container_name),
            self.use_unshare,
            "unmount a working container rootfs",
        )
    }

    pub fn remove_container(&self, container_name: &str) -> Result<()> {
        self.run_checked(
            &self.remove_command(container_name),
            self.use_unshare,
            "remove a working container",
        )
    }

    pub fn cleanup_container(&self, container_name: &str) -> Result<()> {
        self.unmount_container(container_name)?;
        self.remove_container(container_name)
    }

    fn prepare_launch_for_container(
        &self,
        container: BuildahContainer,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedImageLaunch> {
        let rootfs = self.mount_container(&container.container_name)?;
        let image_config = self.inspect_container(&container.container_name)?;

        // Resolve any named user (e.g., "www-data") to numeric uid:gid while
        // the rootfs overlay mount is still accessible.  In rootless mode, the
        // mount only exists inside a `buildah unshare` session, so we read
        // /etc/passwd and /etc/group from inside that session here, before the
        // mount disappears.
        let resolved_user = self.resolve_image_user(
            &container.container_name,
            overrides.user.as_deref().or(image_config.user.as_deref()),
            &rootfs,
        )?;

        let mut config_with_resolved_user = image_config;
        config_with_resolved_user.user = resolved_user;
        let mut process_overrides = overrides.clone();
        process_overrides.user = None;

        let launch_defaults =
            config_with_resolved_user.resolve_launch_defaults(rootfs, &process_overrides)?;
        Ok(PreparedImageLaunch {
            container,
            launch_defaults,
        })
    }

    /// Resolve an image USER string to numeric "uid:gid" by reading
    /// /etc/passwd and /etc/group from inside the container's rootfs.
    /// Runs inside `buildah unshare` so the overlay mount is accessible.
    fn resolve_image_user(
        &self,
        container_name: &str,
        user: Option<&str>,
        rootfs: &Path,
    ) -> Result<Option<String>> {
        let Some(user) = user.map(str::trim).filter(|u| !u.is_empty()) else {
            return Ok(None);
        };

        // If the user is already fully numeric (uid or uid:gid), pass through.
        if is_numeric_user_spec(user) {
            return Ok(Some(user.to_owned()));
        }

        // Read /etc/passwd from inside the rootfs via buildah unshare to
        // resolve named users to numeric uid:gid.
        let passwd_content = self.read_rootfs_file(container_name, rootfs, "etc/passwd")?;
        let group_content = self.read_rootfs_file_optional(container_name, rootfs, "etc/group");

        resolve_user_from_content(user, &passwd_content, group_content.as_deref())
    }

    fn read_rootfs_file(
        &self,
        container_name: &str,
        rootfs: &Path,
        relative_path: &str,
    ) -> Result<String> {
        let file_path = rootfs.join(relative_path);
        let cat_command = CommandSpec::new("cat").arg(file_path.to_string_lossy().into_owned());
        let buildah = BuildahCli::new(self.path.clone()).with_unshare(self.use_unshare);
        let wrapped = buildah.wrap_unshare_with_mount(container_name, &cat_command);
        let output =
            wrapped
                .as_command()
                .output()
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read {relative_path} from container {container_name}: {error}"
                    ),
                })?;
        if !output.status.success() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to read {relative_path} from container {container_name}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn read_rootfs_file_optional(
        &self,
        container_name: &str,
        rootfs: &Path,
        relative_path: &str,
    ) -> Option<String> {
        self.read_rootfs_file(container_name, rootfs, relative_path)
            .ok()
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
pub struct BuildahContainer {
    pub container_name: String,
    pub image_reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedImageLaunch {
    pub container: BuildahContainer,
    pub launch_defaults: OciImageLaunchDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciImageConfig {
    #[serde(default)]
    pub entrypoint: Vec<String>,
    #[serde(default)]
    pub cmd: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    #[serde(default)]
    pub exposed_ports: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<String>,
    pub stop_signal: Option<String>,
    pub healthcheck: Option<ImageHealthcheck>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciImageLaunchDefaults {
    pub filesystem: SandboxFilesystemSpec,
    pub process: SandboxProcessSpec,
    pub exposed_ports: Vec<OciExposedPort>,
    pub user: Option<String>,
    pub stop_signal: Option<String>,
    pub healthcheck: Option<ImageHealthcheck>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageHealthcheck {
    #[serde(default, alias = "Test", alias = "test")]
    pub test: Vec<String>,
    #[serde(default, alias = "Interval", alias = "interval")]
    pub interval: Option<u64>,
    #[serde(default, alias = "Timeout", alias = "timeout")]
    pub timeout: Option<u64>,
    #[serde(default, alias = "StartPeriod", alias = "start_period")]
    pub start_period: Option<u64>,
    #[serde(default, alias = "Retries", alias = "retries")]
    pub retries: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct BuildahInspectPayload {
    #[serde(default, rename = "OCIv1")]
    oci_v1: Option<BuildahImageEnvelope>,
    #[serde(default, rename = "Docker")]
    docker: Option<BuildahImageEnvelope>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct BuildahImageEnvelope {
    #[serde(default, alias = "Config", alias = "config")]
    config: BuildahImageFields,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct BuildahImageFields {
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Entrypoint",
        alias = "entrypoint"
    )]
    entrypoint: Vec<String>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Cmd",
        alias = "cmd"
    )]
    cmd: Vec<String>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Env",
        alias = "env"
    )]
    env: Vec<String>,
    #[serde(default, alias = "WorkingDir", alias = "working_dir")]
    working_dir: Option<String>,
    #[serde(default, alias = "User", alias = "user")]
    user: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "ExposedPorts",
        alias = "exposed_ports"
    )]
    exposed_ports: BTreeMap<String, Value>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Volumes",
        alias = "volumes"
    )]
    volumes: BTreeMap<String, Value>,
    #[serde(default, alias = "StopSignal", alias = "stop_signal")]
    stop_signal: Option<String>,
    #[serde(default, alias = "Healthcheck", alias = "healthcheck")]
    healthcheck: Option<ImageHealthcheck>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Labels",
        alias = "labels"
    )]
    labels: BTreeMap<String, String>,
}

/// Deserialize a field that may be `null` in JSON as the type's `Default` value.
/// This handles the common OCI case where buildah/Docker write `"Entrypoint": null`
/// instead of omitting the field entirely.
fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(|opt| opt.unwrap_or_default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OciExposedPortProtocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciExposedPort {
    pub port: u16,
    pub protocol: OciExposedPortProtocol,
    pub raw: String,
}

fn is_numeric_user_spec(user: &str) -> bool {
    let parts: Vec<&str> = user.split(':').collect();
    match parts.len() {
        1 => parts[0].parse::<u32>().is_ok(),
        2 => parts[0].parse::<u32>().is_ok() && parts[1].parse::<u32>().is_ok(),
        _ => false,
    }
}

fn resolve_user_from_content(
    user: &str,
    passwd_content: &str,
    group_content: Option<&str>,
) -> Result<Option<String>> {
    let (user_part, group_part) = match user.split_once(':') {
        Some((u, g)) => (u.trim(), Some(g.trim())),
        None => (user.trim(), None),
    };

    // Resolve user part
    let (uid, default_gid) = if let Ok(uid) = user_part.parse::<u32>() {
        let gid = find_passwd_gid_by_uid(passwd_content, uid);
        (uid, gid.unwrap_or(0))
    } else {
        let entry = find_passwd_entry_by_name(passwd_content, user_part).ok_or_else(|| {
            SandboxError::InvalidSpec {
                message: format!(
                    "image user {user:?} references user {user_part:?} not found in /etc/passwd"
                ),
            }
        })?;
        (entry.0, entry.1)
    };

    // Resolve group part
    let gid = match group_part {
        Some(g) if !g.is_empty() => {
            if let Ok(gid) = g.parse::<u32>() {
                gid
            } else {
                find_group_gid_by_name(group_content.unwrap_or(""), g).ok_or_else(|| {
                    SandboxError::InvalidSpec {
                        message: format!(
                            "image user {user:?} references group {g:?} not found in /etc/group"
                        ),
                    }
                })?
            }
        }
        _ => default_gid,
    };

    Ok(Some(format!("{uid}:{gid}")))
}

fn find_passwd_entry_by_name(passwd_content: &str, name: &str) -> Option<(u32, u32)> {
    for line in passwd_content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4
            && fields[0] == name
            && let (Ok(uid), Ok(gid)) = (fields[2].parse::<u32>(), fields[3].parse::<u32>())
        {
            return Some((uid, gid));
        }
    }
    None
}

fn find_passwd_gid_by_uid(passwd_content: &str, uid: u32) -> Option<u32> {
    for line in passwd_content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4
            && let Ok(entry_uid) = fields[2].parse::<u32>()
            && entry_uid == uid
        {
            return fields[3].parse::<u32>().ok();
        }
    }
    None
}

fn find_group_gid_by_name(group_content: &str, name: &str) -> Option<u32> {
    for line in group_content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == name {
            return fields[2].parse::<u32>().ok();
        }
    }
    None
}

fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_owned();
    }
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'/' || b == b'.')
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn localhost_image_reference(image_name: &str) -> String {
    format!("localhost/{image_name}")
}

fn parse_inspect_output(stdout: &[u8]) -> Result<OciImageConfig> {
    let value: Value =
        serde_json::from_slice(stdout).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to parse buildah inspect JSON: {error}"),
        })?;

    let payload = match value {
        Value::Array(entries) => {
            let first =
                entries
                    .into_iter()
                    .next()
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: "buildah inspect returned an empty JSON array".to_owned(),
                    })?;
            serde_json::from_value::<BuildahInspectPayload>(first).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to decode buildah inspect payload: {error}"),
                }
            })?
        }
        Value::Object(_) => {
            serde_json::from_value::<BuildahInspectPayload>(value).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to decode buildah inspect payload: {error}"),
                }
            })?
        }
        _ => {
            return Err(SandboxError::OperationFailed {
                message: "buildah inspect JSON was neither an object nor an array".to_owned(),
            });
        }
    };

    Ok(OciImageConfig::from_payload(payload))
}

impl OciImageConfig {
    fn from_payload(payload: BuildahInspectPayload) -> Self {
        let oci = payload
            .oci_v1
            .map(|payload| payload.config)
            .unwrap_or_default();
        let docker = payload
            .docker
            .map(|payload| payload.config)
            .unwrap_or_default();

        let mut labels = docker.labels;
        labels.extend(oci.labels);

        Self {
            entrypoint: prefer_vec(oci.entrypoint, docker.entrypoint),
            cmd: prefer_vec(oci.cmd, docker.cmd),
            env: prefer_vec(oci.env, docker.env),
            working_dir: oci.working_dir.or(docker.working_dir),
            user: oci.user.or(docker.user),
            exposed_ports: merge_map_keys(oci.exposed_ports, docker.exposed_ports),
            volumes: merge_map_keys(oci.volumes, docker.volumes),
            stop_signal: oci.stop_signal.or(docker.stop_signal),
            healthcheck: oci.healthcheck.or(docker.healthcheck),
            labels,
        }
    }

    pub fn resolve_process_spec(
        &self,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<SandboxProcessSpec> {
        let entrypoint = overrides
            .entrypoint
            .clone()
            .unwrap_or_else(|| self.entrypoint.clone());
        let cmd = overrides.cmd.clone().unwrap_or_else(|| self.cmd.clone());
        let args = if entrypoint.is_empty() {
            cmd
        } else {
            entrypoint.into_iter().chain(cmd).collect()
        };

        if args.is_empty() {
            return Err(SandboxError::InvalidSpec {
                message:
                    "image config did not provide a launch command and no overrides were supplied"
                        .to_owned(),
            });
        }

        let env = merge_env_pairs(&self.env, &overrides.env);
        let cwd = overrides
            .cwd
            .clone()
            .or_else(|| self.working_dir.as_ref().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));

        Ok(SandboxProcessSpec::new(args)
            .with_env(env)
            .with_cwd(cwd)
            .with_terminal(overrides.terminal))
    }

    pub fn resolve_launch_defaults(
        &self,
        rootfs: impl Into<PathBuf>,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<OciImageLaunchDefaults> {
        Ok(OciImageLaunchDefaults {
            filesystem: SandboxFilesystemSpec::new(rootfs),
            process: self.resolve_process_spec(overrides)?,
            exposed_ports: self.exposed_port_bindings()?,
            user: self.user.clone().filter(|user| !user.trim().is_empty()),
            stop_signal: self.stop_signal.clone(),
            healthcheck: self.healthcheck.clone(),
            labels: self.labels.clone(),
        })
    }

    pub fn exposed_port_bindings(&self) -> Result<Vec<OciExposedPort>> {
        let mut ports = self
            .exposed_ports
            .iter()
            .map(|raw| parse_exposed_port(raw))
            .collect::<Result<Vec<_>>>()?;
        ports.sort_by_key(|port| {
            (
                port.port,
                exposed_port_protocol_rank(port.protocol),
                port.raw.clone(),
            )
        });
        Ok(ports)
    }
}

fn prefer_vec(primary: Vec<String>, fallback: Vec<String>) -> Vec<String> {
    if primary.is_empty() {
        fallback
    } else {
        primary
    }
}

fn merge_map_keys(
    primary: BTreeMap<String, Value>,
    fallback: BTreeMap<String, Value>,
) -> Vec<String> {
    let mut keys = fallback;
    keys.extend(primary);
    keys.into_keys().collect()
}

fn merge_env_pairs(base: &[String], overrides: &[String]) -> Vec<String> {
    let mut merged = base.to_vec();
    for override_entry in overrides {
        let Some(override_key) = env_key(override_entry) else {
            merged.push(override_entry.clone());
            continue;
        };

        if let Some(index) = merged
            .iter()
            .position(|entry| env_key(entry).is_some_and(|key| key == override_key))
        {
            merged[index] = override_entry.clone();
        } else {
            merged.push(override_entry.clone());
        }
    }
    merged
}

fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
}

fn parse_exposed_port(raw: &str) -> Result<OciExposedPort> {
    let (port, protocol) = raw
        .split_once('/')
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!("image config exposed port {raw:?} must be in PORT/PROTOCOL form"),
        })?;
    let port = port
        .parse::<u16>()
        .map_err(|error| SandboxError::InvalidSpec {
            message: format!("image config exposed port {raw:?} has invalid port: {error}"),
        })?;
    let protocol = match protocol {
        "tcp" => OciExposedPortProtocol::Tcp,
        "udp" => OciExposedPortProtocol::Udp,
        _ => {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "image config exposed port {raw:?} uses unsupported protocol {protocol:?}"
                ),
            });
        }
    };

    Ok(OciExposedPort {
        port,
        protocol,
        raw: raw.to_owned(),
    })
}

fn exposed_port_protocol_rank(protocol: OciExposedPortProtocol) -> u8 {
    match protocol {
        OciExposedPortProtocol::Tcp => 0,
        OciExposedPortProtocol::Udp => 1,
    }
}

fn display_command(command: &CommandSpec, needs_unshare: bool, buildah: &BuildahCli) -> String {
    let rendered = if needs_unshare {
        buildah.wrap_unshare(command)
    } else {
        command.clone()
    };
    let mut parts = vec![rendered.program.display().to_string()];
    parts.extend(rendered.args);
    parts.join(" ")
}

fn render_command_failure(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    if stdout.is_empty() {
        "stdout and stderr were empty".to_owned()
    } else {
        stdout
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use serde_json::json;
    use tempfile::{NamedTempFile, TempDir};

    use super::{
        BuildahCli, OciExposedPort, OciExposedPortProtocol, OciImageConfig, OciImageLaunchDefaults,
        parse_inspect_output,
    };
    use crate::backends::oci::command::CommandSpec;
    use crate::spec::SandboxImageProcessOverrides;

    #[test]
    fn wrap_unshare_prefixes_existing_command() {
        let buildah = BuildahCli::new("buildah");
        let wrapped = buildah.wrap_unshare(
            &CommandSpec::new("/usr/libexec/neovex/crun")
                .arg("state")
                .arg("sandbox-123"),
        );

        assert_eq!(wrapped.program, PathBuf::from("buildah"));
        assert_eq!(
            wrapped.args,
            vec![
                "unshare",
                "--",
                "/usr/libexec/neovex/crun",
                "state",
                "sandbox-123",
            ]
        );
    }

    #[test]
    fn maybe_wrap_is_identity_when_unshare_is_disabled() {
        let buildah = BuildahCli::new("buildah");
        let command = CommandSpec::new("/usr/bin/crun")
            .arg("state")
            .arg("sandbox-123");

        assert_eq!(buildah.maybe_wrap(command.clone()), command);
    }

    #[test]
    fn inspect_command_requests_container_json_without_template_mode() {
        let buildah = BuildahCli::new("/usr/bin/buildah");
        let command = buildah.inspect_command("postgres-working");

        assert_eq!(command.program, PathBuf::from("/usr/bin/buildah"));
        assert_eq!(
            command.args,
            vec!["inspect", "--type", "container", "postgres-working"]
        );
    }

    #[test]
    fn build_command_matches_expected_shape() {
        let buildah = BuildahCli::new("/usr/bin/buildah");
        let command = buildah.build_command(
            "neovex-test",
            Path::new("/workspace/Dockerfile"),
            Path::new("/workspace"),
        );

        assert_eq!(command.program, PathBuf::from("/usr/bin/buildah"));
        assert_eq!(
            command.args,
            vec![
                "bud",
                "-t",
                "neovex-test",
                "-f",
                "/workspace/Dockerfile",
                "/workspace",
            ]
        );
    }

    #[test]
    fn inspect_payload_merges_oci_and_docker_image_config_fields() {
        let config = parse_inspect_output(sample_inspect_json().as_bytes())
            .expect("sample inspect output should parse");

        assert_eq!(
            config,
            OciImageConfig {
                entrypoint: vec!["/usr/local/bin/docker-entrypoint.sh".to_owned()],
                cmd: vec!["postgres".to_owned()],
                env: vec![
                    "PATH=/usr/local/bin:/usr/bin".to_owned(),
                    "POSTGRES_DB=postgres".to_owned(),
                ],
                working_dir: Some("/var/lib/postgresql".to_owned()),
                user: Some("999:999".to_owned()),
                exposed_ports: vec!["5432/tcp".to_owned()],
                volumes: vec!["/var/lib/postgresql/data".to_owned()],
                stop_signal: Some("SIGINT".to_owned()),
                healthcheck: Some(super::ImageHealthcheck {
                    test: vec!["CMD-SHELL".to_owned(), "pg_isready -U postgres".to_owned()],
                    interval: Some(10_000_000_000),
                    timeout: Some(5_000_000_000),
                    start_period: Some(30_000_000_000),
                    retries: Some(3),
                }),
                labels: std::iter::once(("com.example.role".to_owned(), "primary".to_owned(),))
                    .collect(),
            }
        );
    }

    #[test]
    fn resolve_process_spec_uses_image_defaults() {
        let config = parse_inspect_output(sample_inspect_json().as_bytes())
            .expect("sample inspect output should parse");

        let process = config
            .resolve_process_spec(&SandboxImageProcessOverrides::default())
            .expect("image defaults should lower into a process spec");

        assert_eq!(
            process.args,
            vec![
                "/usr/local/bin/docker-entrypoint.sh".to_owned(),
                "postgres".to_owned(),
            ]
        );
        assert_eq!(
            process.env,
            vec![
                "PATH=/usr/local/bin:/usr/bin".to_owned(),
                "POSTGRES_DB=postgres".to_owned(),
            ]
        );
        assert_eq!(process.cwd, PathBuf::from("/var/lib/postgresql"));
        assert!(!process.terminal);
    }

    #[test]
    fn resolve_process_spec_applies_overrides_with_env_precedence() {
        let config = parse_inspect_output(sample_inspect_json().as_bytes())
            .expect("sample inspect output should parse");

        let process = config
            .resolve_process_spec(&SandboxImageProcessOverrides {
                entrypoint: Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()]),
                cmd: Some(vec!["exec custom-api".to_owned()]),
                env: vec!["POSTGRES_DB=app".to_owned(), "LOG_LEVEL=debug".to_owned()],
                cwd: Some(PathBuf::from("/workspace")),
                user: None,
                terminal: true,
            })
            .expect("overrides should lower into a process spec");

        assert_eq!(
            process.args,
            vec![
                "/bin/sh".to_owned(),
                "-lc".to_owned(),
                "exec custom-api".to_owned(),
            ]
        );
        assert_eq!(
            process.env,
            vec![
                "PATH=/usr/local/bin:/usr/bin".to_owned(),
                "POSTGRES_DB=app".to_owned(),
                "LOG_LEVEL=debug".to_owned(),
            ]
        );
        assert_eq!(process.cwd, PathBuf::from("/workspace"));
        assert!(process.terminal);
    }

    #[test]
    fn resolve_process_spec_rejects_missing_launch_command() {
        let config = OciImageConfig {
            entrypoint: Vec::new(),
            cmd: Vec::new(),
            env: Vec::new(),
            working_dir: None,
            user: None,
            exposed_ports: Vec::new(),
            volumes: Vec::new(),
            stop_signal: None,
            healthcheck: None,
            labels: Default::default(),
        };

        let error = config
            .resolve_process_spec(&SandboxImageProcessOverrides::default())
            .expect_err("missing command should be rejected");
        assert!(
            error
                .to_string()
                .contains("did not provide a launch command"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn exposed_port_bindings_parse_and_sort_image_ports() {
        let config = parse_inspect_output(sample_inspect_with_ports_json().as_bytes())
            .expect("sample inspect output should parse");

        let ports = config
            .exposed_port_bindings()
            .expect("exposed ports should parse");

        assert_eq!(
            ports,
            vec![
                OciExposedPort {
                    port: 53,
                    protocol: OciExposedPortProtocol::Udp,
                    raw: "53/udp".to_owned(),
                },
                OciExposedPort {
                    port: 5432,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "5432/tcp".to_owned(),
                },
                OciExposedPort {
                    port: 8080,
                    protocol: OciExposedPortProtocol::Tcp,
                    raw: "8080/tcp".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn exposed_port_bindings_reject_invalid_port_shape() {
        let config = OciImageConfig {
            entrypoint: vec!["/bin/service".to_owned()],
            cmd: Vec::new(),
            env: Vec::new(),
            working_dir: None,
            user: None,
            exposed_ports: vec!["not-a-port".to_owned()],
            volumes: Vec::new(),
            stop_signal: None,
            healthcheck: None,
            labels: Default::default(),
        };

        let error = config
            .exposed_port_bindings()
            .expect_err("invalid port shape should be rejected");
        assert!(
            error.to_string().contains("PORT/PROTOCOL"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn resolve_launch_defaults_collects_rootfs_process_ports_and_metadata() {
        let config = parse_inspect_output(sample_inspect_with_ports_json().as_bytes())
            .expect("sample inspect output should parse");

        let defaults = config
            .resolve_launch_defaults(
                "/srv/rootfs",
                &SandboxImageProcessOverrides {
                    cmd: Some(vec!["serve".to_owned()]),
                    env: vec!["SERVICE_MODE=foreground".to_owned()],
                    ..SandboxImageProcessOverrides::default()
                },
            )
            .expect("launch defaults should resolve");

        assert_eq!(
            defaults,
            OciImageLaunchDefaults {
                filesystem: crate::spec::SandboxFilesystemSpec::new("/srv/rootfs"),
                process: crate::spec::SandboxProcessSpec::new(["/usr/local/bin/service", "serve",])
                    .with_env(["SERVICE_MODE=foreground"])
                    .with_cwd("/"),
                exposed_ports: vec![
                    OciExposedPort {
                        port: 53,
                        protocol: OciExposedPortProtocol::Udp,
                        raw: "53/udp".to_owned(),
                    },
                    OciExposedPort {
                        port: 5432,
                        protocol: OciExposedPortProtocol::Tcp,
                        raw: "5432/tcp".to_owned(),
                    },
                    OciExposedPort {
                        port: 8080,
                        protocol: OciExposedPortProtocol::Tcp,
                        raw: "8080/tcp".to_owned(),
                    },
                ],
                user: Some("1000:1000".to_owned()),
                stop_signal: Some("SIGTERM".to_owned()),
                healthcheck: Some(super::ImageHealthcheck {
                    test: vec![
                        "CMD-SHELL".to_owned(),
                        "curl -f http://localhost/health".to_owned()
                    ],
                    interval: Some(15_000_000_000),
                    timeout: Some(3_000_000_000),
                    start_period: Some(20_000_000_000),
                    retries: Some(5),
                }),
                labels: std::iter::once(("com.example.service".to_owned(), "edge".to_owned(),))
                    .collect(),
            }
        );
    }

    #[test]
    fn pull_mount_inspect_and_cleanup_execute_expected_commands() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
        let buildah = BuildahCli::new(script_path).with_unshare(true);

        let pulled = buildah
            .pull("postgres-working", "postgres:16")
            .expect("pull should succeed");
        assert_eq!(pulled.container_name, "postgres-working");
        assert_eq!(pulled.image_reference, "postgres:16");

        let mount_path = buildah
            .mount_container("postgres-working")
            .expect("mount should succeed");
        assert_eq!(mount_path, PathBuf::from("/tmp/fake-rootfs"));

        let inspected = buildah
            .inspect_container("postgres-working")
            .expect("inspect should succeed");
        assert_eq!(inspected.cmd, vec!["postgres"]);

        buildah
            .cleanup_container("postgres-working")
            .expect("cleanup should succeed");

        let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines[0], "from --name postgres-working postgres:16");
        assert!(
            lines[1].starts_with("unshare -- "),
            "mount should run inside buildah unshare when enabled"
        );
        assert!(lines[1].ends_with(" mount postgres-working"));
        assert_eq!(lines[2], "inspect --type container postgres-working");
        assert!(
            lines[3].starts_with("unshare -- "),
            "umount should run inside buildah unshare when enabled"
        );
        assert!(lines[3].ends_with(" umount postgres-working"));
        assert!(
            lines[4].starts_with("unshare -- "),
            "rm should run inside buildah unshare when enabled"
        );
        assert!(lines[4].ends_with(" rm postgres-working"));
    }

    #[test]
    fn prepare_image_launch_combines_buildah_materialization_and_launch_defaults() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
        let buildah = BuildahCli::new(script_path).with_unshare(true);

        let prepared = buildah
            .prepare_image_launch(
                "postgres-working",
                "postgres:16",
                &SandboxImageProcessOverrides {
                    env: vec!["PGPORT=5432".to_owned()],
                    cwd: Some(PathBuf::from("/workspace")),
                    ..SandboxImageProcessOverrides::default()
                },
            )
            .expect("prepared launch should succeed");

        assert_eq!(prepared.container.container_name, "postgres-working");
        assert_eq!(prepared.container.image_reference, "postgres:16");
        assert_eq!(
            prepared.launch_defaults.filesystem.rootfs,
            PathBuf::from("/tmp/fake-rootfs")
        );
        assert_eq!(
            prepared.launch_defaults.process.args,
            vec![
                "/usr/local/bin/docker-entrypoint.sh".to_owned(),
                "postgres".to_owned()
            ]
        );
        assert_eq!(
            prepared.launch_defaults.process.env,
            vec![
                "PATH=/usr/local/bin:/usr/bin".to_owned(),
                "POSTGRES_DB=postgres".to_owned(),
                "PGPORT=5432".to_owned(),
            ]
        );
        assert_eq!(
            prepared.launch_defaults.process.cwd,
            PathBuf::from("/workspace")
        );
        assert_eq!(
            prepared.launch_defaults.exposed_ports,
            vec![OciExposedPort {
                port: 5432,
                protocol: OciExposedPortProtocol::Tcp,
                raw: "5432/tcp".to_owned(),
            }]
        );

        let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines[0], "from --name postgres-working postgres:16");
        assert!(lines[1].ends_with(" mount postgres-working"));
        assert_eq!(lines[2], "inspect --type container postgres-working");
    }

    #[test]
    fn prepare_image_launch_prefers_process_user_override_over_image_user() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let (script_path, _log_path) = write_fake_buildah_script(&temp_dir);
        let buildah = BuildahCli::new(script_path).with_unshare(true);

        let prepared = buildah
            .prepare_image_launch(
                "postgres-working",
                "postgres:16",
                &SandboxImageProcessOverrides::default().with_user("123:456"),
            )
            .expect("prepared launch should succeed");

        assert_eq!(
            prepared.launch_defaults.user,
            Some("123:456".to_owned()),
            "explicit process user override should win over the image USER"
        );
    }

    #[test]
    fn build_materializes_localhost_image_before_creating_working_container() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
        let buildah = BuildahCli::new(script_path);

        let built = buildah
            .build(
                "neovex-api",
                "api-working",
                Path::new("/workspace/Dockerfile"),
                Path::new("/workspace"),
            )
            .expect("build should succeed");
        assert_eq!(built.image_reference, "localhost/neovex-api");

        let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(
            lines[0],
            "bud -t neovex-api -f /workspace/Dockerfile /workspace"
        );
        assert_eq!(lines[1], "from --name api-working localhost/neovex-api");
    }

    #[test]
    fn prepare_built_image_launch_uses_built_image_reference() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let (script_path, log_path) = write_fake_buildah_script(&temp_dir);
        let buildah = BuildahCli::new(script_path);

        let prepared = buildah
            .prepare_built_image_launch(
                "neovex-api",
                "api-working",
                Path::new("/workspace/Dockerfile"),
                Path::new("/workspace"),
                &SandboxImageProcessOverrides::default(),
            )
            .expect("prepared built launch should succeed");

        assert_eq!(prepared.container.image_reference, "localhost/neovex-api");
        assert_eq!(
            prepared.launch_defaults.filesystem.rootfs,
            PathBuf::from("/tmp/fake-rootfs")
        );

        let log = fs::read_to_string(log_path).expect("fake buildah log should be readable");
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(
            lines,
            vec![
                "bud -t neovex-api -f /workspace/Dockerfile /workspace",
                "from --name api-working localhost/neovex-api",
                "mount api-working",
                "inspect --type container api-working",
            ]
        );
    }

    fn write_fake_buildah_script(temp_dir: &TempDir) -> (PathBuf, PathBuf) {
        let script_path = temp_dir.path().join("fake-buildah");
        let log_path = temp_dir.path().join("buildah.log");
        let script = format!(
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "{log_path}"
cmd="${{1:-}}"
if [ -z "$cmd" ]; then
  echo "missing buildah subcommand" >&2
  exit 1
fi
shift

if [ "$cmd" = "unshare" ]; then
  if [ "${{1:-}}" != "--" ]; then
    echo "expected -- after buildah unshare" >&2
    exit 1
  fi
  shift
  wrapped_program="${{1:-}}"
  if [ -z "$wrapped_program" ]; then
    echo "missing wrapped program for buildah unshare" >&2
    exit 1
  fi
  shift
  cmd="${{1:-}}"
  if [ -z "$cmd" ]; then
    printf 'missing subcommand for wrapped program %s\n' "$wrapped_program" >&2
    exit 1
  fi
  shift
fi

case "$cmd" in
  from|bud|umount|rm)
    exit 0
    ;;
  mount)
    printf '%s\n' "/tmp/fake-rootfs"
    exit 0
    ;;
  inspect)
    cat <<'JSON'
{inspect_json}
JSON
    exit 0
    ;;
  *)
    printf 'unexpected command: %s\n' "$cmd" >&2
    exit 1
    ;;
esac
"#,
            log_path = log_path.display(),
            inspect_json = sample_inspect_json()
        );

        let mut temp_script = NamedTempFile::new_in(temp_dir.path())
            .expect("temporary fake buildah file should exist");
        temp_script
            .write_all(script.as_bytes())
            .expect("fake buildah script should be written");
        temp_script
            .flush()
            .expect("fake buildah script should flush cleanly");
        let mut permissions = temp_script
            .as_file()
            .metadata()
            .expect("fake buildah temp script metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        temp_script
            .as_file()
            .set_permissions(permissions)
            .expect("fake buildah temp script should be executable");
        temp_script
            .as_file()
            .sync_all()
            .expect("fake buildah script should sync cleanly");
        temp_script
            .persist(&script_path)
            .expect("fake buildah script should persist cleanly");

        (script_path, log_path)
    }

    fn sample_inspect_json() -> String {
        json!([
            {
                "OCIv1": {
                    "Config": {
                        "Entrypoint": ["/usr/local/bin/docker-entrypoint.sh"],
                        "Cmd": ["postgres"],
                        "Env": [
                            "PATH=/usr/local/bin:/usr/bin",
                            "POSTGRES_DB=postgres"
                        ],
                        "WorkingDir": "/var/lib/postgresql",
                        "User": "999:999",
                        "ExposedPorts": {
                            "5432/tcp": {}
                        },
                        "Volumes": {
                            "/var/lib/postgresql/data": {}
                        },
                        "StopSignal": "SIGINT",
                        "Labels": {
                            "com.example.role": "primary"
                        }
                    }
                },
                "Docker": {
                    "Config": {
                        "Healthcheck": {
                            "Test": ["CMD-SHELL", "pg_isready -U postgres"],
                            "Interval": 10000000000_u64,
                            "Timeout": 5000000000_u64,
                            "StartPeriod": 30000000000_u64,
                            "Retries": 3
                        }
                    }
                }
            }
        ])
        .to_string()
    }

    fn sample_inspect_with_ports_json() -> String {
        json!([
            {
                "OCIv1": {
                    "Config": {
                        "Entrypoint": ["/usr/local/bin/service"],
                        "User": "1000:1000",
                        "ExposedPorts": {
                            "8080/tcp": {},
                            "53/udp": {},
                            "5432/tcp": {}
                        },
                        "StopSignal": "SIGTERM",
                        "Labels": {
                            "com.example.service": "edge"
                        }
                    }
                },
                "Docker": {
                    "Config": {
                        "Healthcheck": {
                            "Test": ["CMD-SHELL", "curl -f http://localhost/health"],
                            "Interval": 15000000000_u64,
                            "Timeout": 3000000000_u64,
                            "StartPeriod": 20000000000_u64,
                            "Retries": 5
                        }
                    }
                }
            }
        ])
        .to_string()
    }
}
