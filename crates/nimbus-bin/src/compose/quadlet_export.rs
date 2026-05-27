use std::collections::BTreeSet;
use std::fs;
use std::net::IpAddr;
use std::path::Path;

use nimbus::{Error, PublishedEndpointProtocol, Result, SandboxRestartPolicy};
use sha2::{Digest, Sha256};

use crate::cli_ux;
use crate::compose::discovery::ResolvedComposeSelection;

use super::file::{
    ComposeCommandPlan, ComposeHealthcheckPlan, ComposeLaunchPlan, ComposePortBindingPlan,
    ComposeProjectPlan, ComposeRestartPlan, ComposeServicePlan, ComposeVolumeMountPlan,
};
use super::{ComposeExportQuadletCommand, ComposeQuadletExportMode};

const CONTAINER_TEMPLATE_VERSION: &str = "compose-quadlet-container/v1";
const POD_TEMPLATE_VERSION: &str = "compose-quadlet-pod/v1";
const KUBE_TEMPLATE_VERSION: &str = "compose-quadlet-kube/v1";
const KUBE_YAML_TEMPLATE_VERSION: &str = "compose-quadlet-kube-yaml/v1";

pub(super) fn run_compose_export_quadlet(command: ComposeExportQuadletCommand) -> Result<()> {
    let selection = super::resolve_required_compose_selection(command.file.as_slice())?;
    let rendered = render_quadlet_export_for_selection(&selection, &command)?;
    emit_warnings(&rendered.warnings)?;
    if let Some(output_dir) = &command.output_dir {
        write_artifacts(output_dir, &rendered.artifacts, command.overwrite)?;
        let mut summary = String::new();
        for artifact in &rendered.artifacts {
            summary.push_str(&format!(
                "wrote: {}\n",
                output_dir.join(&artifact.filename).display()
            ));
        }
        cli_ux::write_stdout(&summary)
            .map_err(|error| Error::Internal(format!("failed to write export output: {error}")))?;
    } else {
        cli_ux::write_stdout(&render_artifacts_stdout(&rendered.artifacts))
            .map_err(|error| Error::Internal(format!("failed to write export output: {error}")))?;
    }
    Ok(())
}

fn render_quadlet_export_for_selection(
    selection: &ResolvedComposeSelection,
    command: &ComposeExportQuadletCommand,
) -> Result<QuadletExportRender> {
    let project = ComposeProjectPlan::load_selection(selection)?;
    let options = QuadletExportOptions {
        mode: command.mode,
        podman_version: command.podman_version.clone(),
        selected_services: command.service.clone(),
        strict: command.strict,
    };
    QuadletExporter::new(project, options).render()
}

#[derive(Debug, Clone)]
struct QuadletExportOptions {
    mode: ComposeQuadletExportMode,
    podman_version: Option<String>,
    selected_services: Vec<String>,
    strict: bool,
}

#[derive(Debug, Clone)]
struct QuadletExportRender {
    artifacts: Vec<QuadletArtifact>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuadletArtifact {
    filename: String,
    contents: String,
}

struct QuadletExporter {
    project: ComposeProjectPlan,
    options: QuadletExportOptions,
    warnings: Vec<String>,
}

impl QuadletExporter {
    fn new(project: ComposeProjectPlan, options: QuadletExportOptions) -> Self {
        Self {
            warnings: project.all_warnings(),
            project,
            options,
        }
    }

    fn render(mut self) -> Result<QuadletExportRender> {
        self.validate_podman_version()?;
        let service_names = self.selected_service_names()?;
        let artifacts = match self.options.mode {
            ComposeQuadletExportMode::Containers => self.render_container_mode(&service_names)?,
            ComposeQuadletExportMode::Pod => self.render_pod_mode(&service_names)?,
            ComposeQuadletExportMode::Kube => self.render_kube_mode(&service_names)?,
        };
        if self.options.strict && !self.warnings.is_empty() {
            return Err(Error::InvalidInput(format!(
                "quadlet export has warnings in --strict mode:\n- {}",
                self.warnings.join("\n- ")
            )));
        }
        Ok(QuadletExportRender {
            artifacts,
            warnings: self.warnings,
        })
    }

    fn validate_podman_version(&self) -> Result<()> {
        let Some(version) = self.options.podman_version.as_ref() else {
            return Ok(());
        };
        safe_unit_value(version, "--podman-version")?;
        Ok(())
    }

    fn selected_service_names(&mut self) -> Result<Vec<String>> {
        if self.options.selected_services.is_empty() {
            return Ok(self.project.services.keys().cloned().collect());
        }
        let mut seen = BTreeSet::new();
        let mut selected = Vec::new();
        for service_name in &self.options.selected_services {
            if !self.project.services.contains_key(service_name) {
                return Err(Error::InvalidInput(format!(
                    "service {service_name} is not declared in compose project {}",
                    self.project.project_name
                )));
            }
            if seen.insert(service_name.clone()) {
                selected.push(service_name.clone());
            }
        }
        Ok(selected)
    }

    fn render_container_mode(&mut self, service_names: &[String]) -> Result<Vec<QuadletArtifact>> {
        let mut artifacts = Vec::new();
        for service_name in service_names {
            let Some(artifact) = self.render_container_artifact(service_name, None, false)? else {
                continue;
            };
            artifacts.push(artifact);
        }
        Ok(artifacts)
    }

    fn render_pod_mode(&mut self, service_names: &[String]) -> Result<Vec<QuadletArtifact>> {
        let pod_name = safe_resource_name(&self.project.project_name);
        let pod_filename = format!("{pod_name}.pod");
        let mut artifacts = vec![self.render_pod_artifact(&pod_filename, service_names)?];
        for service_name in service_names {
            let Some(artifact) =
                self.render_container_artifact(service_name, Some(&pod_filename), true)?
            else {
                continue;
            };
            artifacts.push(artifact);
        }
        Ok(artifacts)
    }

    fn render_kube_mode(&mut self, service_names: &[String]) -> Result<Vec<QuadletArtifact>> {
        let project_name = safe_resource_name(&self.project.project_name);
        let yaml_filename = format!("{project_name}.yaml");
        let kube_filename = format!("{project_name}.kube");
        let yaml = self.render_kube_yaml(&yaml_filename, service_names)?;
        let kube_body = format!(
            "[Unit]\nDescription=Nimbus Compose export for project {project}\n\n[Kube]\nYaml={yaml_filename}\nSetWorkingDirectory=unit\n\n[Service]\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
            project = self.project.project_name,
        );
        Ok(vec![
            QuadletArtifact {
                filename: kube_filename,
                contents: with_provenance(
                    KUBE_TEMPLATE_VERSION,
                    &self.project,
                    None,
                    self.options.mode,
                    self.options.podman_version.as_deref(),
                    &kube_body,
                ),
            },
            QuadletArtifact {
                filename: yaml_filename,
                contents: with_provenance(
                    KUBE_YAML_TEMPLATE_VERSION,
                    &self.project,
                    None,
                    self.options.mode,
                    self.options.podman_version.as_deref(),
                    &yaml,
                ),
            },
        ])
    }

    fn render_container_artifact(
        &mut self,
        service_name: &str,
        pod_filename: Option<&str>,
        start_with_pod: bool,
    ) -> Result<Option<QuadletArtifact>> {
        let Some(service) = self.project.services.get(service_name).cloned() else {
            return Err(Error::InvalidInput(format!(
                "service {service_name} is not declared in compose project {}",
                self.project.project_name
            )));
        };
        let image = match &service.source {
            ComposeLaunchPlan::Image { image_reference } => {
                safe_unit_value(image_reference, &format!("services.{service_name}.image"))?
                    .to_string()
            }
            ComposeLaunchPlan::Build { .. } => {
                self.warn(
                    service_name,
                    "build services are not exported to Quadlet; build and tag the image first",
                );
                return Ok(None);
            }
        };
        let resource_name = self.service_resource_name(service_name);
        let mut body = String::new();
        body.push_str("[Unit]\n");
        body.push_str(&format!(
            "Description=Nimbus Compose service {} from project {}\n",
            service_name, self.project.project_name
        ));
        body.push_str("After=network-online.target\n");
        body.push_str("Wants=network-online.target\n");
        for dependency in service.depends_on.keys() {
            let dependency_unit = format!("{}.service", self.service_resource_name(dependency));
            body.push_str(&format!("After={dependency_unit}\n"));
            body.push_str(&format!("Wants={dependency_unit}\n"));
        }
        body.push('\n');
        body.push_str("[Container]\n");
        body.push_str(&format!("Image={image}\n"));
        body.push_str(&format!("ContainerName={resource_name}\n"));
        if let Some(pod_filename) = pod_filename {
            body.push_str(&format!("Pod={pod_filename}\n"));
        }
        if start_with_pod {
            body.push_str("StartWithPod=true\n");
        }
        self.render_process(service_name, &service, &mut body)?;
        self.render_ports(service_name, &service, pod_filename.is_some(), &mut body)?;
        self.render_volumes(service_name, &service.volumes, &mut body)?;
        self.render_labels(service_name, &service, &mut body)?;
        self.render_resources(service_name, &service, &mut body)?;
        self.render_healthcheck(service_name, service.healthcheck.as_ref(), &mut body)?;
        body.push('\n');
        body.push_str("[Service]\n");
        body.push_str(&format!(
            "Restart={}\n",
            self.restart_policy(service_name, &service.restart)
        ));
        if let Some(stop_timeout) = service.stop_grace_period.as_deref() {
            match parse_duration_seconds(stop_timeout) {
                Some(seconds) => body.push_str(&format!("StopTimeout={seconds}\n")),
                None => self.warn(
                    service_name,
                    &format!("stop_grace_period `{stop_timeout}` was not exported"),
                ),
            }
        }
        body.push_str("\n[Install]\nWantedBy=default.target\n");
        Ok(Some(QuadletArtifact {
            filename: format!("{resource_name}.container"),
            contents: with_provenance(
                CONTAINER_TEMPLATE_VERSION,
                &self.project,
                Some(service_name),
                self.options.mode,
                self.options.podman_version.as_deref(),
                &body,
            ),
        }))
    }

    fn render_pod_artifact(
        &mut self,
        pod_filename: &str,
        service_names: &[String],
    ) -> Result<QuadletArtifact> {
        let pod_name = pod_filename.trim_end_matches(".pod");
        let mut body = String::new();
        body.push_str("[Unit]\n");
        body.push_str(&format!(
            "Description=Nimbus Compose pod for project {}\n\n",
            self.project.project_name
        ));
        body.push_str("[Pod]\n");
        body.push_str(&format!("PodName={pod_name}\n"));
        for service_name in service_names {
            let service = self
                .project
                .services
                .get(service_name)
                .expect("selected service should exist")
                .clone();
            self.render_service_ports_for_pod(service_name, &service, &mut body)?;
        }
        body.push_str("\n[Service]\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n");
        Ok(QuadletArtifact {
            filename: pod_filename.to_string(),
            contents: with_provenance(
                POD_TEMPLATE_VERSION,
                &self.project,
                None,
                self.options.mode,
                self.options.podman_version.as_deref(),
                &body,
            ),
        })
    }

    fn render_kube_yaml(
        &mut self,
        yaml_filename: &str,
        service_names: &[String],
    ) -> Result<String> {
        let mut rendered = String::new();
        rendered.push_str("apiVersion: v1\nkind: Pod\nmetadata:\n");
        rendered.push_str(&format!(
            "  name: {}\n  labels:\n    app.kubernetes.io/managed-by: nimbus\n    app.kubernetes.io/name: {}\nspec:\n  containers:\n",
            yaml_string(&safe_resource_name(&self.project.project_name)),
            yaml_string(&self.project.project_name),
        ));
        let mut restart_policy = None;
        for service_name in service_names {
            let Some(service) = self.project.services.get(service_name).cloned() else {
                continue;
            };
            let image = match &service.source {
                ComposeLaunchPlan::Image { image_reference } => image_reference.clone(),
                ComposeLaunchPlan::Build { .. } => {
                    self.warn(service_name, "build services are not exported to Kubernetes YAML; build and tag the image first");
                    continue;
                }
            };
            let next_restart = kube_restart_policy(service.restart.policy);
            if let Some(existing) = restart_policy {
                if existing != next_restart {
                    self.warn(
                        service_name,
                        "mixed restart policies collapse to the first Kubernetes Pod restartPolicy",
                    );
                }
            } else {
                restart_policy = Some(next_restart);
            }
            rendered.push_str(&format!(
                "    - name: {}\n      image: {}\n",
                yaml_string(&safe_resource_name(service_name)),
                yaml_string(&image)
            ));
            self.render_kube_process(service_name, &service, &mut rendered)?;
            self.render_kube_ports(service_name, &service, &mut rendered);
            self.render_kube_env(service_name, &service, &mut rendered)?;
            if !service.volumes.is_empty() {
                self.warn(
                    service_name,
                    "volumes are not exported in --mode kube; review storage manually",
                );
            }
            if service.healthcheck.is_some() {
                self.warn(
                    service_name,
                    "healthcheck is not exported in --mode kube yet; review probes manually",
                );
            }
        }
        rendered.push_str(&format!(
            "  restartPolicy: {}\n",
            restart_policy.unwrap_or("Never")
        ));
        self.warnings.push(format!(
            "{yaml_filename}: Kubernetes YAML is a review artifact for Quadlet .kube and is not installed by Nimbus"
        ));
        Ok(rendered)
    }

    fn render_process(
        &mut self,
        service_name: &str,
        service: &ComposeServicePlan,
        body: &mut String,
    ) -> Result<()> {
        if let Some(entrypoint) = service.process.entrypoint.as_ref() {
            body.push_str(&format!(
                "Entrypoint={}\n",
                render_command(entrypoint, &format!("services.{service_name}.entrypoint"))?
            ));
        }
        if let Some(command) = service.process.command.as_ref() {
            body.push_str(&format!(
                "Exec={}\n",
                render_command(command, &format!("services.{service_name}.command"))?
            ));
        }
        for (name, value) in &service.process.environment {
            safe_env_name(name, &format!("services.{service_name}.environment"))?;
            let value = safe_unit_value(
                value,
                &format!("services.{service_name}.environment.{name}"),
            )?;
            body.push_str(&format!("Environment={name}={value}\n"));
        }
        if let Some(working_dir) = service.process.working_dir.as_ref() {
            let working_dir_value = working_dir.display().to_string();
            let working_dir = safe_unit_value(
                &working_dir_value,
                &format!("services.{service_name}.working_dir"),
            )?;
            body.push_str(&format!("WorkingDir={working_dir}\n"));
        }
        if let Some(user) = service.process.user.as_ref() {
            let user = safe_unit_value(user, &format!("services.{service_name}.user"))?;
            body.push_str(&format!("User={user}\n"));
        }
        Ok(())
    }

    fn render_ports(
        &mut self,
        service_name: &str,
        service: &ComposeServicePlan,
        in_pod: bool,
        body: &mut String,
    ) -> Result<()> {
        if in_pod {
            return Ok(());
        }
        for port in &service.ports {
            body.push_str(&format!(
                "PublishPort={}\n",
                render_port(service_name, port)?
            ));
        }
        Ok(())
    }

    fn render_service_ports_for_pod(
        &mut self,
        service_name: &str,
        service: &ComposeServicePlan,
        body: &mut String,
    ) -> Result<()> {
        for port in &service.ports {
            body.push_str(&format!(
                "PublishPort={}\n",
                render_port(service_name, port)?
            ));
        }
        Ok(())
    }

    fn render_volumes(
        &mut self,
        service_name: &str,
        volumes: &[ComposeVolumeMountPlan],
        body: &mut String,
    ) -> Result<()> {
        for (index, volume) in volumes.iter().enumerate() {
            let Some(source) = volume.source.as_deref() else {
                self.warn(
                    service_name,
                    &format!("anonymous volume at index {index} was not exported"),
                );
                continue;
            };
            let source = safe_unit_value(
                source,
                &format!("services.{service_name}.volumes[{index}].source"),
            )?;
            let target = safe_unit_value(
                &volume.target,
                &format!("services.{service_name}.volumes[{index}].target"),
            )?;
            let suffix = if volume.read_only { ":ro" } else { "" };
            body.push_str(&format!("Volume={source}:{target}{suffix}\n"));
        }
        Ok(())
    }

    fn render_labels(
        &mut self,
        service_name: &str,
        service: &ComposeServicePlan,
        body: &mut String,
    ) -> Result<()> {
        for (name, value) in &service.labels {
            let name = safe_unit_value(name, &format!("services.{service_name}.labels key"))?;
            let value = safe_unit_value(value, &format!("services.{service_name}.labels.{name}"))?;
            body.push_str(&format!("Label={name}={value}\n"));
        }
        Ok(())
    }

    fn render_resources(
        &mut self,
        service_name: &str,
        service: &ComposeServicePlan,
        body: &mut String,
    ) -> Result<()> {
        if let Some(bytes) = service.resources.memory_limit_bytes {
            body.push_str(&format!("Memory={bytes}\n"));
        }
        if service.resources.cpu_count.is_some() || service.resources.requested_cpus.is_some() {
            self.warn(
                service_name,
                "CPU limits are not exported because Nimbus does not use Quadlet PodmanArgs",
            );
        }
        if service.resources.disk_limit_bytes.is_some()
            || service.resources.log_limit_bytes.is_some()
            || service.resources.requested_disk.is_some()
            || service.resources.requested_log.is_some()
        {
            self.warn(
                service_name,
                "disk/log limits are not exported to Quadlet; review host policy manually",
            );
        }
        Ok(())
    }

    fn render_healthcheck(
        &mut self,
        service_name: &str,
        healthcheck: Option<&ComposeHealthcheckPlan>,
        body: &mut String,
    ) -> Result<()> {
        let Some(healthcheck) = healthcheck else {
            return Ok(());
        };
        if healthcheck.disable {
            self.warn(service_name, "disabled healthcheck was not exported");
            return Ok(());
        }
        if let Some(test) = healthcheck.test.as_ref() {
            body.push_str(&format!(
                "HealthCmd={}\n",
                render_health_command(test, &format!("services.{service_name}.healthcheck.test"))?
            ));
        }
        if let Some(interval) = healthcheck.interval.as_ref() {
            let interval = safe_unit_value(
                interval,
                &format!("services.{service_name}.healthcheck.interval"),
            )?;
            body.push_str(&format!("HealthInterval={interval}\n"));
        }
        if let Some(timeout) = healthcheck.timeout.as_ref() {
            let timeout = safe_unit_value(
                timeout,
                &format!("services.{service_name}.healthcheck.timeout"),
            )?;
            body.push_str(&format!("HealthTimeout={timeout}\n"));
        }
        if let Some(retries) = healthcheck.retries {
            body.push_str(&format!("HealthRetries={retries}\n"));
        }
        Ok(())
    }

    fn restart_policy(&mut self, service_name: &str, restart: &ComposeRestartPlan) -> &'static str {
        match restart.policy {
            SandboxRestartPolicy::Never => "no",
            SandboxRestartPolicy::OnFailure { max_restarts } => {
                if max_restarts > 0 {
                    self.warn(
                        service_name,
                        "Compose max restart count is not represented in Quadlet output",
                    );
                }
                "on-failure"
            }
            SandboxRestartPolicy::Always { max_restarts } => {
                if max_restarts > 0 {
                    self.warn(
                        service_name,
                        "Compose max restart count is not represented in Quadlet output",
                    );
                }
                "always"
            }
        }
    }

    fn render_kube_process(
        &mut self,
        service_name: &str,
        service: &ComposeServicePlan,
        rendered: &mut String,
    ) -> Result<()> {
        if let Some(entrypoint) = service.process.entrypoint.as_ref() {
            rendered.push_str("      command:\n");
            for part in command_parts(entrypoint, &format!("services.{service_name}.entrypoint"))? {
                rendered.push_str(&format!("        - {}\n", yaml_string(&part)));
            }
        }
        if let Some(command) = service.process.command.as_ref() {
            rendered.push_str("      args:\n");
            for part in command_parts(command, &format!("services.{service_name}.command"))? {
                rendered.push_str(&format!("        - {}\n", yaml_string(&part)));
            }
        }
        if service.process.working_dir.is_some() || service.process.user.is_some() {
            self.warn(
                service_name,
                "working_dir/user are not exported in --mode kube yet; review manually",
            );
        }
        Ok(())
    }

    fn render_kube_ports(
        &mut self,
        _service_name: &str,
        service: &ComposeServicePlan,
        rendered: &mut String,
    ) {
        if service.ports.is_empty() {
            return;
        }
        rendered.push_str("      ports:\n");
        for port in &service.ports {
            rendered.push_str(&format!(
                "        - containerPort: {}\n          hostPort: {}\n          hostIP: {}\n",
                port.guest_port,
                port.host_port,
                yaml_string(&port.host_address.to_string())
            ));
        }
    }

    fn render_kube_env(
        &mut self,
        service_name: &str,
        service: &ComposeServicePlan,
        rendered: &mut String,
    ) -> Result<()> {
        if service.process.environment.is_empty() {
            return Ok(());
        }
        rendered.push_str("      env:\n");
        for (name, value) in &service.process.environment {
            safe_env_name(name, &format!("services.{service_name}.environment"))?;
            safe_unit_value(
                value,
                &format!("services.{service_name}.environment.{name}"),
            )?;
            rendered.push_str(&format!(
                "        - name: {}\n          value: {}\n",
                yaml_string(name),
                yaml_string(value)
            ));
        }
        Ok(())
    }

    fn service_resource_name(&self, service_name: &str) -> String {
        format!(
            "{}-{}",
            safe_resource_name(&self.project.project_name),
            safe_resource_name(service_name)
        )
    }

    fn warn(&mut self, service_name: &str, message: &str) {
        self.warnings
            .push(format!("services.{service_name}: {message}"));
    }
}

fn render_artifacts_stdout(artifacts: &[QuadletArtifact]) -> String {
    let mut rendered = String::new();
    for artifact in artifacts {
        rendered.push_str(&format!("### {}\n", artifact.filename));
        rendered.push_str(&artifact.contents);
        if !artifact.contents.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push('\n');
    }
    rendered
}

fn write_artifacts(
    output_dir: &Path,
    artifacts: &[QuadletArtifact],
    overwrite: bool,
) -> Result<()> {
    fs::create_dir_all(output_dir).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to create output directory {}: {error}",
            output_dir.display()
        ))
    })?;
    for artifact in artifacts {
        let path = output_dir.join(&artifact.filename);
        if path.exists() && !overwrite {
            return Err(Error::AlreadyExists(format!(
                "{} exists; pass --overwrite to replace it",
                path.display()
            )));
        }
        fs::write(&path, artifact.contents.as_bytes()).map_err(|error| {
            Error::InvalidInput(format!("failed to write {}: {error}", path.display()))
        })?;
    }
    Ok(())
}

fn emit_warnings(warnings: &[String]) -> Result<()> {
    for warning in warnings {
        cli_ux::write_stderr_prefixed_line("Warning:", warning).map_err(|error| {
            Error::Internal(format!("failed to write quadlet export warning: {error}"))
        })?;
    }
    Ok(())
}

fn with_provenance(
    template: &str,
    project: &ComposeProjectPlan,
    service_name: Option<&str>,
    mode: ComposeQuadletExportMode,
    podman_version: Option<&str>,
    body: &str,
) -> String {
    let service = service_name.unwrap_or("-");
    let podman = podman_version.unwrap_or("unspecified");
    let mut hasher = Sha256::new();
    hasher.update(template.as_bytes());
    hasher.update(b"\n");
    hasher.update(project.source_file.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(project.project_name.as_bytes());
    hasher.update(b"\n");
    hasher.update(service.as_bytes());
    hasher.update(b"\n");
    hasher.update(format!("{mode:?}").as_bytes());
    hasher.update(b"\n");
    hasher.update(podman.as_bytes());
    hasher.update(b"\n");
    hasher.update(body.as_bytes());
    let hash = hasher.finalize();
    format!(
        "# Generated by Nimbus compose export quadlet. Review before installing.\n# Nimbus-Template: {template}\n# Nimbus-Source-Compose: {}\n# Nimbus-Project: {}\n# Nimbus-Service: {service}\n# Nimbus-Mode: {mode:?}\n# Nimbus-Podman-Version: {podman}\n# Nimbus-Provenance-SHA256: sha256:{hash:x}\n{body}",
        project.source_file.display(),
        project.project_name,
    )
}

fn render_port(service_name: &str, port: &ComposePortBindingPlan) -> Result<String> {
    match port.protocol {
        PublishedEndpointProtocol::Tcp => Ok(format!(
            "{}:{}:{}",
            render_ip(port.host_address),
            port.host_port,
            port.guest_port
        )),
        other => Err(Error::InvalidInput(format!(
            "services.{service_name}.ports.{}: Quadlet export only supports tcp ports, got {other:?}",
            port.name
        ))),
    }
}

fn render_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(value) => value.to_string(),
        IpAddr::V6(value) => format!("[{value}]"),
    }
}

fn render_command(command: &ComposeCommandPlan, field: &str) -> Result<String> {
    Ok(command_parts(command, field)?.join(" "))
}

fn render_health_command(command: &ComposeCommandPlan, field: &str) -> Result<String> {
    let mut parts = command_parts(command, field)?;
    if matches!(parts.first().map(String::as_str), Some("CMD" | "CMD-SHELL")) {
        parts.remove(0);
    }
    if parts.is_empty() {
        return Err(Error::InvalidInput(format!("{field} cannot be empty")));
    }
    Ok(parts.join(" "))
}

fn command_parts(command: &ComposeCommandPlan, field: &str) -> Result<Vec<String>> {
    match command {
        ComposeCommandPlan::String(value) => Ok(vec![safe_unit_value(value, field)?.to_string()]),
        ComposeCommandPlan::List(values) => values
            .iter()
            .map(|value| safe_unit_token(value, field).map(ToString::to_string))
            .collect(),
    }
}

fn safe_unit_token<'a>(value: &'a str, field: &str) -> Result<&'a str> {
    let value = safe_unit_value(value, field)?;
    if value.contains(char::is_whitespace) {
        return Err(Error::InvalidInput(format!(
            "{field} list values cannot contain whitespace because Quadlet stores commands as a single line"
        )));
    }
    Ok(value)
}

fn safe_unit_value<'a>(value: &'a str, field: &str) -> Result<&'a str> {
    if value.is_empty()
        || value.contains('\0')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('%')
    {
        return Err(Error::InvalidInput(format!(
            "{field} contains an unsupported empty, control-character, or systemd-specifier value"
        )));
    }
    Ok(value)
}

fn safe_env_name(name: &str, field: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || name.as_bytes()[0].is_ascii_digit()
    {
        return Err(Error::InvalidInput(format!(
            "{field}: environment key `{name}` must be ASCII identifier shaped"
        )));
    }
    Ok(())
}

fn safe_resource_name(value: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if next == '-' {
            if !previous_dash && !out.is_empty() {
                out.push('-');
            }
            previous_dash = true;
        } else {
            out.push(next);
            previous_dash = false;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "nimbus".to_string()
    } else {
        trimmed.chars().take(56).collect()
    }
}

fn parse_duration_seconds(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(raw) = value.strip_suffix("ms") {
        let millis = raw.parse::<u64>().ok()?;
        return Some(millis.div_ceil(1000));
    }
    if let Some(raw) = value.strip_suffix('s') {
        return raw.parse::<u64>().ok();
    }
    if let Some(raw) = value.strip_suffix('m') {
        return raw.parse::<u64>().ok().map(|minutes| minutes * 60);
    }
    if let Some(raw) = value.strip_suffix('h') {
        return raw.parse::<u64>().ok().map(|hours| hours * 3600);
    }
    value.parse::<u64>().ok()
}

fn kube_restart_policy(policy: SandboxRestartPolicy) -> &'static str {
    match policy {
        SandboxRestartPolicy::Never => "Never",
        SandboxRestartPolicy::OnFailure { .. } => "OnFailure",
        SandboxRestartPolicy::Always { .. } => "Always",
    }
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use crate::compose::{ComposeCommand, ComposeSubcommand};
    use clap::Parser;
    use std::path::PathBuf;

    fn write_compose(tempdir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = tempdir.path().join("compose.yaml");
        fs::write(&path, body).expect("compose fixture should write");
        path
    }

    fn command_for(path: &Path, mode: ComposeQuadletExportMode) -> ComposeExportQuadletCommand {
        ComposeExportQuadletCommand {
            file: vec![path.to_path_buf()],
            service: Vec::new(),
            mode,
            podman_version: Some("5.6.0".to_string()),
            output_dir: None,
            overwrite: false,
            strict: false,
        }
    }

    #[test]
    fn containers_mode_renders_reviewable_quadlet_artifact_to_stdout_shape() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose(
            &tempdir,
            r#"
name: Demo App
services:
  web:
    image: ghcr.io/example/web:v1
    command: ["serve", "--port", "8080"]
    environment:
      RUST_LOG: info
    ports:
      - "127.0.0.1:18080:8080"
    volumes:
      - webdata:/data:ro
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://127.0.0.1:8080/health"]
      interval: 30s
    restart: on-failure
volumes:
  webdata: {}
"#,
        );
        let selection = ResolvedComposeSelection::explicit(compose.clone());
        let rendered = render_quadlet_export_for_selection(
            &selection,
            &command_for(&compose, ComposeQuadletExportMode::Containers),
        )
        .expect("quadlet export should render");

        assert_eq!(rendered.artifacts.len(), 1);
        let artifact = &rendered.artifacts[0];
        assert_eq!(artifact.filename, "demo-app-web.container");
        assert!(artifact.contents.contains("[Container]"));
        assert!(artifact.contents.contains("Image=ghcr.io/example/web:v1"));
        assert!(
            artifact
                .contents
                .contains("PublishPort=127.0.0.1:18080:8080")
        );
        assert!(artifact.contents.contains("Volume=webdata:/data:ro"));
        assert!(
            artifact
                .contents
                .contains("HealthCmd=curl -fsS http://127.0.0.1:8080/health")
        );
        assert!(
            artifact
                .contents
                .contains("# Nimbus-Provenance-SHA256: sha256:")
        );
        assert!(!artifact.contents.contains("PodmanArgs"));
        assert!(!artifact.contents.contains("Network=host"));

        let stdout = render_artifacts_stdout(&rendered.artifacts);
        assert!(stdout.starts_with("### demo-app-web.container\n"));
    }

    #[test]
    fn strict_mode_fails_when_export_would_drop_review_material() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose(
            &tempdir,
            r#"
services:
  api:
    build:
      context: .
    deploy:
      resources:
        limits:
          cpus: "0.5"
"#,
        );
        let mut command = command_for(&compose, ComposeQuadletExportMode::Containers);
        command.strict = true;
        let selection = ResolvedComposeSelection::explicit(compose);

        let error = render_quadlet_export_for_selection(&selection, &command)
            .expect_err("strict export should fail on warnings");

        assert!(error.to_string().contains("--strict mode"));
        assert!(
            error
                .to_string()
                .contains("build services are not exported")
        );
    }

    #[test]
    fn output_dir_refuses_overwrite_without_explicit_flag() {
        let artifact = QuadletArtifact {
            filename: "demo.container".to_string(),
            contents: "[Container]\nImage=demo\n".to_string(),
        };
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        fs::write(tempdir.path().join("demo.container"), "existing\n")
            .expect("fixture should write");

        let error =
            write_artifacts(tempdir.path(), &[artifact], false).expect_err("overwrite should fail");

        assert!(error.to_string().contains("--overwrite"));
    }

    #[test]
    fn pod_and_kube_modes_render_expected_review_artifacts() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let compose = write_compose(
            &tempdir,
            r#"
name: Demo
services:
  api:
    image: ghcr.io/example/api:v1
    ports:
      - "127.0.0.1:18080:8080"
  worker:
    image: ghcr.io/example/worker:v1
"#,
        );
        let selection = ResolvedComposeSelection::explicit(compose.clone());

        let pod = render_quadlet_export_for_selection(
            &selection,
            &command_for(&compose, ComposeQuadletExportMode::Pod),
        )
        .expect("pod mode should render");
        assert!(
            pod.artifacts
                .iter()
                .any(|artifact| artifact.filename == "demo.pod")
        );
        assert!(
            pod.artifacts
                .iter()
                .any(|artifact| artifact.contents.contains("Pod=demo.pod"))
        );

        let kube = render_quadlet_export_for_selection(
            &selection,
            &command_for(&compose, ComposeQuadletExportMode::Kube),
        )
        .expect("kube mode should render");
        assert!(
            kube.artifacts
                .iter()
                .any(|artifact| artifact.filename == "demo.kube")
        );
        assert!(
            kube.artifacts
                .iter()
                .any(|artifact| artifact.filename == "demo.yaml")
        );
        assert!(
            kube.artifacts
                .iter()
                .any(|artifact| artifact.contents.contains("Yaml=demo.yaml"))
        );
    }

    #[test]
    fn export_quadlet_command_parses() {
        let cli = Cli::parse_from([
            "nimbus",
            "compose",
            "export",
            "quadlet",
            "--file",
            "compose.yaml",
            "--service",
            "api",
            "--mode",
            "pod",
            "--strict",
        ]);
        let crate::Command::Compose(ComposeCommand {
            command: ComposeSubcommand::Export(export),
        }) = cli.command
        else {
            panic!("compose export should parse");
        };
        let super::super::ComposeExportSubcommand::Quadlet(command) = export.command;
        assert_eq!(command.file, vec![PathBuf::from("compose.yaml")]);
        assert_eq!(command.service, vec!["api"]);
        assert_eq!(command.mode, ComposeQuadletExportMode::Pod);
        assert!(command.strict);
    }
}
