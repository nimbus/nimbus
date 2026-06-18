use std::net::SocketAddr;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(unix)]
use std::{env, process};

use nimbus::{ConvexRegistry, Engine, Error, LicenseState, TenantId, run_scheduler};
use nimbus_server::{
    CloudFunctionsRegistry, LocalServerPaths, LocalServerSecurityState, ServeOptions,
    ServerDiscoveryLease, load_or_create_local_admin_token, serve,
};

use super::StartCommand;
use super::adapters::{AdapterEnablement, resolve_adapter_enablement};
use super::config::{
    control_data_dir_from_persistence_config, persistence_config_from_start_command,
};
use super::first_boot::{is_first_boot, spawn_first_boot_announce};
use super::network_bind::{ensure_admin_token_rotated_for_public_bind, ensure_host_opt_in};
use super::runtime_limits::runtime_limits_from_command;
use crate::cli_ux;
use crate::codegen::{CodegenOptions, run_codegen_for_app_dir_with_options};
use crate::compose::discovery::{
    ResolvedComposeSelection, compose_selection_summary, resolve_explicit_compose_selection,
};
use crate::compose::load_host_backed_service_manager_for_selection_with_isolation_mode;
use crate::deploy::resolve_deploy_app_dir;
use crate::dirs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedStartAppDir {
    Explicit(PathBuf),
}

impl ResolvedStartAppDir {
    fn path(&self) -> &Path {
        match self {
            Self::Explicit(path) => path.as_path(),
        }
    }
}

pub(crate) async fn run_start_command(
    command: StartCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    // Stage 1 of the network-bind gate runs before any expensive
    // initialization so a typo'd `--host` (or a forgotten
    // `--allow-network`) fails fast without paying codegen or registry
    // load costs. The freshness check (stage 2) runs after the admin
    // token is loaded from disk.
    if !command.systemd_socket_activation {
        ensure_host_opt_in(&command.host, command.allow_network)?;
    }
    let cors_allowed_origins = resolve_cors_allowed_origins(&command)?;
    let tls_config = match (&command.tls_cert, &command.tls_key) {
        (Some(cert), Some(key)) => Some(nimbus_server::TlsConfig::new(cert, key)),
        _ => None,
    };
    let tls_enabled = tls_config.is_some();
    let persistence_config = persistence_config_from_start_command(&command)?;
    let compose_control_data_dir =
        control_data_dir_from_persistence_config(&persistence_config).to_path_buf();
    // Adapter enablement resolves after the control data dir: default-on
    // listeners without operator credentials load (or generate) theirs
    // from the wire-credential store under that dir.
    let adapter_enablement = resolve_adapter_enablement(&command, &compose_control_data_dir)?;
    // Snapshot first-boot before `Engine::new_with_persistence_config`
    // touches the data dir; otherwise the marker landscape we observe
    // would always say "second boot" because Engine initialization
    // would have already populated the dir. The H5 banner is fired
    // after the listener is up and the discovery lease is held so the
    // launch ticket can mint against the live server.
    let is_first_boot_run = is_first_boot(&compose_control_data_dir);
    let resolved_app_dir = resolve_start_app_dir(&command)?;
    run_codegen_preflight(&command, resolved_app_dir.as_ref()).await?;
    let runtime_limits = runtime_limits_from_command(&command);
    let license_file = resolve_license_path(command.license_file.as_deref());
    let license_state = LicenseState::load(license_file.as_deref())?;
    let license_snapshot = license_state.snapshot();
    let deploy_admin_enabled =
        command.deploy_admin_token.is_some() || std::env::var_os("NIMBUS_DEPLOY_TOKEN").is_some();
    let convex_registry =
        load_convex_registry(&command, resolved_app_dir.as_ref(), &runtime_limits)?;
    let cloud_functions_registry =
        load_cloud_functions_registry(&command, resolved_app_dir.as_ref(), &runtime_limits)?;
    let compose_selection = resolve_optional_compose_selection(&command)?;
    let service_manager = load_service_manager(
        compose_selection.as_ref(),
        &compose_control_data_dir,
        command.tenant_isolation_mode,
    )?;
    let machine_lifecycle_manager = crate::machine::host_machine_lifecycle_manager()?;
    let local_server_paths = LocalServerPaths::resolve_for_current_platform()?;
    let local_admin_token = load_or_create_local_admin_token(&local_server_paths)?;
    if !command.systemd_socket_activation {
        let rotation_warning = ensure_admin_token_rotated_for_public_bind(
            &command.host,
            &local_admin_token,
            time::OffsetDateTime::now_utc(),
        )?;
        emit_rotation_warning(rotation_warning);
    }
    let activated_listener = if command.systemd_socket_activation {
        let listener = start_listener(&command).await?;
        let activated_host = listener.local_addr()?.ip().to_string();
        ensure_host_opt_in(&activated_host, command.allow_network)?;
        let rotation_warning = ensure_admin_token_rotated_for_public_bind(
            &activated_host,
            &local_admin_token,
            time::OffsetDateTime::now_utc(),
        )?;
        emit_rotation_warning(rotation_warning);
        Some(listener)
    } else {
        None
    };
    let local_server_security = Arc::new(LocalServerSecurityState::new(
        local_server_paths.clone(),
        local_admin_token,
    ));
    let engine = Arc::new(Engine::new_with_persistence_config(persistence_config).await?);
    let shutdown_engine = engine.clone();
    engine.recover_scheduled_work_on_startup_async().await?;
    if let Some(tenant_name) = &command.auto_tenant {
        ensure_auto_tenant(&engine, tenant_name)?;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let scheduler_engine = engine.clone();
    let scheduler_handle = tokio::spawn(async move {
        run_scheduler(scheduler_engine, shutdown_rx).await;
    });
    let listener = match activated_listener {
        Some(listener) => listener,
        None => start_listener(&command).await?,
    };
    let discovery_lease =
        ServerDiscoveryLease::acquire(&local_server_paths, listener.local_addr()?)?;
    emit_start_startup_summary(
        &command,
        resolved_app_dir.as_ref(),
        compose_selection.as_ref(),
        &adapter_enablement,
        listener.local_addr()?,
        deploy_admin_enabled,
    );
    let first_boot_handle = if is_first_boot_run {
        let console_url =
            operator_console_url_from_base(&local_listen_url(listener.local_addr()?, tls_enabled));
        Some(spawn_first_boot_announce(
            console_url,
            local_server_paths.clone(),
            compose_control_data_dir.clone(),
        ))
    } else {
        None
    };

    tracing::info!(
        license_kind = ?license_snapshot.kind,
        license_status = ?license_snapshot.status,
        license_source = ?license_snapshot.source.kind,
        "loaded nimbus license state"
    );
    for warning in &license_snapshot.warnings {
        tracing::warn!(license_warning = %warning, "nimbus license warning");
    }

    tracing::info!("nimbus listening on {}", listener.local_addr()?);
    let mut serve_options = ServeOptions::new(engine.clone()).with_license(license_state);
    if let Some(registry) = convex_registry {
        serve_options = serve_options.with_convex_registry(registry);
    }
    if let Some(registry) = cloud_functions_registry {
        serve_options = serve_options.with_cloud_functions_registry(registry);
    }
    if let Some(manager) = service_manager {
        serve_options = serve_options.with_service_manager(manager);
    }
    serve_options = serve_options.with_machine_lifecycle_manager(machine_lifecycle_manager);
    if let Some(token) = command.deploy_admin_token {
        serve_options = serve_options.with_deploy_admin_token(token);
    }
    serve_options = serve_options.with_local_server_security(local_server_security);
    serve_options = serve_options.with_tenant_isolation_mode(command.tenant_isolation_mode);
    serve_options = serve_options.with_cors_allowed_origins(cors_allowed_origins);
    serve_options = adapter_enablement.apply_to(serve_options);
    if let Some(tls_config) = tls_config {
        serve_options = serve_options.with_tls(tls_config);
    }

    let server_result = serve(listener, serve_options).await;
    drop(discovery_lease);
    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
    if let Some(handle) = first_boot_handle {
        handle.abort();
        let _ = handle.await;
    }
    shutdown_engine.quiesce().await;
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
    runtime_limits: &nimbus::RuntimeLimits,
) -> Result<Option<ConvexRegistry>, Error> {
    let Some(resolved_app_dir) = resolved_app_dir else {
        return Ok(None);
    };
    let path = resolved_app_dir.path();
    if !app_dir_has_convex_surface(path) {
        return Ok(None);
    }
    ensure_required_functions_manifest(path, command.skip_codegen)?;
    ConvexRegistry::from_app_dir(path)
        .map(|registry| Some(registry.with_runtime_limits(runtime_limits.clone())))
}

pub(super) fn load_cloud_functions_registry(
    command: &StartCommand,
    resolved_app_dir: Option<&ResolvedStartAppDir>,
    runtime_limits: &nimbus::RuntimeLimits,
) -> Result<Option<CloudFunctionsRegistry>, Error> {
    let Some(resolved_app_dir) = resolved_app_dir else {
        return Ok(None);
    };
    let path = resolved_app_dir.path();
    if !app_dir_has_cloud_functions_surface(path) {
        return Ok(None);
    }
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
    resolve_explicit_compose_selection(explicit_compose_files, &cwd)
        .map_err(|error| Error::InvalidInput(error.to_string()))
}

pub(super) fn load_service_manager(
    compose_selection: Option<&ResolvedComposeSelection>,
    compose_control_data_dir: &std::path::Path,
    tenant_isolation_mode: nimbus_server::TenantIsolationMode,
) -> Result<Option<Arc<nimbus::ServiceManager>>, Error> {
    compose_selection
        .map(|selection| {
            load_host_backed_service_manager_for_selection_with_isolation_mode(
                selection,
                compose_control_data_dir,
                tenant_isolation_mode,
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

/// Surface the advisory stale-rotation notice on both operator channels.
/// Unlike `emit_start_info`, warnings are not gated on info output.
fn emit_rotation_warning(warning: Option<super::network_bind::StaleRotationWarning>) {
    if let Some(warning) = warning {
        tracing::warn!(age_days = warning.age_days, "{warning}");
        let _ = cli_ux::write_stderr_prefixed_line("warning:", &warning.to_string());
    }
}

/// `--cors-allow-origin` flags win; otherwise the comma-separated
/// NIMBUS_CORS_ALLOW_ORIGINS env var applies. Every value is normalized
/// or rejected — never silently dropped.
pub(super) fn resolve_cors_allowed_origins(command: &StartCommand) -> Result<Vec<String>, Error> {
    if !command.cors_allow_origin.is_empty() {
        return Ok(command.cors_allow_origin.clone());
    }
    let Ok(raw) = std::env::var("NIMBUS_CORS_ALLOW_ORIGINS") else {
        return Ok(Vec::new());
    };
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            nimbus_server::normalize_cors_origin(value).map_err(|reason| {
                Error::InvalidInput(format!("NIMBUS_CORS_ALLOW_ORIGINS: {reason}"))
            })
        })
        .collect()
}

fn emit_start_startup_summary(
    command: &StartCommand,
    resolved_app_dir: Option<&ResolvedStartAppDir>,
    compose_selection: Option<&ResolvedComposeSelection>,
    adapter_enablement: &AdapterEnablement,
    listen_addr: SocketAddr,
    deploy_admin_enabled: bool,
) {
    for line in start_startup_summary_lines(
        command,
        resolved_app_dir,
        compose_selection,
        adapter_enablement,
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
    adapter_enablement: &AdapterEnablement,
    listen_addr: SocketAddr,
    deploy_admin_enabled: bool,
) -> Vec<String> {
    let base_url = local_listen_url(
        listen_addr,
        command.tls_cert.is_some() && command.tls_key.is_some(),
    );
    let mut lines = vec![
        format!("Nimbus server listening at {base_url}"),
        format!(
            "operator console:\t{}",
            operator_console_url_from_base(&base_url)
        ),
        "server process owns HTTP, WebSocket, scheduler, and runtime startup".to_string(),
        format!(
            "tenant isolation:\t{}",
            command.tenant_isolation_mode.as_str()
        ),
    ];
    lines.extend(adapter_enablement.status_lines());
    match resolved_app_dir {
        Some(ResolvedStartAppDir::Explicit(app_dir)) => {
            lines.push(format!("app dir: {}", app_dir.display()));
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

pub(crate) fn resolve_start_app_dir(
    command: &StartCommand,
) -> Result<Option<ResolvedStartAppDir>, Error> {
    let Some(explicit_app_dir) = command.app_dir.as_deref() else {
        // `nimbus start` does no source-tree discovery. Without
        // `--app-dir`, the daemon starts at generation 0 and waits for
        // deploys to arrive through the admin API. Deploy records
        // persist in `_nimbus.bundles`, but auto-activating a
        // previously deployed bundle on startup is not yet wired —
        // see CD7(j) in the cli-daemon-canonicalization plan. CD1.
        return Ok(None);
    };
    let cwd = std::env::current_dir().map_err(|error| {
        Error::Internal(format!("failed to determine current directory: {error}"))
    })?;
    let resolved = resolve_deploy_app_dir(Some(explicit_app_dir), &cwd)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    if !app_dir_has_convex_surface(&resolved) && !app_dir_has_cloud_functions_surface(&resolved) {
        return Err(Error::InvalidInput(format!(
            "No Convex or Cloud Functions surface found in {}.\n\n\
             Create a `convex/` or `nimbus/` source directory, a `firebase.json`, \
             or a Functions Framework `package.json` and place your app functions there.",
            resolved.display()
        )));
    }
    Ok(Some(ResolvedStartAppDir::Explicit(resolved)))
}

fn local_listen_url(addr: SocketAddr, tls_enabled: bool) -> String {
    let host = if addr.ip().is_unspecified() {
        "localhost".to_string()
    } else if addr.ip().is_ipv6() {
        format!("[{}]", addr.ip())
    } else {
        addr.ip().to_string()
    };
    let scheme = if tls_enabled { "https" } else { "http" };
    format!("{scheme}://{host}:{}/", addr.port())
}

async fn start_listener(command: &StartCommand) -> std::io::Result<tokio::net::TcpListener> {
    if command.systemd_socket_activation {
        activated_systemd_listener()
    } else {
        tokio::net::TcpListener::bind((command.host.as_str(), command.port)).await
    }
}

#[cfg(unix)]
fn activated_systemd_listener() -> std::io::Result<tokio::net::TcpListener> {
    let listen_fds = env::var("LISTEN_FDS")
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "LISTEN_FDS is not set for systemd socket activation",
            )
        })?
        .parse::<i32>()
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("LISTEN_FDS is invalid: {error}"),
            )
        })?;
    if listen_fds != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("expected exactly one inherited listener, got LISTEN_FDS={listen_fds}"),
        ));
    }
    let listen_pid = env::var("LISTEN_PID")
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "LISTEN_PID is not set for systemd socket activation",
            )
        })?
        .parse::<u32>()
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("LISTEN_PID is invalid: {error}"),
            )
        })?;
    if listen_pid != process::id() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "LISTEN_PID={listen_pid} does not match current process {}",
                process::id()
            ),
        ));
    }
    // SAFETY: systemd socket activation guarantees inherited file descriptors
    // begin at 3 when LISTEN_FDS is set for the current process. The method
    // consumes fd 3 exactly once and transfers ownership to TcpListener.
    let listener = unsafe { std::net::TcpListener::from_raw_fd(3) };
    listener.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(listener)
}

#[cfg(not(unix))]
fn activated_systemd_listener() -> std::io::Result<tokio::net::TcpListener> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "systemd socket activation is only supported on Unix hosts",
    ))
}

/// Build the operator-console URL by appending `/ui/` to the daemon's base
/// URL. Discovery callers keep using `local_listen_url`; the banner adds the
/// path. Mirrors the CockroachDB `webui:\t<url>` precedent (see CD3 in the
/// canonicalization plan).
fn operator_console_url_from_base(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    format!("{trimmed}/ui/")
}

fn app_dir_has_convex_surface(app_dir: &Path) -> bool {
    app_dir.join("convex").is_dir()
        || app_dir.join("nimbus").is_dir()
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
        .join(".nimbus")
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
        .join(".nimbus")
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
    engine: &Engine,
    tenant_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let tenant_id = TenantId::new(tenant_name)?;
    match engine.create_tenant(tenant_id) {
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
            "Run \"nimbus codegen --app {}\" to generate it, or remove --skip-codegen to generate manifests automatically on start.",
            app_dir.display()
        )
    } else {
        format!(
            "Run \"nimbus codegen --app {}\" to generate it before retrying.",
            app_dir.display()
        )
    }
}

fn cloud_functions_manifest_recovery_hint(app_dir: &Path, skip_codegen: bool) -> String {
    if skip_codegen {
        format!(
            "Run \"nimbus codegen --app {}\" to generate it, or remove --skip-codegen to generate manifests automatically on start.",
            app_dir.display()
        )
    } else {
        format!(
            "Run \"nimbus codegen --app {}\" to generate it before retrying.",
            app_dir.display()
        )
    }
}

pub(super) fn resolve_license_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if std::env::var_os(nimbus::LICENSE_FILE_ENV).is_some() {
        return None;
    }
    let config_dir = dirs::global_config_dir().ok()?;
    let default_path = config_dir.join("license.json");
    default_path.exists().then_some(default_path)
}
