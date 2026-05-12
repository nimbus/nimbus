use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use neovex::{ConvexRegistry, Error, LicenseState, Service, TenantId, run_scheduler};
use neovex_server::{
    CloudFunctionsRegistry, LocalServerPaths, LocalServerSecurityState, ServeOptions,
    ServerDiscoveryLease, load_or_create_local_admin_token, serve_with_options,
};

use super::StartCommand;
use super::config::{
    control_data_dir_from_persistence_config, persistence_config_from_start_command,
};
use super::runtime_limits::runtime_limits_from_command;
use crate::cli_ux;
use crate::codegen::{CodegenOptions, run_codegen_for_app_dir_with_options};
use crate::compose::discovery::{
    ResolvedComposeSelection, compose_selection_summary, resolve_compose_selection,
};
use crate::compose::load_host_backed_sandbox_service_manager_for_selection;
use crate::deploy::resolve_deploy_app_dir;
use crate::dirs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ResolvedStartAppDir {
    Explicit(PathBuf),
    AutoDetected(PathBuf),
}

impl ResolvedStartAppDir {
    fn path(&self) -> &Path {
        match self {
            Self::Explicit(path) | Self::AutoDetected(path) => path.as_path(),
        }
    }
}

pub(crate) async fn run_start_command(
    command: StartCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let persistence_config = persistence_config_from_start_command(&command)?;
    let compose_control_data_dir =
        control_data_dir_from_persistence_config(&persistence_config).to_path_buf();
    let resolved_app_dir = resolve_start_app_dir(&command)?;
    run_codegen_preflight(&command, resolved_app_dir.as_ref()).await?;
    let runtime_limits = runtime_limits_from_command(&command);
    let license_file = resolve_license_path(command.license_file.as_deref());
    let license_state = LicenseState::load(license_file.as_deref())?;
    let license_snapshot = license_state.snapshot();
    let deploy_admin_enabled =
        command.deploy_admin_token.is_some() || std::env::var_os("NEOVEX_DEPLOY_TOKEN").is_some();
    let convex_registry =
        load_convex_registry(&command, resolved_app_dir.as_ref(), &runtime_limits)?;
    let cloud_functions_registry =
        load_cloud_functions_registry(&command, resolved_app_dir.as_ref(), &runtime_limits)?;
    let compose_selection = resolve_optional_compose_selection(&command)?;
    let sandbox_service_manager =
        load_sandbox_service_manager(compose_selection.as_ref(), &compose_control_data_dir)?;
    let local_server_paths = LocalServerPaths::resolve_for_current_platform()?;
    let local_admin_token = load_or_create_local_admin_token(&local_server_paths)?;
    let local_server_security = Arc::new(LocalServerSecurityState::new(
        local_server_paths.clone(),
        local_admin_token,
    ));
    let service = Arc::new(Service::new_with_persistence_config(persistence_config).await?);
    let shutdown_service = service.clone();
    service.recover_scheduled_work_on_startup_async().await?;
    if let Some(tenant_name) = &command.auto_tenant {
        ensure_auto_tenant(&service, tenant_name)?;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let scheduler_service = service.clone();
    let scheduler_handle = tokio::spawn(async move {
        run_scheduler(scheduler_service, shutdown_rx).await;
    });
    let listener = tokio::net::TcpListener::bind((command.host.as_str(), command.port)).await?;
    let discovery_lease =
        ServerDiscoveryLease::acquire(&local_server_paths, listener.local_addr()?)?;
    emit_start_startup_summary(
        &command,
        resolved_app_dir.as_ref(),
        compose_selection.as_ref(),
        listener.local_addr()?,
        deploy_admin_enabled,
    );
    emit_non_loopback_warning(listener.local_addr()?);

    tracing::info!(
        license_kind = ?license_snapshot.kind,
        license_status = ?license_snapshot.status,
        license_source = ?license_snapshot.source.kind,
        "loaded neovex license state"
    );
    for warning in &license_snapshot.warnings {
        tracing::warn!(license_warning = %warning, "neovex license warning");
    }

    tracing::info!("neovex listening on {}", listener.local_addr()?);
    let mut serve_options = ServeOptions::default().with_license(license_state);
    if let Some(registry) = convex_registry {
        serve_options = serve_options.with_convex_registry(registry);
    }
    if let Some(registry) = cloud_functions_registry {
        serve_options = serve_options.with_cloud_functions_registry(registry);
    }
    if let Some(manager) = sandbox_service_manager {
        serve_options = serve_options.with_sandbox_service_manager(manager);
    }
    if let Some(token) = command.deploy_admin_token {
        serve_options = serve_options.with_deploy_admin_token(token);
    }
    serve_options = serve_options.with_local_server_security(local_server_security);

    let server_result = serve_with_options(listener, service, serve_options).await;
    drop(discovery_lease);
    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
    shutdown_service.quiesce().await;
    server_result?;
    Ok(())
}

pub(super) async fn run_codegen_preflight(
    command: &StartCommand,
    resolved_app_dir: Option<&ResolvedStartAppDir>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(app_dir) = resolved_app_dir.map(ResolvedStartAppDir::path) else {
        return Ok(());
    };
    if command.skip_codegen {
        return Ok(());
    }

    emit_start_info(format!(
        "running one-shot codegen preflight for {}",
        app_dir.display()
    ));
    run_codegen_for_app_dir_with_options(
        app_dir,
        CodegenOptions {
            debug_node_apis: command.debug_node_apis,
        },
    )
    .await?;
    emit_start_info(format!("generated app artifacts for {}", app_dir.display()));
    Ok(())
}

pub(super) fn load_convex_registry(
    command: &StartCommand,
    resolved_app_dir: Option<&ResolvedStartAppDir>,
    runtime_limits: &neovex::RuntimeLimits,
) -> Result<Option<ConvexRegistry>, Error> {
    let Some(resolved_app_dir) = resolved_app_dir else {
        return Ok(None);
    };
    let should_load = match resolved_app_dir {
        ResolvedStartAppDir::Explicit(_) => true,
        ResolvedStartAppDir::AutoDetected(path) => app_dir_has_convex_surface(path),
    };
    if !should_load {
        return Ok(None);
    }
    let path = resolved_app_dir.path();
    ensure_required_functions_manifest(path, command.skip_codegen)?;
    ConvexRegistry::from_app_dir(path)
        .map(|registry| Some(registry.with_runtime_limits(runtime_limits.clone())))
}

pub(super) fn load_cloud_functions_registry(
    command: &StartCommand,
    resolved_app_dir: Option<&ResolvedStartAppDir>,
    runtime_limits: &neovex::RuntimeLimits,
) -> Result<Option<CloudFunctionsRegistry>, Error> {
    let Some(resolved_app_dir) = resolved_app_dir else {
        return Ok(None);
    };
    let should_load = match resolved_app_dir {
        ResolvedStartAppDir::Explicit(_) => true,
        ResolvedStartAppDir::AutoDetected(path) => app_dir_has_cloud_functions_surface(path),
    };
    if !should_load {
        return Ok(None);
    }
    let path = resolved_app_dir.path();
    ensure_required_cloud_functions_manifest(path, command.skip_codegen)?;
    CloudFunctionsRegistry::from_app_dir(path)
        .map(|registry| Some(registry.with_runtime_limits(runtime_limits.clone())))
}

pub(crate) fn resolve_optional_compose_selection(
    command: &StartCommand,
) -> Result<Option<ResolvedComposeSelection>, Error> {
    let cwd = std::env::current_dir().map_err(|error| {
        Error::Internal(format!("failed to determine current directory: {error}"))
    })?;
    let explicit_compose_files = command.compose_file.as_slice();
    resolve_compose_selection(explicit_compose_files, &cwd)
        .map_err(|error| Error::InvalidInput(error.to_string()))
}

pub(super) fn load_sandbox_service_manager(
    compose_selection: Option<&ResolvedComposeSelection>,
    compose_control_data_dir: &std::path::Path,
) -> Result<Option<Arc<neovex::SandboxServiceManager>>, Error> {
    compose_selection
        .map(|selection| {
            load_host_backed_sandbox_service_manager_for_selection(
                selection,
                compose_control_data_dir,
            )
        })
        .transpose()
        .map(|manager| manager.map(Arc::new))
}

fn emit_start_info(message: impl AsRef<str>) {
    if cli_ux::info_output_enabled() {
        let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
    }
}

fn emit_start_warning(message: impl AsRef<str>) {
    if cli_ux::info_output_enabled() {
        let _ = cli_ux::write_stderr_prefixed_line("warning:", message.as_ref());
    }
}

fn emit_non_loopback_warning(listen_addr: SocketAddr) {
    if !listen_addr.ip().is_loopback() {
        emit_start_warning(format!(
            "server is listening on non-loopback address {}; local admin routes are reachable from that interface",
            listen_addr.ip()
        ));
    }
}

fn emit_start_startup_summary(
    command: &StartCommand,
    resolved_app_dir: Option<&ResolvedStartAppDir>,
    compose_selection: Option<&ResolvedComposeSelection>,
    listen_addr: SocketAddr,
    deploy_admin_enabled: bool,
) {
    for line in start_startup_summary_lines(
        command,
        resolved_app_dir,
        compose_selection,
        listen_addr,
        deploy_admin_enabled,
    ) {
        emit_start_info(line);
    }
}

pub(super) fn start_startup_summary_lines(
    command: &StartCommand,
    resolved_app_dir: Option<&ResolvedStartAppDir>,
    compose_selection: Option<&ResolvedComposeSelection>,
    listen_addr: SocketAddr,
    deploy_admin_enabled: bool,
) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Neovex server listening at {}",
            local_listen_url(listen_addr)
        ),
        "server process owns HTTP, WebSocket, scheduler, and runtime startup".to_string(),
    ];
    match resolved_app_dir {
        Some(ResolvedStartAppDir::Explicit(app_dir)) => {
            lines.push(format!("app dir: {}", app_dir.display()));
            if command.skip_codegen {
                lines.push("codegen preflight: skipped by --skip-codegen".to_string());
            } else {
                lines.push("codegen preflight: completed before registry load".to_string());
            }
        }
        Some(ResolvedStartAppDir::AutoDetected(app_dir)) => {
            lines.push(format!("app dir: auto-detected {}", app_dir.display()));
            if command.skip_codegen {
                lines.push("codegen preflight: skipped by --skip-codegen".to_string());
            } else {
                lines.push("codegen preflight: completed before registry load".to_string());
            }
        }
        None => lines
            .push("app dir: none; Convex-compatible routes wait for deploy activation".to_string()),
    }
    if let Some(selection) = compose_selection {
        lines.push(format!(
            "compose file: {}",
            compose_selection_summary(selection)
        ));
    }
    if deploy_admin_enabled {
        lines.push("deploy admin API: enabled".to_string());
    }
    lines
}

pub(super) fn resolve_start_app_dir(
    command: &StartCommand,
) -> Result<Option<ResolvedStartAppDir>, Error> {
    let cwd = std::env::current_dir().map_err(|error| {
        Error::Internal(format!("failed to determine current directory: {error}"))
    })?;
    if let Some(explicit_app_dir) = command.app_dir.as_deref() {
        let resolved = resolve_deploy_app_dir(Some(explicit_app_dir), &cwd)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        return Ok(Some(ResolvedStartAppDir::Explicit(resolved)));
    }

    let detected = resolve_deploy_app_dir(None, &cwd)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    if app_dir_has_convex_surface(&detected) || app_dir_has_cloud_functions_surface(&detected) {
        return Ok(Some(ResolvedStartAppDir::AutoDetected(detected)));
    }
    Ok(None)
}

fn local_listen_url(addr: SocketAddr) -> String {
    let host = if addr.ip().is_unspecified() {
        "localhost".to_string()
    } else if addr.ip().is_ipv6() {
        format!("[{}]", addr.ip())
    } else {
        addr.ip().to_string()
    };
    format!("http://{host}:{}/", addr.port())
}

fn app_dir_has_convex_surface(app_dir: &Path) -> bool {
    app_dir.join("convex").is_dir()
        || app_dir.join("neovex").is_dir()
        || required_functions_manifest_path(app_dir).is_file()
}

fn app_dir_has_cloud_functions_surface(app_dir: &Path) -> bool {
    app_dir.join("firebase.json").is_file()
        || required_cloud_functions_manifest_path(app_dir).is_file()
        || package_declares_functions_framework(&app_dir.join("package.json"))
}

fn ensure_required_functions_manifest(app_dir: &Path, skip_codegen: bool) -> Result<(), Error> {
    let functions_path = required_functions_manifest_path(app_dir);
    match std::fs::read_to_string(&functions_path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(Error::InvalidInput(format!(
                "No generated function manifest found at {}.\n\n{}",
                functions_path.display(),
                manifest_recovery_hint(app_dir, skip_codegen)
            )))
        }
        Err(error) => Err(Error::InvalidInput(format!(
            "Generated function manifest {} is not readable: {error}.\n\n{}",
            functions_path.display(),
            manifest_recovery_hint(app_dir, skip_codegen)
        ))),
    }
}

fn required_functions_manifest_path(app_dir: &Path) -> PathBuf {
    app_dir
        .join(".neovex")
        .join("convex")
        .join("functions.json")
}

fn ensure_required_cloud_functions_manifest(
    app_dir: &Path,
    skip_codegen: bool,
) -> Result<(), Error> {
    let artifact_path = required_cloud_functions_manifest_path(app_dir);
    match std::fs::read_to_string(&artifact_path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(Error::InvalidInput(format!(
                "No generated cloud functions artifact manifest found at {}.\n\n{}",
                artifact_path.display(),
                cloud_functions_manifest_recovery_hint(app_dir, skip_codegen)
            )))
        }
        Err(error) => Err(Error::InvalidInput(format!(
            "Generated cloud functions artifact manifest {} is not readable: {error}.\n\n{}",
            artifact_path.display(),
            cloud_functions_manifest_recovery_hint(app_dir, skip_codegen)
        ))),
    }
}

fn required_cloud_functions_manifest_path(app_dir: &Path) -> PathBuf {
    app_dir
        .join(".neovex")
        .join("firebase")
        .join("artifact.json")
}

fn package_declares_functions_framework(package_json_path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(package_json_path) else {
        return false;
    };
    let Ok(package_json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return false;
    };
    [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ]
    .into_iter()
    .any(|field| {
        package_json
            .get(field)
            .and_then(serde_json::Value::as_object)
            .is_some_and(|deps| deps.contains_key("@google-cloud/functions-framework"))
    })
}

fn ensure_auto_tenant(
    service: &Service,
    tenant_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let tenant_id = TenantId::new(tenant_name)?;
    match service.create_tenant(tenant_id) {
        Ok(()) => {
            emit_start_info(format!("auto-created tenant \"{tenant_name}\""));
        }
        Err(Error::AlreadyExists(_)) => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn manifest_recovery_hint(app_dir: &Path, skip_codegen: bool) -> String {
    if skip_codegen {
        format!(
            "Run \"neovex codegen --app {}\" to generate it, or remove --skip-codegen to generate manifests automatically on start.",
            app_dir.display()
        )
    } else {
        format!(
            "Run \"neovex codegen --app {}\" to generate it before retrying.",
            app_dir.display()
        )
    }
}

fn cloud_functions_manifest_recovery_hint(app_dir: &Path, skip_codegen: bool) -> String {
    if skip_codegen {
        format!(
            "Run \"neovex codegen --app {}\" to generate it, or remove --skip-codegen to generate manifests automatically on start.",
            app_dir.display()
        )
    } else {
        format!(
            "Run \"neovex codegen --app {}\" to generate it before retrying.",
            app_dir.display()
        )
    }
}

pub(super) fn resolve_license_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if std::env::var_os(neovex::LICENSE_FILE_ENV).is_some() {
        return None;
    }
    let config_dir = dirs::global_config_dir().ok()?;
    let default_path = config_dir.join("license.json");
    default_path.exists().then_some(default_path)
}
