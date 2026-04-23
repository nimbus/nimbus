use super::parse::*;
use super::raw::*;
use super::warnings::*;
use super::*;
use crate::compose::discovery::ResolvedComposeSelection;

impl ComposeProjectPlan {
    #[cfg(test)]
    pub(crate) fn load(path: &Path) -> Result<Self, Error> {
        Self::load_selection(&ResolvedComposeSelection::explicit(path.to_path_buf()))
    }

    pub(crate) fn load_selection(selection: &ResolvedComposeSelection) -> Result<Self, Error> {
        if selection.files.is_empty() {
            return Err(Error::InvalidInput(
                "resolved compose selection did not include any files".to_owned(),
            ));
        }

        let mut documents = selection.files.iter();
        let primary_file = documents
            .next()
            .expect("selection should include a primary compose file");
        let mut raw = read_raw_compose_document(primary_file)?;
        for path in documents {
            raw.merge_from(read_raw_compose_document(path)?);
        }
        Self::from_raw(selection.primary_file(), raw)
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
                "{}: top-level configs: ignored (not yet supported by neovex compose config)",
                path.display()
            ));
        }
        if !raw.secrets.is_empty() {
            warnings.push(format!(
                "{}: top-level secrets: ignored (not yet supported by neovex compose config)",
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
                "failed to render resolved compose config from {}: {error}",
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

fn read_raw_compose_document(path: &Path) -> Result<RawComposeDocument, Error> {
    let bytes = fs::read(path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read compose file {}: {error}",
            path.display()
        ))
    })?;
    serde_yaml::from_slice(&bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse compose file {} as YAML: {error}",
            path.display()
        ))
    })
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
        let backend = raw
            .x_neovex
            .as_ref()
            .and_then(|extensions| extensions.backend)
            .unwrap_or(SandboxBackendKind::Krun);

        Ok(Self {
            backend,
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
impl ComposePortBindingPlan {
    fn into_binding(self) -> SandboxPortBinding {
        SandboxPortBinding::new(self.name, self.protocol, self.host_port, self.guest_port)
            .with_host_address(self.host_address)
    }
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
