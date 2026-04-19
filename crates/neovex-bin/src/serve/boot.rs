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
use crate::service::load_host_backed_sandbox_service_manager;

pub(crate) async fn run_serve_command(
    command: ServeCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let service_config = service_persistence_config_from_serve_command(&command)?;
    let compose_control_data_dir =
        control_data_dir_from_service_config(&service_config).to_path_buf();
    let service = Arc::new(Service::new_with_persistence_config(service_config).await?);
    let shutdown_service = service.clone();
    service.recover_scheduled_work_on_startup_async().await?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let scheduler_service = service.clone();
    let scheduler_handle = tokio::spawn(async move {
        run_scheduler(scheduler_service, shutdown_rx).await;
    });
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", command.port)).await?;
    let runtime_limits = runtime_limits_from_command(&command);
    let license_state = LicenseState::load(command.license_file.as_deref())?;
    let license_snapshot = license_state.snapshot();
    let convex_registry = load_convex_registry(&command, &runtime_limits)?;
    let sandbox_service_manager =
        load_sandbox_service_manager(&command, &compose_control_data_dir)?;

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

fn load_convex_registry(
    command: &ServeCommand,
    runtime_limits: &neovex::RuntimeLimits,
) -> Result<Option<ConvexRegistry>, Error> {
    command
        .convex_app_dir
        .as_ref()
        .map(|path| {
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
