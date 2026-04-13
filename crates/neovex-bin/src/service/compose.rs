use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use neovex::{
    Error, PublishedEndpointProtocol, SandboxBackendKind, SandboxBuildLaunchSpec,
    SandboxFilesystemSpec, SandboxImageLaunchSpec, SandboxImageProcessOverrides,
    SandboxLifecycleSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRestartPolicy,
    SandboxServiceCatalog, SandboxServiceLaunch, SandboxSpec, TenantId,
};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

pub(crate) const DEFAULT_COMPOSE_FILE: &str = "compose.yaml";
const CONFIG_VALIDATION_TENANT_ID: &str = "compose-config";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedComposeProject {
    pub(crate) stdout: String,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeProjectPlan {
    pub(crate) source_file: PathBuf,
    pub(crate) project_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) volumes: Vec<String>,
    pub(crate) services: BTreeMap<String, ComposeServicePlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

impl ComposeProjectPlan {
    pub(crate) fn load(path: &Path) -> Result<Self, Error> {
        let bytes = fs::read(path).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to read compose file {}: {error}",
                path.display()
            ))
        })?;
        let raw: RawComposeDocument = serde_yaml::from_slice(&bytes).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to parse compose file {} as YAML: {error}",
                path.display()
            ))
        })?;
        Self::from_raw(path, raw)
    }

    fn from_raw(path: &Path, raw: RawComposeDocument) -> Result<Self, Error> {
        if raw.services.is_empty() {
            return Err(Error::InvalidInput(format!(
                "{}: missing top-level services map",
                path.display()
            )));
        }

        let compose_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let project_name = raw
            .name
            .as_deref()
            .map(sanitize_project_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| sanitize_project_name(default_project_name(path)));

        let mut warnings = Vec::new();
        if !raw.networks.is_empty() {
            warnings.push(format!(
                "{}: top-level networks: ignored (neovex uses TSI networking)",
                path.display()
            ));
        }
        if !raw.configs.is_empty() {
            warnings.push(format!(
                "{}: top-level configs: ignored (not yet supported by neovex service config)",
                path.display()
            ));
        }
        if !raw.secrets.is_empty() {
            warnings.push(format!(
                "{}: top-level secrets: ignored (not yet supported by neovex service config)",
                path.display()
            ));
        }
        warnings.extend(warnings_for_unknown_fields(
            &format!("{}", path.display()),
            raw.extra,
        ));

        let mut services = BTreeMap::new();
        for (service_name, service) in raw.services {
            let resolved =
                ComposeServicePlan::from_raw(&service_name, &project_name, compose_dir, service)?;
            services.insert(service_name, resolved);
        }

        Ok(Self {
            source_file: path.to_path_buf(),
            project_name,
            volumes: raw.volumes.into_keys().collect(),
            services,
            warnings,
        })
    }

    pub(crate) fn all_warnings(&self) -> Vec<String> {
        let mut warnings = self.warnings.clone();
        for (service_name, service) in &self.services {
            for warning in &service.warnings {
                warnings.push(format!("services.{service_name}: {warning}"));
            }
        }
        warnings
    }

    pub(crate) fn render(&self) -> Result<String, Error> {
        serde_yaml::to_string(self).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to render resolved service config from {}: {error}",
                self.source_file.display()
            ))
        })
    }

    pub(crate) fn render_service_names(&self) -> String {
        self.services.keys().cloned().collect::<Vec<_>>().join("\n")
    }

    pub(crate) fn into_service_catalog(self) -> Result<ComposeServiceCatalog, Error> {
        let tenant_id = TenantId::new(CONFIG_VALIDATION_TENANT_ID)
            .expect("config validation tenant id should remain valid");
        for (service_name, service) in &self.services {
            let _ = service.to_sandbox_service_launch(&tenant_id, service_name)?;
        }
        Ok(ComposeServiceCatalog { project: self })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposeServiceCatalog {
    project: ComposeProjectPlan,
}

impl SandboxServiceCatalog for ComposeServiceCatalog {
    fn sandbox_service_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxServiceLaunch> {
        self.project.services.get(service_name).map(|service| {
            service
                .to_sandbox_service_launch(tenant_id, service_name)
                .expect("validated compose services should keep lowering through the server catalog seam")
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeServicePlan {
    pub(crate) backend: SandboxBackendKind,
    pub(crate) source: ComposeLaunchPlan,
    pub(crate) process: ComposeProcessPlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) ports: Vec<ComposePortBindingPlan>,
    pub(crate) resources: ComposeResourcePlan,
    pub(crate) restart: ComposeRestartPlan,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) depends_on: BTreeMap<String, ComposeDependencyCondition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) healthcheck: Option<ComposeHealthcheckPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop_grace_period: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) volumes: Vec<ComposeVolumeMountPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) x_neovex: Option<ComposeNeovexPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

impl ComposeServicePlan {
    fn from_raw(
        service_name: &str,
        project_name: &str,
        compose_dir: &Path,
        raw: RawComposeService,
    ) -> Result<Self, Error> {
        let mut warnings = Vec::new();
        warnings.extend(warnings_for_known_ignored_service_fields(&raw));
        warnings.extend(warnings_for_unknown_fields(
            &format!("services.{service_name}"),
            raw.extra.clone(),
        ));

        let source = ComposeLaunchPlan::from_raw(
            service_name,
            project_name,
            compose_dir,
            raw.image.as_deref(),
            raw.build,
        )?;
        let process = ComposeProcessPlan::from_raw(
            compose_dir,
            raw.environment,
            raw.env_file,
            raw.entrypoint,
            raw.command,
            raw.working_dir,
            raw.user,
        )?;
        let _ = process.to_image_process_overrides()?;
        let ports = resolve_ports(service_name, raw.ports, &mut warnings)?;
        let resources = ComposeResourcePlan::from_raw(service_name, raw.deploy, &mut warnings)?;
        let restart = ComposeRestartPlan::from_raw(raw.restart.as_deref(), &mut warnings)?;
        let _ = compose_lifecycle_spec(
            &restart,
            raw.stop_grace_period.as_deref(),
            &format!("services.{service_name}.stop_grace_period"),
        )?;
        let depends_on = resolve_depends_on(raw.depends_on)?;
        let healthcheck = raw
            .healthcheck
            .map(ComposeHealthcheckPlan::from_raw)
            .transpose()?;
        let labels = parse_string_map(raw.labels, &format!("services.{service_name}.labels"))?;
        let volumes = resolve_volume_mounts(raw.volumes);

        Ok(Self {
            backend: SandboxBackendKind::Krun,
            source,
            process,
            ports,
            resources,
            restart,
            depends_on,
            healthcheck,
            stop_grace_period: raw.stop_grace_period,
            labels,
            volumes,
            x_neovex: raw.x_neovex,
            warnings,
        })
    }

    fn to_sandbox_service_launch(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<SandboxServiceLaunch, Error> {
        let spec = self.to_sandbox_spec(tenant_id, service_name)?;
        let process_overrides = self.process.to_image_process_overrides()?;
        match &self.source {
            ComposeLaunchPlan::Image { image_reference } => Ok(SandboxServiceLaunch::image(
                SandboxImageLaunchSpec::new(spec, image_reference.clone())
                    .with_process_overrides(process_overrides),
            )),
            ComposeLaunchPlan::Build {
                image_name,
                dockerfile_path,
                context_path,
            } => Ok(SandboxServiceLaunch::build(
                SandboxBuildLaunchSpec::new(
                    spec,
                    image_name.clone(),
                    dockerfile_path.clone(),
                    context_path.clone(),
                )
                .with_process_overrides(process_overrides),
            )),
        }
    }

    fn to_sandbox_spec(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<SandboxSpec, Error> {
        let mut spec = SandboxSpec::new(
            tenant_id.clone(),
            service_name,
            self.backend,
            SandboxFilesystemSpec::new(PathBuf::new()),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .with_lifecycle(compose_lifecycle_spec(
            &self.restart,
            self.stop_grace_period.as_deref(),
            "services.<service>.stop_grace_period",
        )?)
        .with_port_bindings(
            self.ports
                .iter()
                .cloned()
                .map(ComposePortBindingPlan::into_binding),
        );

        if let Some(cpu_count) = self.resources.cpu_count {
            spec = spec.with_cpu_count(cpu_count);
        }
        if let Some(memory_limit_bytes) = self.resources.memory_limit_bytes {
            spec = spec.with_memory_limit_bytes(memory_limit_bytes);
        }
        Ok(spec)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum ComposeLaunchPlan {
    Image {
        image_reference: String,
    },
    Build {
        image_name: String,
        dockerfile_path: PathBuf,
        context_path: PathBuf,
    },
}

impl ComposeLaunchPlan {
    fn from_raw(
        service_name: &str,
        project_name: &str,
        compose_dir: &Path,
        image: Option<&str>,
        build: Option<RawComposeBuild>,
    ) -> Result<Self, Error> {
        match (image, build) {
            (Some(image_reference), None) => Ok(Self::Image {
                image_reference: image_reference.to_owned(),
            }),
            (image_name, Some(build)) => {
                let (context_path, dockerfile_path) = build.resolve_paths(compose_dir)?;
                Ok(Self::Build {
                    image_name: image_name
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| default_build_image_name(project_name, service_name)),
                    dockerfile_path,
                    context_path,
                })
            }
            (None, None) => Err(Error::InvalidInput(format!(
                "services.{service_name}: expected either image or build"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeProcessPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entrypoint: Option<ComposeCommandPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<ComposeCommandPlan>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) environment: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) working_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) user: Option<String>,
}

impl ComposeProcessPlan {
    fn from_raw(
        compose_dir: &Path,
        environment: Option<RawComposeStringMap>,
        env_file: Option<RawComposeEnvFile>,
        entrypoint: Option<ComposeCommandPlan>,
        command: Option<ComposeCommandPlan>,
        working_dir: Option<String>,
        user: Option<String>,
    ) -> Result<Self, Error> {
        let mut merged_environment = load_env_files(compose_dir, env_file)?;
        let inline_environment =
            parse_environment_map(environment, "services.<service>.environment")?;
        merged_environment.extend(inline_environment);

        Ok(Self {
            entrypoint,
            command,
            environment: merged_environment,
            working_dir: working_dir.map(PathBuf::from),
            user,
        })
    }

    pub(crate) fn to_image_process_overrides(&self) -> Result<SandboxImageProcessOverrides, Error> {
        let mut overrides = SandboxImageProcessOverrides::default();
        if let Some(entrypoint) = &self.entrypoint {
            overrides.entrypoint = Some(command_plan_to_argv(
                entrypoint,
                "services.<service>.entrypoint",
            )?);
        }
        if let Some(command) = &self.command {
            overrides.cmd = Some(command_plan_to_argv(command, "services.<service>.command")?);
        }
        overrides.env = self
            .environment
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        overrides.cwd = self.working_dir.clone();
        overrides.user = self.user.clone();
        Ok(overrides)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ComposeCommandPlan {
    String(String),
    List(Vec<String>),
}

fn command_plan_to_argv(
    command: &ComposeCommandPlan,
    field_label: &str,
) -> Result<Vec<String>, Error> {
    let argv = match command {
        ComposeCommandPlan::String(command) => shell_words::split(command).map_err(|error| {
            Error::InvalidInput(format!(
                "{field_label}: failed to parse shell-style command {command:?}: {error}"
            ))
        })?,
        ComposeCommandPlan::List(argv) => argv.clone(),
    };
    if argv.is_empty() {
        return Err(Error::InvalidInput(format!(
            "{field_label}: empty command/entrypoint overrides are not supported by the current sandbox launch surface"
        )));
    }
    Ok(argv)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposePortBindingPlan {
    pub(crate) name: String,
    pub(crate) protocol: PublishedEndpointProtocol,
    pub(crate) host_address: IpAddr,
    pub(crate) host_port: u16,
    pub(crate) guest_port: u16,
}

impl ComposePortBindingPlan {
    fn into_binding(self) -> SandboxPortBinding {
        SandboxPortBinding::new(self.name, self.protocol, self.host_port, self.guest_port)
            .with_host_address(self.host_address)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeResourcePlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) requested_cpus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cpu_count: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) requested_memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) memory_limit_bytes: Option<u64>,
}

impl ComposeResourcePlan {
    fn from_raw(
        service_name: &str,
        deploy: Option<RawComposeDeploy>,
        warnings: &mut Vec<String>,
    ) -> Result<Self, Error> {
        let Some(deploy) = deploy else {
            return Ok(Self {
                requested_cpus: None,
                cpu_count: None,
                requested_memory: None,
                memory_limit_bytes: None,
            });
        };

        if deploy.replicas.is_some() {
            warnings
                .push("deploy.replicas: ignored (neovex handles scaling separately)".to_owned());
        }
        if deploy.placement.is_some() {
            warnings.push(
                "deploy.placement: ignored (single-node placement only in the current M5 slice)"
                    .to_owned(),
            );
        }

        let limits = deploy.resources.and_then(|resources| resources.limits);
        let requested_cpus = limits.as_ref().and_then(|limits| limits.cpus.clone());
        let requested_memory = limits.as_ref().and_then(|limits| limits.memory.clone());
        let cpu_count = requested_cpus
            .as_deref()
            .map(|value| parse_cpu_count(service_name, value, warnings))
            .transpose()?;
        let memory_limit_bytes = requested_memory
            .as_deref()
            .map(|value| parse_memory_limit(service_name, value))
            .transpose()?;

        Ok(Self {
            requested_cpus,
            cpu_count,
            requested_memory,
            memory_limit_bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeRestartPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) requested: Option<String>,
    pub(crate) policy: SandboxRestartPolicy,
}

impl ComposeRestartPlan {
    fn from_raw(restart: Option<&str>, warnings: &mut Vec<String>) -> Result<Self, Error> {
        let requested = restart.map(ToOwned::to_owned);
        let policy = match restart.unwrap_or("no").trim() {
            "" | "no" => SandboxRestartPolicy::Never,
            "always" => SandboxRestartPolicy::Always {
                max_restarts: u32::MAX,
            },
            "unless-stopped" => {
                warnings.push(
                    "restart: unless-stopped is currently lowered as always with no max restarts"
                        .to_owned(),
                );
                SandboxRestartPolicy::Always {
                    max_restarts: u32::MAX,
                }
            }
            policy if policy.starts_with("on-failure") => {
                let max_restarts = policy
                    .split_once(':')
                    .map(|(_, count)| {
                        count.trim().parse::<u32>().map_err(|error| {
                            Error::InvalidInput(format!(
                                "restart policy on-failure:{count} has an invalid retry count: {error}"
                            ))
                        })
                    })
                    .transpose()?
                    .unwrap_or(u32::MAX);
                SandboxRestartPolicy::OnFailure { max_restarts }
            }
            other => {
                return Err(Error::InvalidInput(format!(
                    "unsupported restart policy {other:?}; expected no, always, unless-stopped, or on-failure[:max-retries]"
                )));
            }
        };

        Ok(Self { requested, policy })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComposeDependencyCondition {
    ServiceStarted,
    ServiceHealthy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeHealthcheckPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) test: Option<ComposeCommandPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) interval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_period: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) disable: bool,
}

impl ComposeHealthcheckPlan {
    fn from_raw(raw: RawComposeHealthcheck) -> Result<Self, Error> {
        if !raw.extra.is_empty() {
            return Err(Error::InvalidInput(format!(
                "unsupported healthcheck fields: {}",
                raw.extra.keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        }

        Ok(Self {
            test: raw.test,
            interval: raw.interval,
            timeout: raw.timeout,
            retries: raw.retries,
            start_period: raw.start_period,
            disable: raw.disable.unwrap_or(false),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeVolumeMountPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    pub(crate) target: String,
    pub(crate) kind: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ComposeNeovexPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) idle_timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) snapshot: Option<bool>,
}

pub(crate) fn render_compose_project(
    path: &Path,
    list_services: bool,
) -> Result<RenderedComposeProject, Error> {
    let project = ComposeProjectPlan::load(path)?;
    let _catalog = project.clone().into_service_catalog()?;
    let warnings = if list_services {
        project.all_warnings()
    } else {
        Vec::new()
    };
    let stdout = if list_services {
        let rendered = project.render_service_names();
        if rendered.is_empty() {
            String::new()
        } else {
            format!("{rendered}\n")
        }
    } else {
        project.render()?
    };
    Ok(RenderedComposeProject { stdout, warnings })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawComposeDocument {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    services: BTreeMap<String, RawComposeService>,
    #[serde(default)]
    volumes: BTreeMap<String, Value>,
    #[serde(default)]
    networks: BTreeMap<String, Value>,
    #[serde(default)]
    configs: BTreeMap<String, Value>,
    #[serde(default)]
    secrets: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeService {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    build: Option<RawComposeBuild>,
    #[serde(default)]
    environment: Option<RawComposeStringMap>,
    #[serde(default)]
    env_file: Option<RawComposeEnvFile>,
    #[serde(default)]
    ports: Vec<String>,
    #[serde(default)]
    volumes: Vec<RawComposeVolumeMount>,
    #[serde(default)]
    deploy: Option<RawComposeDeploy>,
    #[serde(default)]
    restart: Option<String>,
    #[serde(default)]
    depends_on: Option<RawComposeDependsOn>,
    #[serde(default)]
    stop_grace_period: Option<String>,
    #[serde(default)]
    command: Option<ComposeCommandPlan>,
    #[serde(default)]
    entrypoint: Option<ComposeCommandPlan>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    labels: Option<RawComposeStringMap>,
    #[serde(default)]
    healthcheck: Option<RawComposeHealthcheck>,
    #[serde(default)]
    x_neovex: Option<ComposeNeovexPlan>,
    #[serde(default)]
    networks: Option<Value>,
    #[serde(default)]
    configs: Option<Value>,
    #[serde(default)]
    secrets: Option<Value>,
    #[serde(default)]
    cap_add: Option<Value>,
    #[serde(default)]
    cap_drop: Option<Value>,
    #[serde(default)]
    privileged: Option<bool>,
    #[serde(default)]
    logging: Option<Value>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawComposeBuild {
    Context(String),
    Detail(RawComposeBuildDetail),
}

impl RawComposeBuild {
    fn resolve_paths(&self, compose_dir: &Path) -> Result<(PathBuf, PathBuf), Error> {
        let (context, dockerfile) = match self {
            Self::Context(context) => (PathBuf::from(context), PathBuf::from("Dockerfile")),
            Self::Detail(detail) => (
                PathBuf::from(detail.context.clone().unwrap_or_else(|| ".".to_owned())),
                PathBuf::from(
                    detail
                        .dockerfile
                        .clone()
                        .unwrap_or_else(|| "Dockerfile".to_owned()),
                ),
            ),
        };

        let context_path = compose_dir.join(context);
        let dockerfile_path = if dockerfile.is_absolute() {
            dockerfile
        } else {
            context_path.join(dockerfile)
        };
        Ok((context_path, dockerfile_path))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeBuildDetail {
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    dockerfile: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawComposeStringMap {
    List(Vec<String>),
    Map(BTreeMap<String, Option<Value>>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawComposeEnvFile {
    Single(String),
    List(Vec<RawComposeEnvFileEntry>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawComposeEnvFileEntry {
    Path(String),
    Detail(RawComposeEnvFileDetail),
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeEnvFileDetail {
    path: String,
    #[serde(default)]
    required: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawComposeDependsOn {
    List(Vec<String>),
    Map(BTreeMap<String, RawComposeDependencyDetail>),
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeDependencyDetail {
    #[serde(default)]
    condition: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeDeploy {
    #[serde(default)]
    resources: Option<RawComposeResources>,
    #[serde(default)]
    replicas: Option<Value>,
    #[serde(default)]
    placement: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeResources {
    #[serde(default)]
    limits: Option<RawComposeResourceLimits>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeResourceLimits {
    #[serde(default)]
    cpus: Option<String>,
    #[serde(default)]
    memory: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeHealthcheck {
    #[serde(default)]
    test: Option<ComposeCommandPlan>,
    #[serde(default)]
    interval: Option<String>,
    #[serde(default)]
    timeout: Option<String>,
    #[serde(default)]
    retries: Option<u32>,
    #[serde(default)]
    start_period: Option<String>,
    #[serde(default)]
    disable: Option<bool>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawComposeVolumeMount {
    Short(String),
    Long(RawComposeVolumeMountDetail),
}

#[derive(Debug, Clone, Deserialize)]
struct RawComposeVolumeMountDetail {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    source: Option<String>,
    target: String,
    #[serde(default)]
    read_only: Option<bool>,
}

fn resolve_ports(
    service_name: &str,
    ports: Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ComposePortBindingPlan>, Error> {
    ports
        .into_iter()
        .enumerate()
        .map(|(index, port)| parse_port_binding(service_name, &port, index, warnings))
        .collect()
}

fn parse_port_binding(
    service_name: &str,
    raw: &str,
    index: usize,
    warnings: &mut Vec<String>,
) -> Result<ComposePortBindingPlan, Error> {
    let (port_part, protocol) = match raw.rsplit_once('/') {
        Some((port_part, "tcp")) => (port_part, PublishedEndpointProtocol::Tcp),
        Some((_, other)) => {
            return Err(Error::InvalidInput(format!(
                "services.{service_name}.ports: unsupported protocol {other:?}; neovex currently supports tcp only"
            )));
        }
        None => (raw, PublishedEndpointProtocol::Tcp),
    };

    let segments = port_part.split(':').collect::<Vec<_>>();
    let (host_address, host_port, guest_port) = match segments.as_slice() {
        [host_port, guest_port] => (
            "127.0.0.1".parse::<IpAddr>().expect("localhost ip parses"),
            parse_u16_field(
                &format!("services.{service_name}.ports host port"),
                host_port,
            )?,
            parse_u16_field(
                &format!("services.{service_name}.ports guest port"),
                guest_port,
            )?,
        ),
        [host_address, host_port, guest_port] => (
            host_address.parse::<IpAddr>().map_err(|error| {
                Error::InvalidInput(format!(
                    "services.{service_name}.ports: invalid host address {host_address:?}: {error}"
                ))
            })?,
            parse_u16_field(
                &format!("services.{service_name}.ports host port"),
                host_port,
            )?,
            parse_u16_field(
                &format!("services.{service_name}.ports guest port"),
                guest_port,
            )?,
        ),
        _ => {
            return Err(Error::InvalidInput(format!(
                "services.{service_name}.ports: unsupported port mapping {raw:?}; expected HOST:CONTAINER or HOST_IP:HOST:CONTAINER"
            )));
        }
    };

    if index > 0 {
        warnings.push(format!(
            "ports[{index}]: additional exposed port {guest_port} will be available through ctx.services.<name>.endpoints"
        ));
    }

    Ok(ComposePortBindingPlan {
        name: if index == 0 {
            "default".to_owned()
        } else {
            format!("tcp-{guest_port}")
        },
        protocol,
        host_address,
        host_port,
        guest_port,
    })
}

fn parse_u16_field(label: &str, value: &str) -> Result<u16, Error> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|error| Error::InvalidInput(format!("{label} {value:?} is invalid: {error}")))
}

fn parse_cpu_count(
    service_name: &str,
    value: &str,
    warnings: &mut Vec<String>,
) -> Result<u8, Error> {
    let parsed = value.trim().parse::<f64>().map_err(|error| {
        Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.cpus: invalid value {value:?}: {error}"
        ))
    })?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.cpus: expected a positive CPU value, got {value:?}"
        )));
    }

    let rounded = parsed.ceil();
    if rounded > u8::MAX as f64 {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.cpus: value {value:?} exceeds the current krun vCPU limit of {}",
            u8::MAX
        )));
    }

    if (rounded - parsed).abs() > f64::EPSILON {
        warnings.push(format!(
            "deploy.resources.limits.cpus: rounded {value} up to {} vCPU(s) because the krun backend currently requires whole guest CPU counts",
            rounded as u8
        ));
    }

    Ok(rounded as u8)
}

fn parse_memory_limit(service_name: &str, value: &str) -> Result<u64, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: expected a byte value like 256M or 1G"
        )));
    }

    let digits_len = trimmed
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .count();
    if digits_len == 0 {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: invalid value {value:?}. Expected format: 256M, 1G, etc."
        )));
    }

    let amount = trimmed[..digits_len].parse::<u64>().map_err(|error| {
        Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: invalid numeric value {value:?}: {error}"
        ))
    })?;
    let unit = trimmed[digits_len..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        "t" | "tb" => 1024_u64.pow(4),
        other => {
            return Err(Error::InvalidInput(format!(
                "services.{service_name}.deploy.resources.limits.memory: unsupported unit {other:?}. Expected format: 256M, 1G, etc."
            )));
        }
    };
    amount.checked_mul(multiplier).ok_or_else(|| {
        Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: value {value:?} overflowed u64 bytes"
        ))
    })
}

fn compose_lifecycle_spec(
    restart: &ComposeRestartPlan,
    stop_grace_period: Option<&str>,
    stop_grace_period_label: &str,
) -> Result<SandboxLifecycleSpec, Error> {
    let mut lifecycle = SandboxLifecycleSpec::default().with_restart_policy(restart.policy);
    if let Some(stop_grace_period) = stop_grace_period {
        lifecycle = lifecycle.with_stop_timeout(parse_compose_duration(
            stop_grace_period_label,
            stop_grace_period,
        )?);
    }
    Ok(lifecycle)
}

fn parse_compose_duration(label: &str, value: &str) -> Result<Duration, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput(format!(
            "{label}: expected a duration like 30s or 1m30s"
        )));
    }

    let mut total_nanos = 0_f64;
    let mut offset = 0;
    while offset < trimmed.len() {
        let remaining = &trimmed[offset..];
        let skipped = remaining
            .chars()
            .take_while(|character| character.is_ascii_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        offset += skipped;
        if offset >= trimmed.len() {
            break;
        }

        let number_start = offset;
        let mut seen_digit = false;
        let mut seen_decimal = false;
        while offset < trimmed.len() {
            let character = trimmed[offset..]
                .chars()
                .next()
                .expect("slice should contain a character");
            if character.is_ascii_digit() {
                seen_digit = true;
                offset += character.len_utf8();
                continue;
            }
            if character == '.' && !seen_decimal {
                seen_decimal = true;
                offset += character.len_utf8();
                continue;
            }
            break;
        }
        if !seen_digit {
            return Err(Error::InvalidInput(format!(
                "{label}: invalid duration {value:?}. Expected a duration like 30s or 1m30s"
            )));
        }

        let amount = trimmed[number_start..offset]
            .parse::<f64>()
            .map_err(|error| {
                Error::InvalidInput(format!("{label}: invalid duration {value:?}: {error}"))
            })?;
        if !amount.is_finite() || amount < 0.0 {
            return Err(Error::InvalidInput(format!(
                "{label}: invalid duration {value:?}. Expected a positive duration"
            )));
        }

        let remaining = &trimmed[offset..];
        let (unit, unit_nanos) = ["ns", "us", "µs", "μs", "ms", "s", "m", "h"]
            .into_iter()
            .find_map(|unit| {
                remaining
                    .strip_prefix(unit)
                    .map(|_| (unit, duration_unit_nanos(unit)))
            })
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "{label}: invalid duration {value:?}. Supported units are ns, us, ms, s, m, h"
                ))
            })?;
        offset += unit.len();
        total_nanos += amount * unit_nanos;
    }

    if !total_nanos.is_finite() || total_nanos < 0.0 || total_nanos > u64::MAX as f64 {
        return Err(Error::InvalidInput(format!(
            "{label}: duration {value:?} exceeds the supported range"
        )));
    }

    Ok(Duration::from_nanos(total_nanos.round() as u64))
}

fn duration_unit_nanos(unit: &str) -> f64 {
    match unit {
        "ns" => 1.0,
        "us" | "µs" | "μs" => 1_000.0,
        "ms" => 1_000_000.0,
        "s" => 1_000_000_000.0,
        "m" => 60.0 * 1_000_000_000.0,
        "h" => 60.0 * 60.0 * 1_000_000_000.0,
        _ => unreachable!("unsupported duration unit {unit}"),
    }
}

fn resolve_depends_on(
    depends_on: Option<RawComposeDependsOn>,
) -> Result<BTreeMap<String, ComposeDependencyCondition>, Error> {
    let Some(depends_on) = depends_on else {
        return Ok(BTreeMap::new());
    };

    match depends_on {
        RawComposeDependsOn::List(list) => Ok(list
            .into_iter()
            .map(|name| (name, ComposeDependencyCondition::ServiceStarted))
            .collect()),
        RawComposeDependsOn::Map(map) => map
            .into_iter()
            .map(|(name, detail)| {
                let condition = match detail.condition.as_deref().unwrap_or("service_started") {
                    "service_started" => ComposeDependencyCondition::ServiceStarted,
                    "service_healthy" => ComposeDependencyCondition::ServiceHealthy,
                    other => {
                        return Err(Error::InvalidInput(format!(
                            "depends_on.{name}.condition: unsupported condition {other:?}; expected service_started or service_healthy"
                        )));
                    }
                };
                Ok((name, condition))
            })
            .collect(),
    }
}

fn resolve_volume_mounts(volumes: Vec<RawComposeVolumeMount>) -> Vec<ComposeVolumeMountPlan> {
    volumes
        .into_iter()
        .filter_map(|volume| match volume {
            RawComposeVolumeMount::Short(raw) => parse_short_volume_mount(&raw),
            RawComposeVolumeMount::Long(detail) => Some(ComposeVolumeMountPlan {
                source: detail.source,
                target: detail.target,
                kind: detail.kind.unwrap_or_else(|| "volume".to_owned()),
                read_only: detail.read_only.unwrap_or(false),
            }),
        })
        .collect()
}

fn parse_short_volume_mount(raw: &str) -> Option<ComposeVolumeMountPlan> {
    let parts = raw.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [target] => Some(ComposeVolumeMountPlan {
            source: None,
            target: (*target).to_owned(),
            kind: "anonymous".to_owned(),
            read_only: false,
        }),
        [source, target] => Some(ComposeVolumeMountPlan {
            source: Some((*source).to_owned()),
            target: (*target).to_owned(),
            kind: classify_volume_source(source),
            read_only: false,
        }),
        [source, target, mode] => Some(ComposeVolumeMountPlan {
            source: Some((*source).to_owned()),
            target: (*target).to_owned(),
            kind: classify_volume_source(source),
            read_only: mode.split(',').any(|flag| flag.trim() == "ro"),
        }),
        _ => None,
    }
}

fn classify_volume_source(source: &str) -> String {
    if source.starts_with('/')
        || source.starts_with("./")
        || source.starts_with("../")
        || source.starts_with('~')
    {
        "bind".to_owned()
    } else {
        "volume".to_owned()
    }
}

fn warnings_for_known_ignored_service_fields(service: &RawComposeService) -> Vec<String> {
    let mut warnings = Vec::new();
    if service.networks.is_some() {
        warnings.push("networks: ignored (neovex uses TSI networking)".to_owned());
    }
    if service.configs.is_some() {
        warnings.push("configs: ignored (not yet supported by neovex service config)".to_owned());
    }
    if service.secrets.is_some() {
        warnings.push("secrets: ignored (not yet supported by neovex service config)".to_owned());
    }
    if service.cap_add.is_some() {
        warnings.push("cap_add: ignored (VM isolation replaces container capabilities)".to_owned());
    }
    if service.cap_drop.is_some() {
        warnings
            .push("cap_drop: ignored (VM isolation replaces container capabilities)".to_owned());
    }
    if service.privileged.is_some() {
        warnings.push(
            "privileged: ignored (VM isolation replaces privileged container mode)".to_owned(),
        );
    }
    if service.logging.is_some() {
        warnings.push(
            "logging: ignored (conmon-backed logging is the current source of truth)".to_owned(),
        );
    }
    warnings
}

fn warnings_for_unknown_fields(prefix: &str, fields: BTreeMap<String, Value>) -> Vec<String> {
    fields
        .into_keys()
        .map(|field| {
            if field.starts_with("x-") {
                format!("{prefix}.{field}: ignored extension field")
            } else {
                format!("{prefix}.{field}: ignored unknown field")
            }
        })
        .collect()
}

fn parse_environment_map(
    environment: Option<RawComposeStringMap>,
    field_label: &str,
) -> Result<BTreeMap<String, String>, Error> {
    match environment {
        None => Ok(BTreeMap::new()),
        Some(RawComposeStringMap::List(entries)) => entries
            .into_iter()
            .filter_map(|entry| parse_inline_key_value_entry(&entry))
            .map(|(key, value)| Ok((key, value)))
            .collect(),
        Some(RawComposeStringMap::Map(entries)) => {
            let mut resolved = BTreeMap::new();
            for (key, value) in entries {
                if let Some(value) = scalar_value_to_string(field_label, value)? {
                    resolved.insert(key, value);
                }
            }
            Ok(resolved)
        }
    }
}

fn parse_string_map(
    values: Option<RawComposeStringMap>,
    field_label: &str,
) -> Result<BTreeMap<String, String>, Error> {
    parse_environment_map(values, field_label)
}

fn parse_inline_key_value_entry(entry: &str) -> Option<(String, String)> {
    let (key, value) = entry.split_once('=')?;
    Some((key.trim().to_owned(), value.trim().to_owned()))
}

fn scalar_value_to_string(
    field_label: &str,
    value: Option<Value>,
) -> Result<Option<String>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value {
        Value::Null => Ok(None),
        Value::Bool(value) => Ok(Some(value.to_string())),
        Value::Number(value) => Ok(Some(value.to_string())),
        Value::String(value) => Ok(Some(value)),
        other => Err(Error::InvalidInput(format!(
            "{field_label}: expected a scalar string/number/bool value, got {other:?}"
        ))),
    }
}

fn load_env_files(
    compose_dir: &Path,
    env_file: Option<RawComposeEnvFile>,
) -> Result<BTreeMap<String, String>, Error> {
    let mut environment = BTreeMap::new();
    let entries = match env_file {
        None => return Ok(environment),
        Some(RawComposeEnvFile::Single(path)) => vec![RawComposeEnvFileEntry::Path(path)],
        Some(RawComposeEnvFile::List(entries)) => entries,
    };

    for entry in entries {
        let detail = match entry {
            RawComposeEnvFileEntry::Path(path) => RawComposeEnvFileDetail {
                path,
                required: None,
            },
            RawComposeEnvFileEntry::Detail(detail) => detail,
        };
        let path = compose_dir.join(&detail.path);
        let bytes = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && detail.required == Some(false) =>
            {
                continue;
            }
            Err(error) => {
                return Err(Error::InvalidInput(format!(
                    "failed to read env_file {}: {error}",
                    path.display()
                )));
            }
        };
        for line in bytes.lines() {
            if let Some((key, value)) = parse_env_file_line(line) {
                environment.insert(key, value);
            }
        }
    }

    Ok(environment)
}

fn parse_env_file_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let (key, value) = trimmed
        .split_once('=')
        .or_else(|| trimmed.split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))?;

    if key.is_empty() {
        return None;
    }

    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
        .to_owned();
    Some((key.to_owned(), value))
}

fn default_project_name(path: &Path) -> &str {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .or_else(|| path.file_stem().and_then(|name| name.to_str()))
        .unwrap_or("neovex")
}

fn sanitize_project_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn default_build_image_name(project_name: &str, service_name: &str) -> String {
    format!(
        "neovex-{}-{}",
        sanitize_project_name(project_name),
        sanitize_project_name(service_name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_compose_fixture(tempdir: &tempfile::TempDir, name: &str, contents: &str) -> PathBuf {
        let path = tempdir.path().join(name);
        fs::write(&path, contents).expect("fixture file should write");
        path
    }

    #[test]
    fn compose_project_resolves_image_and_build_services() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        write_compose_fixture(
            &tempdir,
            "db.env",
            "FROM_ENV=from-file\nOVERRIDE_ME=from-env-file\n",
        );
        let compose = write_compose_fixture(
            &tempdir,
            "compose.yaml",
            r#"
name: Demo App
services:
  db:
    image: postgres:16
    env_file:
      - ./db.env
    environment:
      POSTGRES_PASSWORD: secret
      OVERRIDE_ME: inline
    ports:
      - "5432:5432"
      - "127.0.0.1:15433:5433/tcp"
    deploy:
      resources:
        limits:
          cpus: "1.0"
          memory: 256M
    restart: on-failure:3
    depends_on:
      cache:
        condition: service_healthy
    healthcheck:
      test:
        - CMD
        - pg_isready
        - -U
        - postgres
      interval: 10s
    stop_grace_period: 30s
    labels:
      app.role: database
    x_neovex:
      snapshot: true
  api:
    build:
      context: .
      dockerfile: Dockerfile.api
    command: ["./server"]
    entrypoint: ["/bin/sh", "-lc"]
    working_dir: /workspace
    user: "1000:1000"
    deploy:
      resources:
        limits:
          cpus: "0.5"
          memory: 128M
volumes:
  pgdata: {}
"#,
        );

        let project = ComposeProjectPlan::load(&compose).expect("compose file should resolve");
        assert_eq!(project.project_name, "demo-app");
        assert_eq!(project.volumes, vec!["pgdata".to_owned()]);

        let db = project.services.get("db").expect("db service should exist");
        assert_eq!(db.backend, SandboxBackendKind::Krun);
        assert_eq!(
            db.source,
            ComposeLaunchPlan::Image {
                image_reference: "postgres:16".to_owned(),
            }
        );
        assert_eq!(
            db.process.environment.get("FROM_ENV"),
            Some(&"from-file".to_owned())
        );
        assert_eq!(
            db.process.environment.get("OVERRIDE_ME"),
            Some(&"inline".to_owned())
        );
        assert_eq!(db.ports.len(), 2);
        assert_eq!(db.ports[0].name, "default");
        assert_eq!(db.ports[0].host_port, 5432);
        assert_eq!(db.ports[0].guest_port, 5432);
        assert_eq!(db.resources.cpu_count, Some(1));
        assert_eq!(db.resources.memory_limit_bytes, Some(256 * 1024 * 1024));
        assert_eq!(
            db.restart.policy,
            SandboxRestartPolicy::OnFailure { max_restarts: 3 }
        );
        assert_eq!(
            db.depends_on.get("cache"),
            Some(&ComposeDependencyCondition::ServiceHealthy)
        );
        assert_eq!(
            db.healthcheck
                .as_ref()
                .and_then(|healthcheck| healthcheck.interval.as_deref()),
            Some("10s")
        );
        assert_eq!(db.stop_grace_period.as_deref(), Some("30s"));
        assert_eq!(db.labels.get("app.role"), Some(&"database".to_owned()));
        assert_eq!(
            db.x_neovex
                .as_ref()
                .and_then(|extensions| extensions.snapshot),
            Some(true)
        );

        let api = project
            .services
            .get("api")
            .expect("api service should exist");
        assert_eq!(
            api.source,
            ComposeLaunchPlan::Build {
                image_name: "neovex-demo-app-api".to_owned(),
                dockerfile_path: tempdir.path().join("Dockerfile.api"),
                context_path: tempdir.path().to_path_buf(),
            }
        );
        assert_eq!(api.process.user.as_deref(), Some("1000:1000"));
        assert_eq!(
            api.process.working_dir.as_ref(),
            Some(&PathBuf::from("/workspace"))
        );
        assert_eq!(
            api.process.command.as_ref(),
            Some(&ComposeCommandPlan::List(vec!["./server".to_owned()]))
        );
        assert_eq!(api.resources.cpu_count, Some(1));
        assert!(
            api.warnings
                .iter()
                .any(|warning| warning.contains("rounded 0.5 up to 1 vCPU")),
            "expected fractional CPU rounding warning, got {:?}",
            api.warnings
        );
    }

    #[test]
    fn compose_project_reports_ignored_fields() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose_fixture(
            &tempdir,
            "compose.yaml",
            r#"
services:
  db:
    image: postgres:16
    networks:
      - default
    privileged: true
    logging:
      driver: json-file
"#,
        );

        let project = ComposeProjectPlan::load(&compose).expect("compose file should resolve");
        let db = project.services.get("db").expect("db service should exist");
        assert!(
            db.warnings
                .iter()
                .any(|warning| warning.contains("networks")),
            "expected network warning, got {:?}",
            db.warnings
        );
        assert!(
            db.warnings
                .iter()
                .any(|warning| warning.contains("privileged")),
            "expected privileged warning, got {:?}",
            db.warnings
        );
        assert!(
            db.warnings
                .iter()
                .any(|warning| warning.contains("logging")),
            "expected logging warning, got {:?}",
            db.warnings
        );
    }

    #[test]
    fn compose_project_rejects_invalid_memory_values() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose_fixture(
            &tempdir,
            "compose.yaml",
            r#"
services:
  db:
    image: postgres:16
    deploy:
      resources:
        limits:
          memory: abc
"#,
        );

        let error = ComposeProjectPlan::load(&compose).expect_err("invalid memory should fail");
        assert!(
            error
                .to_string()
                .contains("Expected format: 256M, 1G, etc."),
            "expected actionable memory error, got: {error}"
        );
    }

    #[test]
    fn render_compose_project_services_lists_names_and_warnings() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose_fixture(
            &tempdir,
            "compose.yaml",
            r#"
services:
  cache:
    image: redis:7
  db:
    image: postgres:16
    networks:
      - default
"#,
        );

        let rendered =
            render_compose_project(&compose, true).expect("service listing should render");
        assert_eq!(rendered.stdout, "cache\ndb\n");
        assert!(
            rendered
                .warnings
                .iter()
                .any(|warning| warning.contains("services.db")),
            "expected service warning to surface in list mode, got {:?}",
            rendered.warnings
        );
    }

    #[test]
    fn compose_process_plan_lowers_to_image_process_overrides() {
        let process = ComposeProcessPlan {
            entrypoint: Some(ComposeCommandPlan::List(vec![
                "/bin/sh".to_owned(),
                "-lc".to_owned(),
            ])),
            command: Some(ComposeCommandPlan::String(
                "exec ./server --port 8080".to_owned(),
            )),
            environment: BTreeMap::from([
                ("APP_ENV".to_owned(), "dev".to_owned()),
                ("LOG_LEVEL".to_owned(), "debug".to_owned()),
            ]),
            working_dir: Some(PathBuf::from("/workspace")),
            user: Some("1000:1000".to_owned()),
        };

        let overrides = process
            .to_image_process_overrides()
            .expect("compose process should lower");

        assert_eq!(
            overrides.entrypoint,
            Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()])
        );
        assert_eq!(
            overrides.cmd,
            Some(vec![
                "exec".to_owned(),
                "./server".to_owned(),
                "--port".to_owned(),
                "8080".to_owned()
            ])
        );
        assert_eq!(
            overrides.env,
            vec!["APP_ENV=dev".to_owned(), "LOG_LEVEL=debug".to_owned(),]
        );
        assert_eq!(overrides.cwd, Some(PathBuf::from("/workspace")));
        assert_eq!(overrides.user.as_deref(), Some("1000:1000"));
    }

    #[test]
    fn compose_process_plan_rejects_empty_override_commands() {
        let process = ComposeProcessPlan {
            entrypoint: None,
            command: Some(ComposeCommandPlan::List(Vec::new())),
            environment: BTreeMap::new(),
            working_dir: None,
            user: None,
        };

        let error = process
            .to_image_process_overrides()
            .expect_err("empty command override should be rejected");
        assert!(
            error
                .to_string()
                .contains("empty command/entrypoint overrides"),
            "expected actionable empty override error, got: {error}"
        );
    }

    #[test]
    fn compose_service_plan_lowers_stop_grace_period_into_sandbox_lifecycle() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose_fixture(
            &tempdir,
            "compose.yaml",
            r#"
services:
  db:
    image: postgres:16
    restart: on-failure:3
    stop_grace_period: 1m30s
"#,
        );

        let project = ComposeProjectPlan::load(&compose).expect("compose file should resolve");
        let service = project.services.get("db").expect("db service should exist");
        let lifecycle = compose_lifecycle_spec(
            &service.restart,
            service.stop_grace_period.as_deref(),
            "services.db.stop_grace_period",
        )
        .expect("compose lifecycle should lower");

        assert_eq!(
            lifecycle.restart_policy,
            SandboxRestartPolicy::OnFailure { max_restarts: 3 }
        );
        assert_eq!(lifecycle.stop_timeout, Some(Duration::from_secs(90)));
    }

    #[test]
    fn compose_project_rejects_invalid_stop_grace_period() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose_fixture(
            &tempdir,
            "compose.yaml",
            r#"
services:
  db:
    image: postgres:16
    stop_grace_period: later
"#,
        );

        let error = ComposeProjectPlan::load(&compose).expect_err("invalid stop grace should fail");
        assert!(
            error.to_string().contains("services.db.stop_grace_period"),
            "expected field-scoped stop_grace_period error, got: {error}"
        );
    }

    #[test]
    fn compose_project_lowers_into_sandbox_service_catalog() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose_fixture(
            &tempdir,
            "compose.yaml",
            r#"
name: Demo App
services:
  db:
    image: postgres:16
    ports:
      - "5432:5432"
    deploy:
      resources:
        limits:
          cpus: "1.0"
          memory: 256M
    restart: on-failure:3
    stop_grace_period: 30s
  api:
    build:
      context: .
      dockerfile: Dockerfile.api
    command: ["./server"]
    entrypoint: ["/bin/sh", "-lc"]
    working_dir: /workspace
    user: "1000:1000"
"#,
        );
        std::fs::write(tempdir.path().join("Dockerfile.api"), "FROM scratch\n")
            .expect("dockerfile fixture should be writable");

        let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
        let catalog = ComposeProjectPlan::load(&compose)
            .expect("compose file should resolve")
            .into_service_catalog()
            .expect("compose project should lower into a service catalog");

        assert_eq!(catalog.project.project_name, "demo-app");

        let db = catalog
            .sandbox_service_for_tenant(&tenant_id, "db")
            .expect("db launch should exist");
        match db {
            SandboxServiceLaunch::Image(launch) => {
                assert_eq!(launch.image_reference, "postgres:16");
                assert_eq!(launch.spec.tenant_id, tenant_id);
                assert_eq!(launch.spec.name, "db");
                assert_eq!(launch.spec.resources.cpu_count, Some(1));
                assert_eq!(
                    launch.spec.resources.memory_limit_bytes,
                    Some(256 * 1024 * 1024)
                );
                assert_eq!(
                    launch.spec.lifecycle.restart_policy,
                    SandboxRestartPolicy::OnFailure { max_restarts: 3 }
                );
                assert_eq!(
                    launch.spec.lifecycle.stop_timeout,
                    Some(Duration::from_secs(30))
                );
                assert_eq!(launch.spec.port_bindings.len(), 1);
                assert_eq!(launch.spec.port_bindings[0].host_port, 5432);
                assert_eq!(launch.spec.port_bindings[0].guest_port, 5432);
            }
            SandboxServiceLaunch::Build(_) => panic!("db should lower as an image-backed launch"),
        }

        let api = catalog
            .sandbox_service_for_tenant(&tenant_id, "api")
            .expect("api launch should exist");
        match api {
            SandboxServiceLaunch::Build(launch) => {
                assert_eq!(launch.image_name, "neovex-demo-app-api");
                assert_eq!(
                    launch.dockerfile_path,
                    tempdir.path().join("Dockerfile.api")
                );
                assert_eq!(launch.context_path, tempdir.path());
                assert_eq!(
                    launch.process_overrides.entrypoint,
                    Some(vec!["/bin/sh".to_owned(), "-lc".to_owned()])
                );
                assert_eq!(
                    launch.process_overrides.cmd,
                    Some(vec!["./server".to_owned()])
                );
                assert_eq!(
                    launch.process_overrides.cwd,
                    Some(PathBuf::from("/workspace"))
                );
                assert_eq!(launch.process_overrides.user.as_deref(), Some("1000:1000"));
            }
            SandboxServiceLaunch::Image(_) => panic!("api should lower as a build-backed launch"),
        }

        let other_tenant = TenantId::new("other").expect("tenant id should be valid");
        let other_db = catalog
            .sandbox_service_for_tenant(&other_tenant, "db")
            .expect("catalog should lower the same service plan for another tenant");
        match other_db {
            SandboxServiceLaunch::Image(launch) => {
                assert_eq!(launch.spec.tenant_id, other_tenant);
                assert_eq!(launch.spec.name, "db");
            }
            SandboxServiceLaunch::Build(_) => panic!("db should stay image-backed across tenants"),
        }
    }
}
