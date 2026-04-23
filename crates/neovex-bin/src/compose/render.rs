use neovex::{Error, PublishedEndpoint, SandboxStatus, TenantId};
use serde::Serialize;

use crate::cli_ux;

use super::{
    ComposeInspectOutputFormat, ComposePsOutputFormat, ComposeTopOutputFormat,
    ServiceLifecycleOutcome, ServiceProcessSnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ServiceSandboxSummaryView {
    pub(super) sandbox_id: neovex::SandboxId,
    pub(super) tenant_id: TenantId,
    pub(super) service_name: String,
    pub(super) status: SandboxStatus,
    pub(super) published_endpoints: Vec<PublishedEndpoint>,
    pub(super) restart_count: u32,
    pub(super) last_exit_code: Option<i32>,
    pub(super) shutdown_requested: bool,
}

pub(super) fn render_sandbox_status(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Starting => "starting",
        SandboxStatus::Ready => "ready",
        SandboxStatus::NotReady => "not_ready",
        SandboxStatus::Stopping => "stopping",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
    }
}

pub(super) fn render_service_lifecycle_action_summary(
    summary: &str,
    project_name: &str,
    tenant: &neovex::TenantId,
    outcomes: &[ServiceLifecycleOutcome],
) -> String {
    let header = format!("{summary} for project {project_name} (tenant {tenant})");
    let detail_lines = outcomes
        .iter()
        .map(|outcome| {
            format!(
                "{}: {} (sandbox {}, status {})",
                outcome.service_name,
                outcome.action.as_str(),
                outcome.sandbox_id,
                render_sandbox_status(outcome.status),
            )
        })
        .collect::<Vec<_>>();
    cli_ux::format_action_block(&header, &detail_lines)
}

pub(super) fn render_service_list_view(
    summaries: &[ServiceSandboxSummaryView],
    format: ComposePsOutputFormat,
    no_heading: bool,
) -> Result<String, Error> {
    match format {
        ComposePsOutputFormat::Json => serde_json::to_string_pretty(summaries).map_err(|error| {
            Error::Serialization(format!("failed to render compose ps output: {error}"))
        }),
        ComposePsOutputFormat::Yaml => serde_yaml::to_string(summaries).map_err(|error| {
            Error::Serialization(format!("failed to render compose ps output: {error}"))
        }),
        ComposePsOutputFormat::Table => Ok(render_service_list_table(summaries, no_heading)),
    }
}

fn render_service_list_table(summaries: &[ServiceSandboxSummaryView], no_heading: bool) -> String {
    let columns = [
        cli_ux::TableColumn::left("SERVICE", 12),
        cli_ux::TableColumn::left("TENANT", 16),
        cli_ux::TableColumn::left("STATUS", 12),
        cli_ux::TableColumn::left("SANDBOX", 14),
        cli_ux::TableColumn::right("RESTARTS", 8),
        cli_ux::TableColumn::right("EXIT", 4),
        cli_ux::TableColumn::left("ENDPOINTS", 12),
    ];
    let rows = summaries
        .iter()
        .map(|summary| {
            vec![
                summary.service_name.clone(),
                summary.tenant_id.to_string(),
                render_sandbox_status(summary.status).to_owned(),
                summary.sandbox_id.to_string(),
                summary.restart_count.to_string(),
                summary
                    .last_exit_code
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_owned()),
                render_published_endpoints(&summary.published_endpoints),
            ]
        })
        .collect::<Vec<_>>();
    cli_ux::render_table_with_options(
        &columns,
        &rows,
        cli_ux::TableRenderOptions {
            omit_header: no_heading,
        },
    )
}

pub(super) fn render_service_inspect_view<T: Serialize>(
    details: &T,
    format: ComposeInspectOutputFormat,
    service_name: &str,
) -> Result<String, Error> {
    match format {
        ComposeInspectOutputFormat::Json => {
            serde_json::to_string_pretty(details).map_err(|error| {
                Error::Serialization(format!(
                    "failed to render sandbox details for service {}: {error}",
                    service_name
                ))
            })
        }
        ComposeInspectOutputFormat::Yaml => serde_yaml::to_string(details).map_err(|error| {
            Error::Serialization(format!(
                "failed to render sandbox details for service {}: {error}",
                service_name
            ))
        }),
    }
}

pub(super) fn render_service_process_snapshot_view(
    snapshot: &ServiceProcessSnapshot,
    format: ComposeTopOutputFormat,
    no_heading: bool,
) -> Result<String, Error> {
    match format {
        ComposeTopOutputFormat::Json => serde_json::to_string_pretty(snapshot).map_err(|error| {
            Error::Serialization(format!("failed to render compose top output: {error}"))
        }),
        ComposeTopOutputFormat::Yaml => serde_yaml::to_string(snapshot).map_err(|error| {
            Error::Serialization(format!("failed to render compose top output: {error}"))
        }),
        ComposeTopOutputFormat::Table => {
            Ok(render_service_process_snapshot_table(snapshot, no_heading))
        }
    }
}

fn render_service_process_snapshot_table(
    snapshot: &ServiceProcessSnapshot,
    no_heading: bool,
) -> String {
    let mut detail_lines = vec![
        format!("sandbox: {}", snapshot.sandbox_id),
        format!("status: {}", render_sandbox_status(snapshot.status)),
        format!(
            "runtime pid: {}",
            snapshot
                .runtime_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_owned())
        ),
        format!(
            "conmon pid: {}",
            snapshot
                .conmon_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_owned())
        ),
    ];
    if snapshot.process_rows.is_empty() {
        detail_lines.push("tracked processes: none".to_owned());
        return cli_ux::format_action_block(
            &format!(
                "Compose top snapshot for {} (tenant {})",
                snapshot.service_name, snapshot.tenant_id
            ),
            &detail_lines,
        );
    }

    let mut rendered = cli_ux::format_action_block(
        &format!(
            "Compose top snapshot for {} (tenant {})",
            snapshot.service_name, snapshot.tenant_id
        ),
        &detail_lines,
    );
    let columns = [
        cli_ux::TableColumn::right("PID", 5),
        cli_ux::TableColumn::right("PPID", 5),
        cli_ux::TableColumn::left("COMMAND", 24),
    ];
    let rows = snapshot
        .process_rows
        .iter()
        .map(|row| {
            vec![
                row.pid.to_string(),
                row.ppid.to_string(),
                row.command.clone(),
            ]
        })
        .collect::<Vec<_>>();
    rendered.push_str(&cli_ux::render_table_with_options(
        &columns,
        &rows,
        cli_ux::TableRenderOptions {
            omit_header: no_heading,
        },
    ));
    rendered
}

fn render_published_endpoints(endpoints: &[PublishedEndpoint]) -> String {
    if endpoints.is_empty() {
        return "-".to_owned();
    }

    endpoints
        .iter()
        .map(|endpoint| {
            format!(
                "{}={}/{}",
                endpoint.name,
                endpoint.address,
                render_published_endpoint_protocol(endpoint.protocol)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_published_endpoint_protocol(protocol: neovex::PublishedEndpointProtocol) -> &'static str {
    match protocol {
        neovex::PublishedEndpointProtocol::Tcp => "tcp",
        neovex::PublishedEndpointProtocol::Http => "http",
        neovex::PublishedEndpointProtocol::Https => "https",
    }
}
