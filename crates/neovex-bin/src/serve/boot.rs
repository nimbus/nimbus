use std::path::{Path, PathBuf};
use std::sync::Arc;

use neovex::{
    ConvexRegistry, Error, LicenseState, SandboxCatalog, Service, run_scheduler,
    serve_with_convex_and_license, serve_with_convex_and_license_and_sandbox_service_manager,
    serve_with_license, serve_with_license_and_sandbox_catalog,
};

use super::ServeCommand;
use super::config::{
    control_data_dir_from_service_config, service_persistence_config_from_serve_command,
};
use super::runtime_limits::runtime_limits_from_command;
use crate::cli_ux;
use crate::codegen::run_codegen_for_app_dir;
use crate::service::load_host_backed_sandbox_service_manager;

pub(crate) async fn run_serve_command(
    command: ServeCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let service_config = service_persistence_config_from_serve_command(&command)?;
    let compose_control_data_dir =
        control_data_dir_from_service_config(&service_config).to_path_buf();
    run_codegen_preflight(&command).await?;
    let runtime_limits = runtime_limits_from_command(&command);
    let license_state = LicenseState::load(command.license_file.as_deref())?;
    let license_snapshot = license_state.snapshot();
    let convex_registry = load_convex_registry(&command, &runtime_limits)?;
    let sandbox_service_manager =
        load_sandbox_service_manager(&command, &compose_control_data_dir)?;
    let service = Arc::new(Service::new_with_persistence_config(service_config).await?);
    let shutdown_service = service.clone();
    service.recover_scheduled_work_on_startup_async().await?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let scheduler_service = service.clone();
    let scheduler_handle = tokio::spawn(async move {
        run_scheduler(scheduler_service, shutdown_rx).await;
    });
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", command.port)).await?;

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
    let server_result = serve_with_optional_runtime_and_services(
        listener,
        service,
        convex_registry,
        license_state,
        sandbox_service_manager,
    )
    .await;
    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
    shutdown_service.quiesce().await;
    server_result?;
    Ok(())
}

pub(super) async fn run_codegen_preflight(
    command: &ServeCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(app_dir) = command.app_dir.as_deref() else {
        return Ok(());
    };
    if command.skip_codegen {
        return Ok(());
    }

    emit_serve_info(format!(
        "running one-shot codegen preflight for {}",
        app_dir.display()
    ));
    run_codegen_for_app_dir(app_dir).await?;
    emit_serve_info(format!("generated app artifacts for {}", app_dir.display()));
    Ok(())
}

pub(super) fn load_convex_registry(
    command: &ServeCommand,
    runtime_limits: &neovex::RuntimeLimits,
) -> Result<Option<ConvexRegistry>, Error> {
    command
        .app_dir
        .as_ref()
        .map(|path| {
            ensure_required_functions_manifest(path, command.skip_codegen)?;
            ConvexRegistry::from_app_dir(path)
                .map(|registry| registry.with_runtime_limits(runtime_limits.clone()))
        })
        .transpose()
}

fn load_sandbox_service_manager(
    command: &ServeCommand,
    compose_control_data_dir: &std::path::Path,
) -> Result<Option<Arc<neovex::SandboxServiceManager>>, Error> {
    command
        .compose_file
        .as_deref()
        .map(|path| load_host_backed_sandbox_service_manager(path, compose_control_data_dir))
        .transpose()
        .map(|manager| manager.map(Arc::new))
}

fn emit_serve_info(message: impl AsRef<str>) {
    if cli_ux::info_output_enabled() {
        let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
    }
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

fn manifest_recovery_hint(app_dir: &Path, skip_codegen: bool) -> String {
    if skip_codegen {
        format!(
            "Run \"neovex codegen --app {}\" to generate it, or remove --skip-codegen to generate manifests automatically on serve.",
            app_dir.display()
        )
    } else {
        format!(
            "Run \"neovex codegen --app {}\" to generate it before retrying.",
            app_dir.display()
        )
    }
}

async fn serve_with_optional_runtime_and_services(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: Option<ConvexRegistry>,
    license_state: LicenseState,
    sandbox_service_manager: Option<Arc<neovex::SandboxServiceManager>>,
) -> std::io::Result<()> {
    match (convex_registry, sandbox_service_manager) {
        (Some(registry), Some(manager)) => {
            serve_with_convex_and_license_and_sandbox_service_manager(
                listener,
                service,
                registry,
                license_state,
                manager,
            )
            .await
        }
        (Some(registry), None) => {
            serve_with_convex_and_license(listener, service, registry, license_state).await
        }
        (None, Some(manager)) => {
            let sandbox_catalog: Arc<dyn SandboxCatalog> = manager;
            serve_with_license_and_sandbox_catalog(
                listener,
                service,
                license_state,
                sandbox_catalog,
            )
            .await
        }
        (None, None) => serve_with_license(listener, service, license_state).await,
    }
}
