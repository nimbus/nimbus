use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use clap::{Args, Subcommand};
use nimbus::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::cli_ux;

const SERVICE_NAME: &str = "nimbus.service";
const SOCKET_NAME: &str = "nimbus.socket";
const QUADLET_NAME: &str = "nimbus.container";
const DEFAULT_NATIVE_BINARY: &str = "/usr/local/bin/nimbus";
const DEFAULT_NATIVE_DATA_DIR: &str = "/var/lib/nimbus";
const DEFAULT_NATIVE_CONTROL_DIR: &str = "/var/lib/nimbus/control";
const DEFAULT_NATIVE_HOST: &str = "127.0.0.1";
const DEFAULT_NATIVE_PORT: u16 = 8080;
const DEFAULT_QUADLET_VOLUME: &str = "nimbus-data:/var/lib/nimbus";
const DEFAULT_QUADLET_PUBLISH_PORT: &str = "127.0.0.1:8080:8080";
const DEFAULT_HEALTH_CMD: &str = "curl -fsS http://127.0.0.1:8080/health";
const NATIVE_TEMPLATE_VERSION: &str = "native-systemd-node-service/v1";
const NATIVE_SOCKET_TEMPLATE_VERSION: &str = "native-systemd-node-socket/v1";
const QUADLET_TEMPLATE_VERSION: &str = "quadlet-node-service/v1";

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    after_help = cli_ux::NODE_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
pub(crate) struct NodeCommand {
    #[command(subcommand)]
    command: NodeSubcommand,
}

#[derive(Debug, Subcommand)]
enum NodeSubcommand {
    /// Install or render Nimbus node service-manager artifacts.
    Install(NodeInstallCommand),
    /// Show the Nimbus node service status through systemd.
    Status(NodeStatusCommand),
    /// Print Nimbus node service logs through journalctl.
    Logs(NodeLogsCommand),
    /// Diagnose host support for the selected node service mode.
    Doctor(NodeDoctorCommand),
    /// Remove Nimbus node service-manager artifacts.
    Uninstall(NodeUninstallCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::NODE_INSTALL_HELP_EXAMPLES
)]
struct NodeInstallCommand {
    /// Install native systemd units for a host binary.
    #[arg(long, conflicts_with = "container")]
    systemd: bool,

    /// Install a Quadlet .container file for the Nimbus OCI image.
    #[arg(long, conflicts_with = "systemd")]
    container: bool,

    /// Install in the user service-manager location.
    #[arg(long, conflicts_with = "system")]
    user: bool,

    /// Install in the system service-manager location.
    #[arg(long, conflicts_with = "user")]
    system: bool,

    /// Print generated artifacts without writing files or calling systemctl.
    #[arg(long)]
    dry_run: bool,

    /// Enable the generated service after writing artifacts.
    #[arg(long)]
    enable: bool,

    /// Start the generated service after writing artifacts.
    #[arg(long)]
    now: bool,

    /// Replace existing generated artifacts.
    #[arg(long)]
    overwrite: bool,

    /// Trusted Nimbus binary path for native systemd installs.
    #[arg(long, value_name = "PATH")]
    binary: Option<PathBuf>,

    /// Nimbus OCI image reference for containerized Quadlet installs.
    #[arg(long, value_name = "IMAGE")]
    image: Option<String>,

    /// Render a matching nimbus.socket and start from systemd's inherited TCP listener.
    #[arg(long)]
    socket_activation: bool,
}

#[derive(Debug, Args)]
struct NodeStatusCommand {
    /// Inspect the native systemd service.
    #[arg(long, conflicts_with = "container")]
    systemd: bool,

    /// Inspect the Quadlet-backed service.
    #[arg(long, conflicts_with = "systemd")]
    container: bool,

    /// Inspect the user service-manager instance.
    #[arg(long, conflicts_with = "system")]
    user: bool,

    /// Inspect the system service-manager instance.
    #[arg(long, conflicts_with = "user")]
    system: bool,
}

#[derive(Debug, Args)]
struct NodeLogsCommand {
    /// Read logs for the native systemd service.
    #[arg(long, conflicts_with = "container")]
    systemd: bool,

    /// Read logs for the Quadlet-backed service.
    #[arg(long, conflicts_with = "systemd")]
    container: bool,

    /// Read the user service-manager journal.
    #[arg(long, conflicts_with = "system")]
    user: bool,

    /// Read the system service-manager journal.
    #[arg(long, conflicts_with = "user")]
    system: bool,

    /// Follow appended logs.
    #[arg(long)]
    follow: bool,
}

#[derive(Debug, Args)]
struct NodeDoctorCommand {
    /// Diagnose native systemd service support.
    #[arg(long, conflicts_with = "container")]
    systemd: bool,

    /// Diagnose Quadlet service support.
    #[arg(long, conflicts_with = "systemd")]
    container: bool,

    /// Diagnose the user service-manager instance.
    #[arg(long, conflicts_with = "system")]
    user: bool,

    /// Diagnose the system service-manager instance.
    #[arg(long, conflicts_with = "user")]
    system: bool,
}

#[derive(Debug, Args)]
struct NodeUninstallCommand {
    /// Remove native systemd units.
    #[arg(long, conflicts_with = "container")]
    systemd: bool,

    /// Remove Quadlet .container artifact.
    #[arg(long, conflicts_with = "systemd")]
    container: bool,

    /// Remove from the user service-manager location.
    #[arg(long, conflicts_with = "system")]
    user: bool,

    /// Remove from the system service-manager location.
    #[arg(long, conflicts_with = "user")]
    system: bool,

    /// Print the files and commands without mutating the host.
    #[arg(long)]
    dry_run: bool,
}

pub(crate) async fn run_node_command(command: NodeCommand) -> Result<()> {
    match command.command {
        NodeSubcommand::Install(command) => run_install(command),
        NodeSubcommand::Status(command) => run_status(command),
        NodeSubcommand::Logs(command) => run_logs(command),
        NodeSubcommand::Doctor(command) => run_doctor(command),
        NodeSubcommand::Uninstall(command) => run_uninstall(command),
    }
}

fn run_install(command: NodeInstallCommand) -> Result<()> {
    let plan = NodeServiceInstallPlan::from_command(&command)?;
    if command.dry_run {
        cli_ux::write_stdout(&plan.render_dry_run()).map_err(io_error)?;
        return Ok(());
    }
    ensure_linux_mutation("install Nimbus node service artifacts")?;
    plan.write_artifacts()?;
    run_systemctl(&["daemon-reload"], plan.scope)?;
    if plan.enable {
        run_systemctl(&["enable", plan.service_name()], plan.scope)?;
    }
    if plan.now {
        run_systemctl(&["start", plan.service_name()], plan.scope)?;
    }
    cli_ux::write_stdout_line(&format!(
        "Installed {} node service artifacts in {}.",
        plan.kind.label(),
        plan.scope.label()
    ))
    .map_err(io_error)
}

fn run_status(command: NodeStatusCommand) -> Result<()> {
    let target = target_from_optional_flags(command.systemd, command.container)?;
    let scope = scope_from_flags(command.user, command.system)?;
    ensure_linux_mutation("inspect Nimbus node service status")?;
    let unit = target.service_name();
    run_systemctl(&["status", unit], scope)
}

fn run_logs(command: NodeLogsCommand) -> Result<()> {
    let target = target_from_optional_flags(command.systemd, command.container)?;
    let scope = scope_from_flags(command.user, command.system)?;
    ensure_linux_mutation("read Nimbus node service logs")?;
    let mut args = journalctl_args(scope, target.service_name());
    if command.follow {
        args.push("-f".to_string());
    }
    run_process("journalctl", &args)
}

fn run_doctor(command: NodeDoctorCommand) -> Result<()> {
    let target = target_from_optional_flags(command.systemd, command.container)?;
    let scope = scope_from_flags(command.user, command.system)?;
    let report = NodeDoctorReport::probe(target, scope);
    cli_ux::write_stdout(&report.render_text()).map_err(io_error)
}

fn run_uninstall(command: NodeUninstallCommand) -> Result<()> {
    let target = target_from_optional_flags(command.systemd, command.container)?;
    let scope = scope_from_flags(command.user, command.system)?;
    let artifacts = uninstall_artifacts(target, scope)?;
    if command.dry_run {
        let mut rendered = String::new();
        rendered.push_str("Nimbus node uninstall dry-run\n");
        rendered.push_str(&format!("mode: {}\n", target.label()));
        rendered.push_str(&format!("scope: {}\n", scope.label()));
        for artifact in &artifacts {
            rendered.push_str(&format!("remove: {}\n", artifact.display()));
        }
        rendered.push_str(&format!(
            "systemctl: {}\n",
            systemctl_command_preview(scope, "daemon-reload")
        ));
        cli_ux::write_stdout(&rendered).map_err(io_error)?;
        return Ok(());
    }
    ensure_linux_mutation("uninstall Nimbus node service artifacts")?;
    for artifact in artifacts {
        match fs::remove_file(&artifact) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::InvalidInput(format!(
                    "failed to remove {}: {error}",
                    artifact.display()
                )));
            }
        }
    }
    run_systemctl(&["daemon-reload"], scope)?;
    cli_ux::write_stdout_line("Removed Nimbus node service artifacts.").map_err(io_error)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NodeServiceKind {
    NativeSystemd,
    ContainerQuadlet,
}

impl NodeServiceKind {
    fn label(self) -> &'static str {
        match self {
            Self::NativeSystemd => "native systemd",
            Self::ContainerQuadlet => "container Quadlet",
        }
    }

    fn service_name(self) -> &'static str {
        SERVICE_NAME
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NodeServiceScope {
    User,
    System,
}

impl NodeServiceScope {
    fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone)]
struct NodeServiceInstallPlan {
    kind: NodeServiceKind,
    scope: NodeServiceScope,
    artifacts: Vec<NodeServiceArtifact>,
    enable: bool,
    now: bool,
    overwrite: bool,
}

impl NodeServiceInstallPlan {
    fn from_command(command: &NodeInstallCommand) -> Result<Self> {
        let kind = target_from_required_flags(command.systemd, command.container)?;
        let scope = scope_from_flags(command.user, command.system)?;
        let artifacts = match kind {
            NodeServiceKind::NativeSystemd => {
                if command.image.is_some() {
                    return Err(Error::InvalidInput(
                        "--image is only valid with --container".to_string(),
                    ));
                }
                let binary = command
                    .binary
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_NATIVE_BINARY));
                NativeSystemdNodeService::new(scope, binary, command.socket_activation)?
                    .artifacts()?
            }
            NodeServiceKind::ContainerQuadlet => {
                if command.binary.is_some() {
                    return Err(Error::InvalidInput(
                        "--binary is only valid with --systemd".to_string(),
                    ));
                }
                if command.socket_activation {
                    return Err(Error::InvalidInput(
                        "--socket-activation is only valid with --systemd".to_string(),
                    ));
                }
                let image = command.image.as_ref().ok_or_else(|| {
                    Error::InvalidInput(
                        "nimbus node install --container requires --image".to_string(),
                    )
                })?;
                QuadletNodeService::new(scope, image)?.artifacts()?
            }
        };
        Ok(Self {
            kind,
            scope,
            artifacts,
            enable: command.enable,
            now: command.now,
            overwrite: command.overwrite,
        })
    }

    fn service_name(&self) -> &'static str {
        self.kind.service_name()
    }

    fn render_dry_run(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str("Nimbus node install dry-run\n");
        rendered.push_str(&format!("mode: {}\n", self.kind.label()));
        rendered.push_str(&format!("scope: {}\n", self.scope.label()));
        rendered.push_str(&format!("enable: {}\n", self.enable));
        rendered.push_str(&format!("now: {}\n", self.now));
        rendered.push('\n');
        for artifact in &self.artifacts {
            rendered.push_str(&format!(
                "### {} -> {}\n",
                artifact.kind.label(),
                artifact.path.display()
            ));
            rendered.push_str(&artifact.contents);
            if !artifact.contents.ends_with('\n') {
                rendered.push('\n');
            }
            rendered.push('\n');
        }
        rendered
    }

    fn write_artifacts(&self) -> Result<()> {
        for artifact in &self.artifacts {
            write_artifact(artifact, self.overwrite)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct NativeSystemdNodeService {
    scope: NodeServiceScope,
    binary: SafeUnitPath,
    data_dir: SafeUnitPath,
    control_dir: SafeUnitPath,
    socket_activation: bool,
}

impl NativeSystemdNodeService {
    fn new(scope: NodeServiceScope, binary: PathBuf, socket_activation: bool) -> Result<Self> {
        Ok(Self {
            scope,
            binary: SafeUnitPath::new(binary, "native Nimbus binary")?,
            data_dir: SafeUnitPath::new(DEFAULT_NATIVE_DATA_DIR, "native Nimbus data dir")?,
            control_dir: SafeUnitPath::new(
                DEFAULT_NATIVE_CONTROL_DIR,
                "native Nimbus control dir",
            )?,
            socket_activation,
        })
    }

    fn artifacts(&self) -> Result<Vec<NodeServiceArtifact>> {
        let mut artifacts = vec![NodeServiceArtifact {
            kind: NodeServiceArtifactKind::SystemdService,
            path: systemd_unit_dir(self.scope)?.join(SERVICE_NAME),
            contents: self.render_service(),
        }];
        if self.socket_activation {
            artifacts.push(NodeServiceArtifact {
                kind: NodeServiceArtifactKind::SystemdSocket,
                path: systemd_unit_dir(self.scope)?.join(SOCKET_NAME),
                contents: self.render_socket(),
            });
        }
        Ok(artifacts)
    }

    fn render_service(&self) -> String {
        let mut body = String::new();
        body.push_str("[Unit]\n");
        body.push_str("Description=Nimbus node service\n");
        body.push_str("Documentation=https://github.com/nimbus/nimbus\n");
        if self.socket_activation {
            body.push_str("Requires=nimbus.socket\n");
            body.push_str("After=nimbus.socket network-online.target local-fs.target\n");
        } else {
            body.push_str("After=network-online.target local-fs.target\n");
        }
        body.push_str("Wants=network-online.target\n\n");
        body.push_str("[Service]\n");
        body.push_str("Type=exec\n");
        body.push_str("KillMode=process\n");
        body.push_str(&format!("WorkingDirectory={}\n", self.data_dir.as_str()));
        body.push_str(&format!("Environment=HOME={}\n", self.data_dir.as_str()));
        body.push_str(&format!("ExecStart={}\n", self.exec_start()));
        body.push_str("Restart=on-failure\n");
        body.push_str("RestartSec=2\n");
        body.push_str("NoNewPrivileges=true\n");
        body.push_str("PrivateTmp=true\n");
        body.push_str("ProtectHome=true\n");
        body.push_str("ProtectSystem=full\n");
        body.push_str(&format!("ReadWritePaths={}\n", self.data_dir.as_str()));
        if self.scope == NodeServiceScope::System {
            body.push_str("StateDirectory=nimbus\n");
        }
        body.push_str("\n[Install]\n");
        body.push_str("WantedBy=multi-user.target\n");
        with_provenance(
            NATIVE_TEMPLATE_VERSION,
            "nimbus node install --systemd",
            &body,
        )
    }

    fn render_socket(&self) -> String {
        let body = format!(
            "[Unit]\nDescription=Nimbus node socket\nDocumentation=https://github.com/nimbus/nimbus\n\n[Socket]\nListenStream={DEFAULT_NATIVE_HOST}:{DEFAULT_NATIVE_PORT}\nSocketMode=0600\nDirectoryMode=0755\n\n[Install]\nWantedBy=sockets.target\n"
        );
        with_provenance(
            NATIVE_SOCKET_TEMPLATE_VERSION,
            "nimbus node install --systemd --socket-activation",
            &body,
        )
    }

    fn exec_start(&self) -> String {
        let mut args = vec![
            self.binary.as_str().to_string(),
            "start".to_string(),
            "--data-dir".to_string(),
            format!("{}/data", self.data_dir.as_str()),
            "--control-data-dir".to_string(),
            self.control_dir.as_str().to_string(),
        ];
        if self.socket_activation {
            args.push("--systemd-socket-activation".to_string());
        } else {
            args.extend([
                "--host".to_string(),
                DEFAULT_NATIVE_HOST.to_string(),
                "--port".to_string(),
                DEFAULT_NATIVE_PORT.to_string(),
            ]);
        }
        args.join(" ")
    }
}

#[derive(Debug, Clone)]
struct QuadletNodeService {
    scope: NodeServiceScope,
    image: SafeImageReference,
}

impl QuadletNodeService {
    fn new(scope: NodeServiceScope, image: impl Into<String>) -> Result<Self> {
        Ok(Self {
            scope,
            image: SafeImageReference::new(image)?,
        })
    }

    fn artifacts(&self) -> Result<Vec<NodeServiceArtifact>> {
        Ok(vec![NodeServiceArtifact {
            kind: NodeServiceArtifactKind::QuadletContainer,
            path: quadlet_unit_dir(self.scope)?.join(QUADLET_NAME),
            contents: self.render_container(),
        }])
    }

    fn render_container(&self) -> String {
        let body = format!(
            "[Unit]\nDescription=Nimbus node container\nDocumentation=https://github.com/nimbus/nimbus\nAfter=network-online.target\nWants=network-online.target\n\n[Container]\nImage={image}\nContainerName=nimbus\nPublishPort={DEFAULT_QUADLET_PUBLISH_PORT}\nVolume={DEFAULT_QUADLET_VOLUME}\nHealthCmd={DEFAULT_HEALTH_CMD}\nHealthInterval=30s\nHealthTimeout=5s\nHealthRetries=3\n\n[Service]\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
            image = self.image.as_str(),
        );
        with_provenance(
            QUADLET_TEMPLATE_VERSION,
            "nimbus node install --container",
            &body,
        )
    }
}

#[derive(Debug, Clone)]
struct NodeServiceArtifact {
    kind: NodeServiceArtifactKind,
    path: PathBuf,
    contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeServiceArtifactKind {
    SystemdService,
    SystemdSocket,
    QuadletContainer,
}

impl NodeServiceArtifactKind {
    fn label(self) -> &'static str {
        match self {
            Self::SystemdService => "nimbus.service",
            Self::SystemdSocket => "nimbus.socket",
            Self::QuadletContainer => "nimbus.container",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SafeUnitPath(String);

impl SafeUnitPath {
    fn new(path: impl Into<PathBuf>, field: &str) -> Result<Self> {
        let path = path.into();
        let value = path.to_string_lossy().to_string();
        if !path.is_absolute()
            || value.is_empty()
            || value.contains('\0')
            || value.contains('\n')
            || value.contains(char::is_whitespace)
            || value.contains('%')
            || value.contains(';')
        {
            return Err(Error::InvalidInput(format!(
                "{field} `{value}` must be an absolute path without whitespace, systemd specifiers, or control characters"
            )));
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SafeImageReference(String);

impl SafeImageReference {
    fn new(image: impl Into<String>) -> Result<Self> {
        let image = image.into();
        if image.is_empty()
            || image.contains('\0')
            || image.contains('\n')
            || image.contains(char::is_whitespace)
            || image.starts_with('-')
        {
            return Err(Error::InvalidInput(
                "Nimbus image reference must be non-empty and must not contain shell/control characters"
                    .to_string(),
            ));
        }
        if !image.starts_with("ghcr.io/nimbus/nimbus:") {
            return Err(Error::PermissionDenied(format!(
                "containerized node installs only accept ghcr.io/nimbus/nimbus images, got `{image}`"
            )));
        }
        if image == "ghcr.io/nimbus/nimbus:latest"
            || image.starts_with("ghcr.io/nimbus/nimbus:latest@")
        {
            return Err(Error::PermissionDenied(
                "containerized node installs must pin an explicit Nimbus release tag or digest, not latest"
                    .to_string(),
            ));
        }
        Ok(Self(image))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Serialize)]
struct NodeDoctorReport {
    mode: NodeServiceKind,
    scope: NodeServiceScope,
    linux: bool,
    systemctl_available: bool,
    journalctl_available: bool,
    podman_available: Option<bool>,
    artifact_dir: String,
    service_name: &'static str,
    diagnostics: Vec<String>,
}

impl NodeDoctorReport {
    fn probe(mode: NodeServiceKind, scope: NodeServiceScope) -> Self {
        let linux = cfg!(target_os = "linux");
        let systemctl_available = command_available("systemctl");
        let journalctl_available = command_available("journalctl");
        let podman_available = if mode == NodeServiceKind::ContainerQuadlet {
            Some(command_available("podman"))
        } else {
            None
        };
        let artifact_dir = match mode {
            NodeServiceKind::NativeSystemd => systemd_unit_dir(scope),
            NodeServiceKind::ContainerQuadlet => quadlet_unit_dir(scope),
        }
        .map(|path| path.display().to_string())
        .unwrap_or_else(|error| format!("unresolved: {error}"));
        let mut diagnostics = Vec::new();
        if !linux {
            diagnostics.push(
                "host mutation requires Linux systemd; use --dry-run for review on this platform"
                    .to_string(),
            );
        }
        if !systemctl_available {
            diagnostics.push("systemctl was not found on PATH".to_string());
        }
        if !journalctl_available {
            diagnostics.push("journalctl was not found on PATH".to_string());
        }
        if podman_available == Some(false) {
            diagnostics.push("podman was not found on PATH for Quadlet node installs".to_string());
        }
        if mode == NodeServiceKind::ContainerQuadlet {
            diagnostics.push(
                "Quadlet support is provided by the host Podman/systemd generator; Nimbus does not run systemd inside the container"
                    .to_string(),
            );
        }
        Self {
            mode,
            scope,
            linux,
            systemctl_available,
            journalctl_available,
            podman_available,
            artifact_dir,
            service_name: SERVICE_NAME,
            diagnostics,
        }
    }

    fn render_text(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str("Nimbus node doctor\n");
        rendered.push_str(&format!("mode: {}\n", self.mode.label()));
        rendered.push_str(&format!("scope: {}\n", self.scope.label()));
        rendered.push_str(&format!("linux: {}\n", self.linux));
        rendered.push_str(&format!(
            "systemctl: {}\n",
            availability(self.systemctl_available)
        ));
        rendered.push_str(&format!(
            "journalctl: {}\n",
            availability(self.journalctl_available)
        ));
        if let Some(podman) = self.podman_available {
            rendered.push_str(&format!("podman: {}\n", availability(podman)));
        }
        rendered.push_str(&format!("artifact_dir: {}\n", self.artifact_dir));
        rendered.push_str(&format!("service: {}\n", self.service_name));
        if self.diagnostics.is_empty() {
            rendered.push_str("diagnostics: ok\n");
        } else {
            rendered.push_str("diagnostics:\n");
            for diagnostic in &self.diagnostics {
                rendered.push_str(&format!("- {diagnostic}\n"));
            }
        }
        rendered
    }
}

fn target_from_required_flags(systemd: bool, container: bool) -> Result<NodeServiceKind> {
    match (systemd, container) {
        (true, false) => Ok(NodeServiceKind::NativeSystemd),
        (false, true) => Ok(NodeServiceKind::ContainerQuadlet),
        (false, false) => Err(Error::InvalidInput(
            "choose exactly one node install target: --systemd or --container".to_string(),
        )),
        (true, true) => Err(Error::InvalidInput(
            "choose only one node install target: --systemd or --container".to_string(),
        )),
    }
}

fn target_from_optional_flags(systemd: bool, container: bool) -> Result<NodeServiceKind> {
    match (systemd, container) {
        (false, false) | (true, false) => Ok(NodeServiceKind::NativeSystemd),
        (false, true) => Ok(NodeServiceKind::ContainerQuadlet),
        (true, true) => Err(Error::InvalidInput(
            "choose only one node service target: --systemd or --container".to_string(),
        )),
    }
}

fn scope_from_flags(user: bool, system: bool) -> Result<NodeServiceScope> {
    match (user, system) {
        (true, false) => Ok(NodeServiceScope::User),
        (false, false) | (false, true) => Ok(NodeServiceScope::System),
        (true, true) => Err(Error::InvalidInput(
            "choose only one node service scope: --user or --system".to_string(),
        )),
    }
}

fn systemd_unit_dir(scope: NodeServiceScope) -> Result<PathBuf> {
    match scope {
        NodeServiceScope::System => Ok(PathBuf::from("/etc/systemd/system")),
        NodeServiceScope::User => Ok(user_config_home()?.join("systemd").join("user")),
    }
}

fn quadlet_unit_dir(scope: NodeServiceScope) -> Result<PathBuf> {
    match scope {
        NodeServiceScope::System => Ok(PathBuf::from("/etc/containers/systemd")),
        NodeServiceScope::User => Ok(user_config_home()?.join("containers").join("systemd")),
    }
}

fn user_config_home() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".config"));
    }
    Err(Error::InvalidInput(
        "HOME is not set; cannot resolve user service-manager directory".to_string(),
    ))
}

fn uninstall_artifacts(target: NodeServiceKind, scope: NodeServiceScope) -> Result<Vec<PathBuf>> {
    match target {
        NodeServiceKind::NativeSystemd => Ok(vec![
            systemd_unit_dir(scope)?.join(SERVICE_NAME),
            systemd_unit_dir(scope)?.join(SOCKET_NAME),
        ]),
        NodeServiceKind::ContainerQuadlet => Ok(vec![quadlet_unit_dir(scope)?.join(QUADLET_NAME)]),
    }
}

fn with_provenance(template: &str, source_command: &str, body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(template.as_bytes());
    hasher.update(b"\n");
    hasher.update(source_command.as_bytes());
    hasher.update(b"\n");
    hasher.update(body.as_bytes());
    let hash = hasher.finalize();
    format!(
        "# Generated by Nimbus. Do not edit tenant-controlled data into this file.\n# Nimbus-Template: {template}\n# Nimbus-Source-Command: {source_command}\n# Nimbus-Provenance-SHA256: sha256:{hash:x}\n{body}"
    )
}

fn write_artifact(artifact: &NodeServiceArtifact, overwrite: bool) -> Result<()> {
    if artifact.path.exists() && !overwrite {
        return Err(Error::AlreadyExists(format!(
            "{} exists; pass --overwrite to replace it",
            artifact.path.display()
        )));
    }
    let parent = artifact.path.parent().ok_or_else(|| {
        Error::InvalidInput(format!(
            "artifact path {} has no parent directory",
            artifact.path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to create artifact directory {}: {error}",
            parent.display()
        ))
    })?;
    fs::write(&artifact.path, artifact.contents.as_bytes()).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to write {}: {error}",
            artifact.path.display()
        ))
    })
}

fn ensure_linux_mutation(action: &str) -> Result<()> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err(Error::ResourceExhausted(format!(
            "cannot {action}: host service mutation requires Linux systemd; use --dry-run to render artifacts for review"
        )))
    }
}

fn run_systemctl(args: &[&str], scope: NodeServiceScope) -> Result<()> {
    let mut owned = Vec::new();
    if scope == NodeServiceScope::User {
        owned.push("--user".to_string());
    }
    owned.extend(args.iter().map(|arg| (*arg).to_string()));
    run_process("systemctl", &owned)
}

fn run_process(program: &str, args: &[String]) -> Result<()> {
    let status = ProcessCommand::new(program)
        .args(args)
        .status()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to run {} {}: {error}",
                program,
                args.join(" ")
            ))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "{} {} exited with status {status}",
            program,
            args.join(" ")
        )))
    }
}

fn journalctl_args(scope: NodeServiceScope, unit: &str) -> Vec<String> {
    match scope {
        NodeServiceScope::User => vec!["--user-unit".to_string(), unit.to_string()],
        NodeServiceScope::System => vec!["-u".to_string(), unit.to_string()],
    }
}

fn systemctl_command_preview(scope: NodeServiceScope, action: &str) -> String {
    match scope {
        NodeServiceScope::User => format!("systemctl --user {action}"),
        NodeServiceScope::System => format!("systemctl {action}"),
    }
}

fn command_available(program: &str) -> bool {
    ProcessCommand::new(program)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn availability(available: bool) -> &'static str {
    if available { "available" } else { "missing" }
}

fn io_error(error: std::io::Error) -> Error {
    Error::InvalidInput(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn native_systemd_dry_run_renders_service_with_provenance_and_hardening() {
        let command = NodeInstallCommand {
            systemd: true,
            container: false,
            user: false,
            system: true,
            dry_run: true,
            enable: true,
            now: false,
            overwrite: false,
            binary: Some(PathBuf::from("/usr/local/bin/nimbus")),
            image: None,
            socket_activation: false,
        };

        let plan = NodeServiceInstallPlan::from_command(&command).expect("plan should build");
        let rendered = plan.render_dry_run();

        assert!(rendered.contains("mode: native systemd"));
        assert!(rendered.contains("### nimbus.service -> /etc/systemd/system/nimbus.service"));
        assert!(rendered.contains("# Nimbus-Template: native-systemd-node-service/v1"));
        assert!(rendered.contains("# Nimbus-Provenance-SHA256: sha256:"));
        assert!(rendered.contains("ExecStart=/usr/local/bin/nimbus start"));
        assert!(rendered.contains("--host 127.0.0.1 --port 8080"));
        assert!(rendered.contains("NoNewPrivileges=true"));
        assert!(rendered.contains("ProtectSystem=full"));
        assert!(!rendered.contains("PodmanArgs"));
        assert!(!rendered.contains("Privileged=true"));
    }

    #[test]
    fn native_socket_activation_renders_matching_socket_and_service() {
        let service = NativeSystemdNodeService::new(
            NodeServiceScope::User,
            PathBuf::from("/opt/nimbus/bin/nimbus"),
            true,
        )
        .expect("service should build");

        let artifacts = service.artifacts().expect("artifacts should render");
        assert_eq!(artifacts.len(), 2);
        let service = artifacts
            .iter()
            .find(|artifact| artifact.kind == NodeServiceArtifactKind::SystemdService)
            .expect("service artifact should exist");
        let socket = artifacts
            .iter()
            .find(|artifact| artifact.kind == NodeServiceArtifactKind::SystemdSocket)
            .expect("socket artifact should exist");

        assert!(service.contents.contains("Requires=nimbus.socket"));
        assert!(service.contents.contains("--systemd-socket-activation"));
        assert!(socket.contents.contains("ListenStream=127.0.0.1:8080"));
        assert!(
            socket
                .contents
                .contains("# Nimbus-Template: native-systemd-node-socket/v1")
        );
    }

    #[test]
    fn quadlet_dry_run_preserves_container_image_contract_without_escape_hatches() {
        let command = NodeInstallCommand {
            systemd: false,
            container: true,
            user: true,
            system: false,
            dry_run: true,
            enable: true,
            now: true,
            overwrite: false,
            binary: None,
            image: Some("ghcr.io/nimbus/nimbus:v1.2.3@sha256:abc123".to_string()),
            socket_activation: false,
        };

        let plan = NodeServiceInstallPlan::from_command(&command).expect("plan should build");
        let rendered = plan.render_dry_run();

        assert!(rendered.contains("mode: container Quadlet"));
        assert!(rendered.contains("scope: user"));
        assert!(rendered.contains("Image=ghcr.io/nimbus/nimbus:v1.2.3@sha256:abc123"));
        assert!(rendered.contains("PublishPort=127.0.0.1:8080:8080"));
        assert!(rendered.contains("Volume=nimbus-data:/var/lib/nimbus"));
        assert!(rendered.contains("HealthCmd=curl -fsS http://127.0.0.1:8080/health"));
        assert!(rendered.contains("# Nimbus-Template: quadlet-node-service/v1"));
        assert!(!rendered.contains("PodmanArgs"));
        assert!(!rendered.contains("Network=host"));
        assert!(!rendered.contains("Privileged=true"));
        assert!(!rendered.contains("systemd inside"));
    }

    #[test]
    fn quadlet_rejects_non_nimbus_or_latest_image_references() {
        let error = SafeImageReference::new("docker.io/library/nginx:latest")
            .expect_err("non-nimbus image should fail");
        assert!(
            error
                .to_string()
                .contains("only accept ghcr.io/nimbus/nimbus")
        );

        let error = SafeImageReference::new("ghcr.io/nimbus/nimbus:latest")
            .expect_err("latest should fail");
        assert!(error.to_string().contains("not latest"));

        let image = SafeImageReference::new("ghcr.io/nimbus/nimbus:v1.2.3")
            .expect("explicit tag should pass");
        assert_eq!(image.as_str(), "ghcr.io/nimbus/nimbus:v1.2.3");
    }

    #[test]
    fn native_paths_reject_systemd_specifiers_and_whitespace() {
        let error = SafeUnitPath::new("/usr/local/bin/nim bus", "binary")
            .expect_err("whitespace should fail");
        assert!(error.to_string().contains("without whitespace"));

        let error =
            SafeUnitPath::new("/usr/local/bin/%n", "binary").expect_err("specifier should fail");
        assert!(error.to_string().contains("systemd specifiers"));
    }

    #[test]
    fn artifact_write_refuses_overwrite_without_opt_in() {
        let temp = tempfile::tempdir().expect("temp dir should create");
        let path = temp.path().join("nimbus.service");
        fs::write(&path, "existing\n").expect("fixture should write");
        let artifact = NodeServiceArtifact {
            kind: NodeServiceArtifactKind::SystemdService,
            path: path.clone(),
            contents: "new\n".to_string(),
        };

        let error = write_artifact(&artifact, false).expect_err("overwrite should fail");
        assert!(error.to_string().contains("--overwrite"));
        assert_eq!(
            fs::read_to_string(&path).expect("fixture should read"),
            "existing\n"
        );

        write_artifact(&artifact, true).expect("overwrite should pass");
        assert_eq!(
            fs::read_to_string(&path).expect("fixture should read"),
            "new\n"
        );
    }

    #[test]
    fn doctor_reports_container_mode_without_systemd_in_container() {
        let report =
            NodeDoctorReport::probe(NodeServiceKind::ContainerQuadlet, NodeServiceScope::System);
        let rendered = report.render_text();

        assert!(rendered.contains("mode: container Quadlet"));
        assert!(rendered.contains("service: nimbus.service"));
        assert!(rendered.contains("Nimbus does not run systemd inside the container"));
    }

    #[test]
    fn node_service_cli_commands_parse() {
        let cli = crate::Cli::parse_from(["nimbus", "node", "install", "--systemd", "--dry-run"]);
        assert!(
            matches!(
                cli.command,
                crate::Command::Node(NodeCommand {
                    command: NodeSubcommand::Install(_)
                })
            ),
            "node install should parse"
        );

        let cli = crate::Cli::parse_from([
            "nimbus",
            "node",
            "install",
            "--container",
            "--image",
            "ghcr.io/nimbus/nimbus:v1.2.3",
            "--user",
            "--dry-run",
        ]);
        assert!(
            matches!(
                cli.command,
                crate::Command::Node(NodeCommand {
                    command: NodeSubcommand::Install(_)
                })
            ),
            "container node install should parse"
        );
    }
}
