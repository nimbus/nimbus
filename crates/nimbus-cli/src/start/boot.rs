use std::net::SocketAddr;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(unix)]
use std::{env, process};

use nimbus::{
    Engine, Error, LicenseState, RuntimeHostResourceBudget, RuntimeLimits, TenantAdmissionOutcome,
    TenantId, run_scheduler,
};
use nimbus_convex::{ConvexRegistry, ConvexSiloAuthRegistry};
use nimbus_operator::{
    LocalServerPaths, LocalServerSecurityState, load_or_create_local_admin_token,
};
use nimbus_server::{
    CloudFunctionsRegistry, LeasedServerListener, RecordedListenerBindFailure, ServeOptions,
    ServerDiscoveryLease, serve_leased,
};
use nimbus_tenant::OperatorPolicyDocument;

use super::StartCommand;
use super::adapters::{AdapterEnablement, resolve_adapter_enablement};
use super::config::{
    control_data_dir_from_persistence_config, network_root_from_start_command,
    persistence_config_from_start_command, runtime_config_from_start_command,
};
use super::first_boot::{is_first_boot, spawn_first_boot_announce};
use super::network_bind::{
    ensure_admin_token_rotated_for_public_bind, ensure_firebase_bypass_loopback_only,
    ensure_host_opt_in,
};
use super::runtime_limits::{
    runtime_adaptive_controller_settings_from_command, runtime_host_resource_budget_from_command,
    runtime_limits_from_command,
};
use super::shutdown::{ProcessShutdownSignals, serve_until_shutdown};
use crate::cli_ux;
use crate::codegen::{CodegenOptions, run_codegen_for_app_dir_with_options};
use crate::compose::discovery::{
    ResolvedComposeSelection, compose_selection_summary, resolve_explicit_compose_selection,
};
use crate::deploy::resolve_deploy_app_dir;
use crate::dirs;
use crate::function_scaling::{
    FunctionScalingAdmissionEnvelope, FunctionScalingAdmissionSet, FunctionScalingContext,
    FunctionScalingFileConfig, admit_function_scaling_plans, load_optional_policy,
    resolve_function_scaling_intent,
};
use crate::network_composition::{PreparedLocalNetworkComposition, StagedLocalNetworkComposition};

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
    // Reject an unsafe explicit bind before claiming the process-wide local
    // network authority. A different Nimbus process can hold that authority;
    // waiting on it must never hide a deterministic CLI policy error.
    if !command.systemd_socket_activation {
        ensure_host_opt_in(&command.host, command.allow_network)?;
    }
    let network_root = network_root_from_start_command(&command)?;
    let staged_network = StagedLocalNetworkComposition::claim(&network_root)?;
    run_start_command_with_network(command, StartNetworkComposition::Staged(staged_network)).await
}

pub(crate) async fn run_start_command_with_prepared_network(
    command: StartCommand,
    prepared_network: PreparedLocalNetworkComposition,
) -> Result<(), Box<dyn std::error::Error>> {
    run_start_command_with_network(
        command,
        StartNetworkComposition::Prepared(Box::new(prepared_network)),
    )
    .await
}

enum StartNetworkComposition {
    Staged(StagedLocalNetworkComposition),
    Prepared(Box<PreparedLocalNetworkComposition>),
}

async fn run_start_command_with_network(
    mut command: StartCommand,
    network: StartNetworkComposition,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut prebound_wire_listeners = command.prebound_wire_listeners.take();
    let result = run_start_command_inner(command, &mut prebound_wire_listeners, network).await;
    finish_prebound_listener_ownership(result, prebound_wire_listeners)
}

async fn run_start_command_inner(
    command: StartCommand,
    prebound_wire_listeners: &mut Option<nimbus_server::PreboundServerListeners>,
    network: StartNetworkComposition,
) -> Result<(), Box<dyn std::error::Error>> {
    if let StartNetworkComposition::Prepared(prepared) = &network {
        let requested_root = network_root_from_start_command(&command)?;
        prepared.authenticate_requested_root(&requested_root)?;
    }
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
    let runtime_config = runtime_config_from_start_command(&command)?;
    let compose_control_data_dir =
        control_data_dir_from_persistence_config(&persistence_config).to_path_buf();
    let resolved_app_dir = resolve_start_app_dir(&command)?;
    // Adapter enablement resolves after the control data dir: default-on
    // listeners without operator credentials load (or generate) theirs
    // from the wire-credential store under that dir. App-dir-backed adapters
    // may also inspect local framework config such as wrangler.*.
    let adapter_enablement = resolve_adapter_enablement(
        &command,
        &compose_control_data_dir,
        resolved_app_dir.as_ref().map(ResolvedStartAppDir::path),
    )?;
    // Refuse the Firebase dev-mode token-verification bypass on a non-loopback
    // bind: it fabricates verified Firebase projects from unsigned emulator
    // tokens (#24), so it must be unreachable over the network by construction.
    // Mirrors `ensure_host_opt_in`; the systemd-activation path re-checks against
    // the activated host below.
    let firebase_bypass_enabled = adapter_enablement
        .firebase
        .as_ref()
        .is_some_and(|firebase| firebase.allows_emulator_token_verification_bypass());
    if !command.systemd_socket_activation {
        ensure_firebase_bypass_loopback_only(&command.host, firebase_bypass_enabled)?;
    }
    // Snapshot first-boot before `Engine::new_with_persistence_config`
    // touches the data dir; otherwise the marker landscape we observe
    // would always say "second boot" because Engine initialization
    // would have already populated the dir. The H5 banner is fired
    // after the listener is up and the discovery lease is held so the
    // launch ticket can mint against the live server.
    let is_first_boot_run = is_first_boot(&compose_control_data_dir);
    run_codegen_preflight(&command, resolved_app_dir.as_ref()).await?;
    let runtime_limits = runtime_limits_from_command(&command);
    let runtime_host_resource_budget = runtime_host_resource_budget_from_command(&command);
    let operator_policy = load_optional_policy(command.policy.clone())?;
    let function_scaling_admission = admit_start_function_scaling_plans(
        &command,
        &runtime_config,
        &runtime_limits,
        runtime_host_resource_budget,
        operator_policy.as_ref(),
    )?;
    let effective_runtime_scaling_plans = function_scaling_admission.plans.clone();
    let runtime_adaptive_controller_settings =
        runtime_adaptive_controller_settings_from_command(&command);
    let license_file = resolve_license_path(command.license_file.as_deref());
    let license_state = LicenseState::load(license_file.as_deref())?;
    let license_snapshot = license_state.snapshot();
    let deploy_admin_enabled =
        command.deploy_admin_token.is_some() || std::env::var_os("NIMBUS_DEPLOY_TOKEN").is_some();
    let convex_registry =
        load_convex_registry(&command, resolved_app_dir.as_ref(), &runtime_limits)?;
    let cloud_functions_registry =
        load_cloud_functions_registry(&command, resolved_app_dir.as_ref(), &runtime_limits)?;
    super::adapters::cloud_functions::ensure_http_targets_are_bound(
        cloud_functions_registry.as_ref(),
        adapter_enablement.cloud_functions_http_tenant.as_ref(),
    )?;
    let compose_selection = resolve_optional_compose_selection(&command)?;
    let workload_boot_plan = match compose_selection.as_ref() {
        Some(selection) => Some(crate::workload_boot::plan_compose_services(
            selection,
            &compose_control_data_dir,
            command.tenant_isolation_mode,
            &crate::workload_boot::default_local_node_capacity()?,
        )?),
        None => None,
    };
    if let Some(plan) = &workload_boot_plan {
        tracing::info!(
            tenant_id = %plan.tenant_id(),
            tenant_isolation_mode = plan.tenant_isolation_mode().as_str(),
            compose_files = plan.compose_file_count(),
            desired_workloads = plan.desired_workload_count(),
            placement_plans = plan.placement_plan_count(),
            "resolved compose workload-control boot plan"
        );
    }
    let server_workload_boot_plan = workload_boot_plan
        .map(crate::workload_boot::WorkloadControlBootPlan::into_server_plan)
        .transpose()?;
    let source_ingress = nimbus_server::nimbus_owned_workload_ingress_registration();
    let prepared_network = match network {
        StartNetworkComposition::Staged(staged) => PreparedLocalNetworkComposition::prepare(
            staged,
            compose_selection.as_ref(),
            &compose_control_data_dir,
            command.tenant_isolation_mode,
            source_ingress.clone(),
        )?,
        StartNetworkComposition::Prepared(prepared) => {
            prepared.validate_start_context(
                compose_selection.as_ref(),
                &compose_control_data_dir,
                command.tenant_isolation_mode,
                &source_ingress,
            )?;
            *prepared
        }
    };
    // Resolve the workload profile before Engine construction or any listener
    // effect. A forwarded-machine profile retains its exact effect-free source
    // here and activates that source only after the caller constructs Engine.
    let prepared_network_authority = prepared_network.authority();
    let prepared_server_profile = prepared_network.prepare_server_workload_profile()?;
    let managed_workload_profile = prepared_server_profile.is_managed();
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
    let local_server_security = Arc::new(LocalServerSecurityState::new(
        local_server_paths.clone(),
        local_admin_token,
    ));
    let engine = Arc::new(Engine::new_with_persistence_config(persistence_config).await?);
    let shutdown_engine = engine.clone();
    engine.recover_scheduled_work_on_startup_async().await?;
    if let Some(tenant_name) = &command.auto_tenant {
        ensure_auto_tenant(&engine, tenant_name).await?;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let scheduler_engine = engine.clone();
    let scheduler_handle = tokio::spawn(async move {
        run_scheduler(scheduler_engine, shutdown_rx).await;
    });
    let convex_silo_auth =
        convex_registry
            .as_ref()
            .map_or_else(ConvexSiloAuthRegistry::new, |registry| {
                let verifier = Arc::new(registry.clone());
                adapter_enablement
                    .convex_auth_silos
                    .iter()
                    .cloned()
                    .fold(ConvexSiloAuthRegistry::new(), |bindings, silo| {
                        bindings.bind(&silo, verifier.clone())
                    })
            });
    let mut serve_options = prepared_server_profile
        .complete(engine.clone())?
        .with_license(license_state)
        .with_runtime_host_resource_budget(runtime_host_resource_budget)
        .with_runtime_adaptive_controller_settings(runtime_adaptive_controller_settings)
        .with_effective_runtime_scaling_plans(effective_runtime_scaling_plans);
    if let Some(plan) = server_workload_boot_plan {
        serve_options = serve_options.with_workload_boot_plan(plan);
    }
    if let Some(listeners) = prebound_wire_listeners.as_ref() {
        serve_options = serve_options.with_prebound_listener_authority(listeners)?;
    }
    if let Some(registry) = convex_registry {
        serve_options = serve_options
            .with_convex_registry(registry)
            .with_convex_silo_auth(convex_silo_auth);
    }
    if let Some(registry) = cloud_functions_registry {
        serve_options = serve_options.with_cloud_functions_registry(registry);
    }
    if let Some(token) = command.deploy_admin_token.clone() {
        serve_options = serve_options.with_deploy_admin_token(token);
    }
    serve_options = serve_options.with_local_server_security(Arc::clone(&local_server_security));
    serve_options = serve_options.with_tenant_isolation_mode(command.tenant_isolation_mode);
    serve_options = serve_options.with_cors_allowed_origins(cors_allowed_origins);
    if let Some(tls_config) = tls_config {
        serve_options = serve_options.with_tls(tls_config);
    }
    serve_options = adapter_enablement.clone().apply_to(serve_options);

    // Validate the actual inherited bind before machine-lifecycle
    // construction, so a refused public systemd socket fails before any
    // machine-owned provider is consulted.
    let activated_listener = if command.systemd_socket_activation {
        let listener = activated_systemd_listener()?;
        let activated_host = listener.local_addr()?.ip().to_string();
        ensure_host_opt_in(&activated_host, command.allow_network)?;
        ensure_firebase_bypass_loopback_only(&activated_host, firebase_bypass_enabled)?;
        let rotation_warning = ensure_admin_token_rotated_for_public_bind(
            &activated_host,
            &local_server_security.current_token(),
            time::OffsetDateTime::now_utc(),
        )?;
        emit_rotation_warning(rotation_warning);
        Some(listener)
    } else {
        None
    };

    if managed_workload_profile {
        let machine_lifecycle_manager = crate::machine::host_machine_lifecycle_manager(
            prepared_network_authority,
            Arc::clone(&engine),
        )?;
        serve_options = serve_options.with_machine_lifecycle_manager(machine_lifecycle_manager);
    }

    let listener = match activated_listener {
        Some(listener) => serve_options.adopt_external_main_listener(listener)?,
        None => start_listener(&command, &serve_options).await?,
    };
    let listener_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(error) => {
            return Err(Box::new(close_listener_after_startup_error(
                listener,
                error,
                "failed to settle the main listener after its bound address could not be observed",
            )));
        }
    };
    let discovery_lease = match ServerDiscoveryLease::acquire(&local_server_paths, listener_addr) {
        Ok(lease) => lease,
        Err(error) => {
            return Err(Box::new(close_listener_after_startup_error(
                listener,
                error,
                "failed to settle the main listener after discovery acquisition failed",
            )));
        }
    };
    let server_shutdown = serve_options.shutdown_handle();
    let shutdown_signals = ProcessShutdownSignals::install()?;
    if let Some(listeners) = prebound_wire_listeners.take() {
        serve_options = serve_options.with_prebound_wire_listeners(listeners)?;
    }
    emit_start_startup_summary(
        &command,
        resolved_app_dir.as_ref(),
        compose_selection.as_ref(),
        &adapter_enablement,
        listener_addr,
        deploy_admin_enabled,
    );
    let first_boot_handle = if is_first_boot_run {
        let console_url =
            operator_console_url_from_base(&local_listen_url(listener_addr, tls_enabled));
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

    tracing::info!("nimbus listening on {listener_addr}");
    let server_result = serve_until_shutdown(
        serve_leased(listener, serve_options),
        shutdown_signals,
        server_shutdown,
    )
    .await;
    drop(discovery_lease);
    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
    if let Some(handle) = first_boot_handle {
        handle.abort();
        let _ = handle.await;
    }
    shutdown_engine.quiesce().await;
    drop(prepared_network);
    server_result.map_err(|error| conventional_wire_port_guidance(&command, error))?;
    Ok(())
}

pub(super) fn conventional_wire_port_guidance(
    command: &StartCommand,
    error: std::io::Error,
) -> std::io::Error {
    if error.kind() != std::io::ErrorKind::AddrInUse {
        return error;
    }
    let message = error.to_string();
    let conventional_conflict = [
        (
            "mongodb",
            "MongoDB",
            command.mongodb,
            command.mongodb_port,
            super::adapters::MONGODB_CONVENTIONAL_PORT,
            "--mongodb-port",
            "--no-mongodb",
        ),
        (
            "dynamodb",
            "DynamoDB",
            command.dynamodb,
            command.dynamodb_port,
            super::adapters::DYNAMODB_CONVENTIONAL_PORT,
            "--dynamodb-port",
            "--no-dynamodb",
        ),
        (
            "s3",
            "S3",
            command.s3,
            command.s3_port,
            super::adapters::S3_CONVENTIONAL_PORT,
            "--s3-port",
            "--no-s3",
        ),
    ]
    .into_iter()
    .find(|(adapter, _, enabled, explicit_port, conventional, _, _)| {
        *enabled
            && explicit_port.is_none()
            && message.contains(&format!("{adapter} listener"))
            && message.contains(&format!(":{conventional}"))
    });
    let Some((_, display_name, _, _, conventional, port_flag, disable_flag)) =
        conventional_conflict
    else {
        return error;
    };
    std::io::Error::new(
        error.kind(),
        format!(
            "{display_name} conventional port {conventional} is busy; pass {port_flag} to serve \
             on another port or {disable_flag} to disable the listener: {error}"
        ),
    )
}

fn finish_prebound_listener_ownership(
    result: Result<(), Box<dyn std::error::Error>>,
    listeners: Option<nimbus_server::PreboundServerListeners>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(listeners) = listeners else {
        return result;
    };
    let cleanup = listeners.close_and_settle();
    match (result, cleanup) {
        (Err(primary), Ok(())) => Err(primary),
        (Err(primary), Err(cleanup_error)) => Err(Box::new(std::io::Error::new(
            cleanup_error.kind(),
            format!(
                "{primary}; failed to settle pre-bound dev listeners after startup failed: \
                 {cleanup_error}"
            ),
        ))),
        (Ok(()), Ok(())) => Err(Box::new(std::io::Error::other(
            "startup completed without transferring its pre-bound dev listeners",
        ))),
        (Ok(()), Err(cleanup_error)) => Err(Box::new(std::io::Error::new(
            cleanup_error.kind(),
            format!(
                "startup completed without transferring its pre-bound dev listeners; \
                 failed to settle them: {cleanup_error}"
            ),
        ))),
    }
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
    let runtime_host_resource_budget = runtime_host_resource_budget_from_command(command);
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
        runtime_host_budget_summary_line(runtime_host_resource_budget),
        default_function_scaling_summary_line(command),
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

fn default_function_scaling_summary_line(command: &StartCommand) -> String {
    let context = function_scaling_context_from_command(command);
    let intent = resolve_function_scaling_intent(
        &FunctionScalingFileConfig::default(),
        context,
        "__default__",
    )
    .expect("baked function scaling defaults should always resolve");
    intent.boot_summary(context)
}

pub(super) fn admit_start_function_scaling_plans(
    command: &StartCommand,
    runtime_config: &crate::start::RuntimeConfigFile,
    runtime_limits: &RuntimeLimits,
    runtime_host_resource_budget: RuntimeHostResourceBudget,
    operator_policy: Option<&OperatorPolicyDocument>,
) -> Result<FunctionScalingAdmissionSet, Error> {
    admit_function_scaling_plans(
        &runtime_config.functions.scaling,
        function_scaling_context_from_command(command),
        operator_policy,
        command.auto_tenant.as_deref(),
        FunctionScalingAdmissionEnvelope::from_host_budget(
            runtime_host_resource_budget,
            runtime_limits.max_warm_pool_entries_per_worker,
        ),
    )
}

fn function_scaling_context_from_command(command: &StartCommand) -> FunctionScalingContext {
    if matches!(
        command.tenant_isolation_mode,
        nimbus_tenant::TenantIsolationMode::LocalDevelopment
    ) {
        FunctionScalingContext::Dev
    } else {
        FunctionScalingContext::Start
    }
}

fn runtime_host_budget_summary_line(budget: RuntimeHostResourceBudget) -> String {
    let hard_ceiling = budget
        .runtime_hard_ceiling_millicpus
        .map(|ceiling| format!("{ceiling}m"))
        .unwrap_or_else(|| "none".to_string());
    format!(
        "runtime host budget:\t{}m allocatable CPU ({}m host - {}m system reserve - {}m Nimbus control-plane reserve; hard ceiling {hard_ceiling}; seat {}m)",
        budget.runtime_allocatable_millicpus(),
        budget.host_millicpus,
        budget.system_reserved_millicpus,
        budget.nimbus_control_plane_reserved_millicpus,
        budget.runtime_seat_millicpus.get()
    )
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

async fn start_listener(
    command: &StartCommand,
    serve_options: &ServeOptions,
) -> std::io::Result<LeasedServerListener> {
    let candidates = tokio::net::lookup_host((command.host.as_str(), command.port)).await?;
    let mut last_error = None;
    for requested_addr in candidates {
        let prepared = match serve_options.prepare_main_listener(requested_addr) {
            Ok(prepared) => prepared,
            Err(error) => {
                retain_candidate_preparation_failure(&mut last_error, error)?;
                continue;
            }
        };
        match tokio::net::TcpListener::bind(requested_addr).await {
            Ok(listener) => return prepared.adopt(listener),
            Err(error) => {
                retain_recorded_candidate_failure(
                    &mut last_error,
                    prepared.record_bind_failure(error),
                )?;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!(
                "host `{}` resolved no usable TCP addresses for port {}",
                command.host, command.port
            ),
        )
    }))
}

fn retain_candidate_preparation_failure(
    last_error: &mut Option<std::io::Error>,
    error: std::io::Error,
) -> std::io::Result<()> {
    if error.kind() == std::io::ErrorKind::AddrInUse {
        *last_error = Some(error);
        Ok(())
    } else {
        Err(error)
    }
}

fn retain_recorded_candidate_failure(
    last_error: &mut Option<std::io::Error>,
    receipt: Result<RecordedListenerBindFailure, std::io::Error>,
) -> std::io::Result<()> {
    match receipt {
        Ok(recorded_bind_failure) => {
            *last_error = Some(recorded_bind_failure.into_error());
            Ok(())
        }
        Err(authority_error) => Err(authority_error),
    }
}

fn close_listener_after_startup_error(
    listener: LeasedServerListener,
    primary: std::io::Error,
    context: &str,
) -> std::io::Error {
    match listener.close_and_settle() {
        Ok(()) => primary,
        Err(cleanup_error) => std::io::Error::new(
            primary.kind(),
            format!("{primary}; {context}: {cleanup_error}"),
        ),
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

pub(super) async fn ensure_auto_tenant(
    engine: &Arc<Engine>,
    tenant_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let tenant_id = TenantId::new(tenant_name)?;
    match engine.ensure_tenant_ready_async(tenant_id).await? {
        TenantAdmissionOutcome::Created => {
            emit_start_info(format!("auto-created tenant \"{tenant_name}\""));
        }
        TenantAdmissionOutcome::Existing => {}
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

#[cfg(test)]
mod listener_tests {
    use nimbus_network::{
        LocalPortLeaseAuthority, PortBindFailureKind, PortBindingProvenance, PortLeasePhase,
    };
    use nimbus_process_harness::PortWindow;

    use super::*;

    #[test]
    #[serial_test::serial]
    fn divergent_prepared_start_root_settles_dev_listener_before_other_effects() {
        std::thread::Builder::new()
            .name("divergent-prepared-start-root".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(divergent_prepared_start_root_case)
            .expect("divergent-root test thread should start")
            .join()
            .expect("divergent-root test thread should not panic");
    }

    fn divergent_prepared_start_root_case() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("divergent-root test runtime should build")
            .block_on(divergent_prepared_start_root_case_async());
    }

    async fn divergent_prepared_start_root_case_async() {
        let root = tempfile::tempdir().expect("fixture root should exist");
        let source_path = root.path().join("source-node");
        let foreign_path = root.path().join("foreign-node");
        let source_root =
            nimbus_operator::LocalNodeNetworkRoot::resolve_for_current_platform(Some(&source_path))
                .expect("source node root should resolve");
        let staged = StagedLocalNetworkComposition::claim(&source_root)
            .expect("source manager should claim");
        let mut listeners = nimbus_server::PreboundServerListeners::new(staged.authority());
        // The window holds this port for the rest of the case, so the closing
        // re-bind proves the rejected handoff released its socket instead of
        // racing an unrelated process for a freed ephemeral number.
        let port_window = PortWindow::claim();
        let requested_addr =
            std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port_window.port(0)));
        let prepared_listener = listeners
            .prepare("dev-mongodb-conventional", requested_addr)
            .expect("source listener should reserve before bind");
        let raw = std::net::TcpListener::bind(requested_addr).expect("source listener should bind");
        let listener = prepared_listener
            .adopt_std(raw)
            .expect("source listener should activate");
        let actual_addr = listener
            .local_addr()
            .expect("source listener address should resolve");
        listeners
            .insert("mongodb", listener)
            .expect("source listener should enter the handoff bundle");
        let prepared_network = PreparedLocalNetworkComposition::prepare(
            staged,
            None,
            &root.path().join("control"),
            nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            nimbus_server::nimbus_owned_workload_ingress_registration(),
        )
        .expect("source composition should freeze empty");
        let command = StartCommand {
            network_state_dir: Some(foreign_path.clone()),
            prebound_wire_listeners: Some(listeners),
            ..StartCommand::default()
        };

        let error = run_start_command_with_prepared_network(command, prepared_network)
            .await
            .expect_err("a divergent prepared start root must fail before startup effects");
        let rendered = error.to_string();
        assert!(
            rendered.contains("prepared local network authority")
                && rendered.contains("active")
                && rendered.contains(&source_path.display().to_string())
                && rendered.contains(&foreign_path.display().to_string()),
            "typed root evidence should remain actionable: {rendered}"
        );
        assert!(
            !foreign_path.exists(),
            "divergent root validation must precede attempted-root mutation"
        );
        std::net::TcpListener::bind(actual_addr)
            .expect("rejected dev handoff must close and settle its held listener");
    }

    #[test]
    fn listener_tls_does_not_change_workload_ingress_provider_evidence() {
        let plain_listener_evidence = nimbus_server::nimbus_owned_workload_ingress_registration();
        let _listener_tls = nimbus_server::TlsConfig::new("fixture-cert.pem", "fixture-key.pem");
        let tls_listener_evidence = nimbus_server::nimbus_owned_workload_ingress_registration();

        assert_eq!(plain_listener_evidence, tls_listener_evidence);
    }

    #[test]
    #[serial_test::serial]
    fn crossed_prepared_source_fails_before_engine_or_listener_effects() {
        std::thread::Builder::new()
            .name("crossed-prepared-start-source".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(crossed_prepared_source_case)
            .expect("crossed-source test thread should start")
            .join()
            .expect("crossed-source test thread should not panic");
    }

    fn crossed_prepared_source_case() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crossed-source test runtime should build")
            .block_on(crossed_prepared_source_case_async());
    }

    async fn crossed_prepared_source_case_async() {
        let root = tempfile::tempdir().expect("fixture root should exist");
        let network_path = root.path().join("source-node");
        let source_control = root.path().join("source-control");
        let requested_control = root.path().join("requested-control");
        let engine_data = root.path().join("engine-data");
        let network_root = nimbus_operator::LocalNodeNetworkRoot::resolve_for_current_platform(
            Some(&network_path),
        )
        .expect("source node root should resolve");
        let staged = StagedLocalNetworkComposition::claim(&network_root)
            .expect("source manager should claim");
        let prepared_network = PreparedLocalNetworkComposition::prepare(
            staged,
            None,
            &source_control,
            nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            nimbus_server::nimbus_owned_workload_ingress_registration(),
        )
        .expect("source composition should freeze empty");
        let command = StartCommand {
            data_dir: Some(engine_data.clone()),
            control_data_dir: Some(requested_control),
            network_state_dir: Some(network_path),
            tenant_provider: Some(crate::start::CliTenantProvider::Sqlite),
            tenant_isolation_mode: nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            ..StartCommand::default()
        };

        let error = run_start_command_with_prepared_network(command, prepared_network)
            .await
            .expect_err("a crossed prepared source must fail before startup effects");
        assert!(
            error
                .to_string()
                .contains("control-data root changed after capability freeze"),
            "typed source mismatch should remain actionable: {error}"
        );
        assert!(
            !engine_data.exists(),
            "source validation must precede Engine persistence effects"
        );
    }

    #[tokio::test]
    async fn start_listener_activates_provider_assigned_lease_for_cli_owned_bind() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let engine = Arc::new(Engine::new(state_root.path()).expect("engine should initialize"));
        let options = ServeOptions::reconstruct_direct(engine).expect("test authority should open");
        let command = StartCommand {
            host: "127.0.0.1".to_owned(),
            port: 0,
            ..StartCommand::default()
        };

        let listener = start_listener(&command, &options)
            .await
            .expect("CLI-owned listener should bind through durable authority");
        let actual_addr = listener
            .local_addr()
            .expect("leased listener address should resolve");
        let records = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should open")
            .list()
            .expect("port records should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Active);
        let binding = records[0]
            .binding()
            .expect("Active lease should retain binding evidence");
        assert_eq!(binding.actual_port().get(), actual_addr.port());
        assert_eq!(
            binding.provenance(),
            PortBindingProvenance::ProviderAssigned
        );
    }

    #[tokio::test]
    async fn start_listener_records_exact_addr_in_use_failure_without_serving() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let external = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("external owner should bind");
        let occupied_addr = external
            .local_addr()
            .expect("external address should resolve");
        let engine = Arc::new(Engine::new(state_root.path()).expect("engine should initialize"));
        let options = ServeOptions::reconstruct_direct(engine).expect("test authority should open");
        let command = StartCommand {
            host: occupied_addr.ip().to_string(),
            port: occupied_addr.port(),
            ..StartCommand::default()
        };

        let error = match start_listener(&command, &options).await {
            Ok(_) => panic!("external owner should win the exact kernel bind"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
        let records = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should open")
            .list()
            .expect("port records should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Failed);
        assert_eq!(
            records[0]
                .failure()
                .expect("failed bind should retain evidence")
                .kind(),
            PortBindFailureKind::AddrInUse
        );
    }

    #[tokio::test]
    async fn startup_error_closes_and_releases_cli_owned_listener() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let engine = Arc::new(Engine::new(state_root.path()).expect("engine should initialize"));
        let options = ServeOptions::reconstruct_direct(engine).expect("test authority should open");
        // A claimed port rather than a provider-assigned one, so the re-bind at
        // the end measures the CLI's release and nothing else: no other process
        // can be holding this number when the assertion runs.
        let port_window = PortWindow::claim();
        let command = StartCommand {
            host: "127.0.0.1".to_owned(),
            port: port_window.port(0),
            ..StartCommand::default()
        };
        let listener = start_listener(&command, &options)
            .await
            .expect("CLI-owned listener should activate");
        let actual_addr = listener
            .local_addr()
            .expect("leased listener address should resolve");

        let error = close_listener_after_startup_error(
            listener,
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "discovery record already exists",
            ),
            "failed discovery cleanup",
        );
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        let records = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Released);
        tokio::net::TcpListener::bind(actual_addr)
            .await
            .expect("confirmed startup failure must close the CLI-owned socket");
    }

    #[test]
    fn startup_error_closes_and_releases_untransferred_prebound_listeners() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let mut listeners =
            nimbus_server::PreboundServerListeners::reconstruct_direct(state_root.path())
                .expect("test authority should open");
        // The window holds this port for the rest of the test, so the closing
        // re-bind proves the untransferred listener was really closed rather
        // than racing an unrelated process for a freed ephemeral number.
        let port_window = PortWindow::claim();
        let requested_addr =
            std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port_window.port(0)));
        let prepared = listeners
            .prepare("dev-mongodb-conventional", requested_addr)
            .expect("pre-bound listener should reserve");
        let raw = std::net::TcpListener::bind(requested_addr)
            .expect("provider should bind its requested socket");
        let listener = prepared
            .adopt_std(raw)
            .expect("pre-bound listener should activate");
        let actual_addr = listener
            .local_addr()
            .expect("pre-bound address should resolve");
        listeners
            .insert("mongodb", listener)
            .expect("listener should enter the handoff bundle");

        let result: Result<(), Box<dyn std::error::Error>> = Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "startup validation failed",
        )));
        let error = finish_prebound_listener_ownership(result, Some(listeners))
            .expect_err("the primary startup failure must remain visible");
        assert!(error.to_string().contains("startup validation failed"));

        let records = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Released);
        std::net::TcpListener::bind(actual_addr)
            .expect("startup failure must close every untransferred pre-bound socket");
    }

    #[test]
    fn authority_receipt_failure_aborts_candidate_fallback() {
        let mut last_error = Some(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            "earlier recorded candidate",
        ));
        let error = retain_recorded_candidate_failure(
            &mut last_error,
            Err(std::io::Error::other("authority receipt failed")),
        )
        .expect_err("authority failure must stop hostname candidate iteration");
        assert_eq!(error.to_string(), "authority receipt failed");
        assert_eq!(
            last_error
                .expect("the earlier recorded bind diagnostic should remain visible")
                .kind(),
            std::io::ErrorKind::AddrInUse
        );
    }

    #[test]
    fn authority_preparation_failure_aborts_candidate_fallback() {
        let mut last_error = Some(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            "earlier safe candidate conflict",
        ));
        let error = retain_candidate_preparation_failure(
            &mut last_error,
            std::io::Error::other("bind-claim authority receipt failed"),
        )
        .expect_err("authority preparation failure must stop candidate iteration");
        assert_eq!(error.to_string(), "bind-claim authority receipt failed");
        assert_eq!(
            last_error
                .expect("the earlier safe conflict should remain visible")
                .kind(),
            std::io::ErrorKind::AddrInUse
        );
    }

    #[test]
    fn durable_port_conflict_may_try_the_next_hostname_candidate() {
        let mut last_error = None;
        retain_candidate_preparation_failure(
            &mut last_error,
            std::io::Error::new(std::io::ErrorKind::AddrInUse, "durable port conflict"),
        )
        .expect("a proven conflict has no partial preparation to reconcile");
        assert_eq!(
            last_error
                .expect("the conflict should remain as the fallback diagnostic")
                .kind(),
            std::io::ErrorKind::AddrInUse
        );
    }
}
