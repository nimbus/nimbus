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
        lines.push(format!(
            "Firestore:  {}v1/projects/{}/databases/(default)/documents",
            plan.local_url, mapping.tenant
        ));
    }
    // Detected wire surfaces get an endpoint line plus a copy-paste client
    // snippet. The snippets reference the Nimbus-owned `.env.local` keys —
    // the banner never prints credential values.
    if plan.wire_surfaces.mongodb {
        lines.push(format!(
            "MongoDB:    mongodb://127.0.0.1:{}/ (NIMBUS_MONGODB_URL in .env.local)",
            plan.wire.mongodb_port.port
        ));
        lines.push("            new MongoClient(process.env.NIMBUS_MONGODB_URL)".to_string());
    }
    if plan.wire_surfaces.dynamodb {
        lines.push(format!(
            "DynamoDB:   http://127.0.0.1:{} (NIMBUS_DYNAMODB_ENDPOINT in .env.local)",
            plan.wire.dynamodb_port.port
        ));
        lines.push(
            "            new DynamoDBClient({ endpoint: process.env.NIMBUS_DYNAMODB_ENDPOINT, \
             credentials: { accessKeyId: process.env.NIMBUS_DYNAMODB_ACCESS_KEY_ID, \
             secretAccessKey: process.env.NIMBUS_DYNAMODB_SECRET_ACCESS_KEY } })"
                .to_string(),
        );
    } else if plan.wire_surfaces.aws_sdk_v2_hint {
        // D3: aws-sdk v2 alone never promotes the endpoint — the v2 import
        // shape is too ambiguous (S3, SQS, …) — but it earns a hint.
        lines.push(
            "Hint:       aws-sdk v2 detected; @aws-sdk/client-dynamodb (v3) enables \
             automatic DynamoDB endpoint + credentials in .env.local"
                .to_string(),
        );
    }
    if let Some(selection) = plan.compose_selection.as_ref() {
        lines.push(format!(
            "Compose:    {}",
            compose_selection_summary(selection)
        ));
    }
    match plan.adapter.as_ref() {
        // A client app has no server-side sources: the watch loop never
        // runs, so a Watch line would describe a loop that does not exist.
        Some(adapter) if adapter.source_roots().is_empty() => {}
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
