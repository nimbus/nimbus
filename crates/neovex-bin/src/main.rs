use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use neovex::{
    ConvexRegistry, LicenseState, RuntimeLimits, Service, run_scheduler,
    serve_with_convex_and_license, serve_with_license,
};

#[derive(Debug, Parser)]
#[command(name = "neovex", about = "Reactive document database")]
struct Cli {
    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Data directory used for tenant databases.
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,

    /// Optional app directory with a generated .neovex/convex/functions.json manifest.
    #[arg(long)]
    convex_app_dir: Option<PathBuf>,

    /// Optional path to a Neovex license file. Defaults to ./.neovex/license.json when present.
    #[arg(long)]
    license_file: Option<PathBuf>,

    /// V8 heap limit per runtime isolate in megabytes.
    #[arg(long, default_value_t = default_runtime_heap_mb())]
    runtime_heap_mb: usize,

    /// Initial V8 heap size per runtime isolate in megabytes.
    #[arg(long, default_value_t = default_runtime_initial_heap_mb())]
    runtime_initial_heap_mb: usize,

    /// Maximum wall-clock execution time for a runtime invocation in seconds.
    #[arg(long, default_value_t = default_runtime_timeout_secs())]
    runtime_timeout_secs: u64,

    /// Maximum number of concurrent top-level runtime isolates.
    #[arg(long, default_value_t = default_runtime_max_isolates())]
    runtime_max_isolates: usize,

    /// Maximum number of nested runtime ctx.run* invocations allowed per request tree.
    #[arg(long, default_value_t = default_runtime_max_nested_calls())]
    runtime_max_nested_calls: usize,
}

fn default_runtime_heap_mb() -> usize {
    RuntimeLimits::default().max_heap_mb
}

fn default_runtime_initial_heap_mb() -> usize {
    RuntimeLimits::default().initial_heap_mb
}

fn default_runtime_timeout_secs() -> u64 {
    RuntimeLimits::default().execution_timeout.as_secs()
}

fn default_runtime_max_isolates() -> usize {
    RuntimeLimits::default().max_concurrent_isolates
}

fn default_runtime_max_nested_calls() -> usize {
    RuntimeLimits::default().max_nested_runtime_invocations
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let service = Arc::new(Service::new(cli.data_dir)?);
    service.load_tenants_with_scheduled_work()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let scheduler_service = service.clone();
    let scheduler_handle = tokio::spawn(async move {
        run_scheduler(scheduler_service, shutdown_rx).await;
    });
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", cli.port)).await?;
    let runtime_limits = RuntimeLimits {
        max_heap_mb: cli.runtime_heap_mb,
        initial_heap_mb: cli.runtime_initial_heap_mb,
        execution_timeout: Duration::from_secs(cli.runtime_timeout_secs),
        max_concurrent_isolates: cli.runtime_max_isolates,
        max_nested_runtime_invocations: cli.runtime_max_nested_calls,
    };
    let license_state = LicenseState::load(cli.license_file.as_deref())?;
    let license_snapshot = license_state.snapshot();
    let convex_registry = cli
        .convex_app_dir
        .as_ref()
        .map(|path| {
            ConvexRegistry::from_app_dir(path)
                .map(|registry| registry.with_runtime_limits(runtime_limits.clone()))
        })
        .transpose()?;

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
    let server_result = match convex_registry {
        Some(registry) => {
            serve_with_convex_and_license(listener, service, registry, license_state).await
        }
        None => serve_with_license(listener, service, license_state).await,
    };
    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
    server_result?;
    Ok(())
}
