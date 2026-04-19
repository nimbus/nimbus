use super::inspect::BuildahImageFields;
#[cfg(test)]
use super::inspect::BuildahInspectPayload;
use super::*;

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

impl OciImageConfig {
    #[cfg(test)]
    pub(super) fn from_payload(payload: BuildahInspectPayload) -> Self {
        Self::from_fields(
            payload
                .oci_v1
                .map(|payload| payload.config)
                .unwrap_or_default(),
            payload
                .docker
                .map(|payload| payload.config)
                .unwrap_or_default(),
        )
    }

    pub(super) fn from_fields(oci: BuildahImageFields, docker: BuildahImageFields) -> Self {
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
