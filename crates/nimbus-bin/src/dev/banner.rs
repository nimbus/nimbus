use std::io;
use std::path::PathBuf;

use crate::cli_ux;
use crate::compose::discovery::compose_selection_summary;

use super::launch::operator_console_url;
use super::plan::DevPlan;

pub(super) fn emit_dev_banner(plan: &DevPlan) -> io::Result<()> {
    for line in dev_banner_lines(plan) {
        cli_ux::write_stderr_line(&line)?;
    }
    Ok(())
}

pub(super) fn dev_banner_lines(plan: &DevPlan) -> Vec<String> {
    let mut lines = vec![
        "Nimbus dev ready to start".to_string(),
        format!("Local:      {}", plan.local_url),
        format!(
            "operator console:\t{}",
            operator_console_url(&plan.local_url)
        ),
        format!("Deployment: local:{}", plan.deployment_slug),
        format!("App dir:    {}", plan.app_dir.display()),
        format!("Data:       {}", plan.data_dir.display()),
    ];
    if let Some(adapter) = &plan.adapter {
        lines.push(format!("Adapter:    {}", adapter.name()));
    }
    if let Some(mapping) = &plan.firestore_tenant {
        lines.push(format!(
            "Tenant:     {} ({})",
            mapping.tenant,
            mapping.describe_source()
        ));
    }
    if let Some(selection) = plan.compose_selection.as_ref() {
        lines.push(format!(
            "Compose:    {}",
            compose_selection_summary(selection)
        ));
    }
    match plan.adapter.as_ref() {
        Some(adapter) if plan.once => lines.push(format!(
            "Watch:      disabled by --once; detected {}",
            format_watch_roots(adapter.source_roots())
        )),
        Some(adapter) => {
            lines.push(format!(
                "Watch:      {}",
                format_watch_roots(adapter.source_roots())
            ));
        }
        None if plan.once => {
            lines.push("Watch:      disabled by --once; no adapter detected".to_string());
        }
        None => {
            lines.push("Watch:      disabled; no adapter detected".to_string());
        }
    }
    lines.push(format!("Logs:       {}", plan.tail_logs.as_str()));
    lines.push(
        "Note: watched codegen activates regenerated artifacts locally after validation; runtime log multiplexing is still pending.".to_string(),
    );
    lines
}

pub(super) fn format_watch_roots(source_roots: &[PathBuf]) -> String {
    source_roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
