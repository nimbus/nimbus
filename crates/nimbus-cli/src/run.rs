use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use nimbus::Error;
use nimbus_operator::LocalServerPaths;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::function_scaling::{
    FunctionScalingAdmissionEnvelope, FunctionScalingContext, admit_function_scaling_intent,
    load_config, load_optional_policy, resolve_function_scaling_intent,
};
use crate::local_server_client::LocalServerHttpClient;
use crate::target_context::{TargetContext, TargetContextKind, TargetSelector};

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = "Examples:\n  nimbus run functions messages:send '{\"body\":\"hello\"}'\n  nimbus run exec -- npm test\n"
)]
pub(crate) struct RunCommand {
    #[command(flatten)]
    pub(crate) target: TargetSelector,

    #[command(subcommand)]
    pub(crate) resource: RunResource,
}

#[derive(Debug, Subcommand)]
pub(crate) enum RunResource {
    /// Run an admitted function selector with optional JSON args.
    Functions(RunFunctionsCommand),
    /// Run a future explicit command workload against a target.
    Exec(RunExecCommand),
}

#[derive(Debug, Args)]
pub(crate) struct RunFunctionsCommand {
    /// Function selector such as messages:send.
    pub(crate) selector: String,
    /// JSON argument payload.
    pub(crate) json_args: Option<String>,
    /// Function kind. Inferred from .nimbus/convex/functions.json when omitted.
    #[arg(long, value_enum)]
    pub(crate) kind: Option<RunFunctionKind>,
    /// Tenant to invoke.
    #[arg(long, default_value = "demo")]
    pub(crate) tenant: String,
    /// App directory used for generated function-kind inference.
    #[arg(long, value_name = "DIR")]
    pub(crate) app: Option<PathBuf>,
    /// Page size for paginated query functions.
    #[arg(long, default_value_t = 100)]
    pub(crate) page_size: usize,
    /// Cursor for paginated query functions.
    #[arg(long)]
    pub(crate) cursor: Option<String>,
    /// Path to nimbus.yaml / nimbus.json.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    /// Path to nimbus.policy.yaml for operator quota admission.
    #[arg(long, value_name = "FILE")]
    pub(crate) policy: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct RunExecCommand {
    /// Command and arguments to run against the selected Nimbus target.
    #[arg(value_name = "COMMAND", trailing_var_arg = true, allow_hyphen_values = true, num_args = 1..)]
    pub(crate) argv: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunFunctionKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
}

impl RunFunctionKind {
    fn endpoint_suffix(self) -> &'static str {
        match self {
            Self::Query => "/query",
            Self::PaginatedQuery => "/query/paginated",
            Self::Mutation => "/mutation",
            Self::Action => "/action",
        }
    }

    fn payload(self, selector: &str, args: Value, page_size: usize, cursor: Option<&str>) -> Value {
        match self {
            Self::PaginatedQuery => json!({
                "name": selector,
                "args": args,
                "page_size": page_size,
                "cursor": cursor,
            }),
            Self::Query | Self::Mutation | Self::Action => {
                json!({ "name": selector, "args": args })
            }
        }
    }
}

pub(crate) async fn run_run_command(command: RunCommand) -> Result<(), Error> {
    let target = resolve_run_target(&command)?;
    match command.resource {
        RunResource::Functions(function) => run_function_command(target, function).await,
        RunResource::Exec(exec) => Err(Error::InvalidInput(format!(
            "nimbus run exec resolved {target:?} with argv {:?}, but generic command workload execution is reserved for the service-sandbox-node workload-control path",
            exec.argv
        ))),
    }
}

pub(crate) fn resolve_run_target(command: &RunCommand) -> Result<TargetContext, Error> {
    resolve_run_target_with_env(command, |name| env::var(name).ok())
}

fn resolve_run_target_with_env(
    command: &RunCommand,
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Result<TargetContext, Error> {
    command.target.resolve("run", |name| env_lookup(name))
}

async fn run_function_command(
    target: TargetContext,
    command: RunFunctionsCommand,
) -> Result<(), Error> {
    let args = parse_json_args(&command)?;
    let config = load_config(command.config.clone())?;
    let policy = load_optional_policy(command.policy.clone())?;
    let intent = resolve_function_scaling_intent(
        &config.functions.scaling,
        FunctionScalingContext::Start,
        &command.selector,
    )?;
    admit_function_scaling_intent(
        &intent,
        policy.as_ref(),
        Some(&command.tenant),
        FunctionScalingAdmissionEnvelope::default(),
    )?;
    let kind = resolve_run_function_kind(&command)?;
    let payload = kind.payload(
        &command.selector,
        args,
        command.page_size,
        command.cursor.as_deref(),
    );
    let response = invoke_run_function(&target, &command.tenant, kind, &payload).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).map_err(|error| Error::Internal(format!(
            "failed to render function result: {error}"
        )))?
    );
    Ok(())
}

fn parse_json_args(command: &RunFunctionsCommand) -> Result<Value, Error> {
    match &command.json_args {
        Some(args) => serde_json::from_str(args).map_err(|error| {
            Error::InvalidInput(format!(
                "nimbus run functions {} argument payload must be valid JSON: {error}",
                command.selector
            ))
        }),
        None => Ok(json!({})),
    }
}

fn resolve_run_function_kind(command: &RunFunctionsCommand) -> Result<RunFunctionKind, Error> {
    if let Some(kind) = command.kind {
        return Ok(kind);
    }
    if let Some(kind) =
        infer_function_kind_from_generated_manifest(command.app.as_deref(), &command.selector)?
    {
        return Ok(kind);
    }
    Err(Error::InvalidInput(format!(
        "could not infer kind for `{}`; pass --kind query, --kind paginated-query, --kind mutation, or --kind action, or run from an app directory with .nimbus/convex/functions.json",
        command.selector
    )))
}

fn infer_function_kind_from_generated_manifest(
    app_dir: Option<&Path>,
    selector: &str,
) -> Result<Option<RunFunctionKind>, Error> {
    let base = match app_dir {
        Some(path) => path.to_path_buf(),
        None => env::current_dir()
            .map_err(|error| Error::Internal(format!("failed to inspect current dir: {error}")))?,
    };
    let path = base.join(".nimbus").join("convex").join("functions.json");
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| {
        Error::Internal(format!(
            "failed to read generated function manifest {}: {error}",
            path.display()
        ))
    })?;
    let manifest: GeneratedFunctionManifest = serde_json::from_slice(&bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse generated function manifest {}: {error}",
            path.display()
        ))
    })?;
    let matches: Vec<_> = manifest
        .functions
        .into_iter()
        .filter(|function| function.name == selector)
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [function] => Ok(Some(function.kind)),
        _ => Err(Error::InvalidInput(format!(
            "generated function manifest {} contains duplicate selector `{selector}`",
            path.display()
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct GeneratedFunctionManifest {
    #[serde(default)]
    functions: Vec<GeneratedFunctionDefinition>,
}

#[derive(Debug, Deserialize)]
struct GeneratedFunctionDefinition {
    name: String,
    kind: RunFunctionKind,
}

async fn invoke_run_function(
    target: &TargetContext,
    tenant: &str,
    kind: RunFunctionKind,
    payload: &Value,
) -> Result<Value, Error> {
    let path = format!("/convex/{tenant}{}", kind.endpoint_suffix());
    match &target.kind {
        TargetContextKind::LocalDiscovery => {
            let paths = LocalServerPaths::resolve_for_current_platform().map_err(|error| {
                Error::Internal(format!("failed to resolve local server paths: {error}"))
            })?;
            let client = LocalServerHttpClient::discover(&paths, reqwest::Client::new())?
                .ok_or_else(|| {
                    Error::InvalidInput(
                        "no running local Nimbus server was found; start one with `nimbus dev` or `nimbus start`, or pass a TARGET URL".to_string(),
                    )
                })?;
            client.post_json(&path, payload).await
        }
        TargetContextKind::RemoteUrl(base_url) => {
            invoke_remote_run_function(base_url, &path, payload).await
        }
        TargetContextKind::NamedTarget(target) => Err(Error::InvalidInput(format!(
            "named target `{target}` is not yet backed by a target registry; pass a TARGET URL or omit TARGET for local"
        ))),
    }
}

async fn invoke_remote_run_function(
    base_url: &str,
    path: &str,
    payload: &Value,
) -> Result<Value, Error> {
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let response = reqwest::Client::new()
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|error| Error::Internal(format!("failed to invoke Nimbus function: {error}")))?;
    decode_run_function_response(response).await
}

async fn decode_run_function_response(response: reqwest::Response) -> Result<Value, Error> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(|error| {
        Error::Internal(format!("failed to read function response body: {error}"))
    })?;
    if status.is_success() {
        return serde_json::from_slice(&bytes)
            .map_err(|error| Error::Internal(format!("failed to parse function JSON: {error}")));
    }
    let message = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| String::from_utf8_lossy(&bytes).trim().to_owned());
    let message = if message.is_empty() {
        format!("Nimbus function route returned HTTP {status}")
    } else {
        format!("Nimbus function route returned HTTP {status}: {message}")
    };
    match status {
        reqwest::StatusCode::BAD_REQUEST
        | reqwest::StatusCode::NOT_FOUND
        | reqwest::StatusCode::UNPROCESSABLE_ENTITY => Err(Error::InvalidInput(message)),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            Err(Error::PermissionDenied(message))
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err(Error::ResourceExhausted(message)),
        _ => Err(Error::Internal(message)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser;

    use super::RunResource;
    use crate::{Cli, Command};

    #[test]
    fn run_command_resolves_target() {
        let cli = Cli::parse_from(["nimbus", "run", "dev", "exec", "--", "npm", "test"]);
        let Command::Run(command) = cli.command else {
            panic!("run command should parse");
        };

        let context =
            super::resolve_run_target_with_env(&command, |_| None).expect("target should resolve");

        assert_eq!(
            context.kind,
            crate::target_context::TargetContextKind::NamedTarget("dev".to_owned())
        );
        let RunResource::Exec(exec) = command.resource else {
            panic!("run exec should parse");
        };
        assert_eq!(exec.argv, vec!["npm".to_owned(), "test".to_owned()]);
    }

    #[test]
    fn run_functions_command_parses_selector_and_json_args() {
        let cli = Cli::parse_from([
            "nimbus",
            "run",
            "functions",
            "messages:send",
            "{\"body\":\"hello\"}",
        ]);
        let Command::Run(command) = cli.command else {
            panic!("run command should parse");
        };

        let RunResource::Functions(function) = command.resource else {
            panic!("run functions should parse");
        };
        assert_eq!(function.selector, "messages:send");
        assert_eq!(function.json_args.as_deref(), Some("{\"body\":\"hello\"}"));
        assert_eq!(function.kind, None);
        assert_eq!(function.tenant, "demo");
        assert_eq!(function.page_size, 100);
    }

    #[test]
    fn run_functions_defaults_to_local_discovery_without_target() {
        let cli = Cli::parse_from([
            "nimbus",
            "run",
            "functions",
            "messages:send",
            "{\"body\":\"hello\"}",
            "--kind",
            "mutation",
        ]);
        let Command::Run(command) = cli.command else {
            panic!("run command should parse");
        };

        let context = super::resolve_run_target_with_env(&command, |_| None)
            .expect("run functions should default to local discovery");

        assert_eq!(
            context.kind,
            crate::target_context::TargetContextKind::LocalDiscovery
        );
        assert_eq!(
            context.source,
            crate::target_context::TargetContextSource::ImplicitLocalDefault
        );
    }

    #[test]
    fn generated_manifest_infers_function_kind() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let manifest_dir = temp.path().join(".nimbus").join("convex");
        fs::create_dir_all(&manifest_dir).expect("manifest dir should build");
        fs::write(
            manifest_dir.join("functions.json"),
            r#"
{
  "functions": [
    { "name": "messages:list", "kind": "query" },
    { "name": "messages:send", "kind": "mutation" }
  ]
}
"#,
        )
        .expect("manifest fixture should write");

        let kind =
            super::infer_function_kind_from_generated_manifest(Some(temp.path()), "messages:send")
                .expect("manifest should parse");

        assert_eq!(kind, Some(super::RunFunctionKind::Mutation));
    }

    #[test]
    fn run_functions_requires_kind_when_manifest_is_absent() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let error = super::resolve_run_function_kind(&super::RunFunctionsCommand {
            selector: "messages:send".to_string(),
            json_args: Some("{\"body\":\"hello\"}".to_string()),
            kind: None,
            tenant: "demo".to_string(),
            app: Some(temp.path().to_path_buf()),
            page_size: 100,
            cursor: None,
            config: None,
            policy: None,
        })
        .expect_err("missing kind should reject without a generated manifest");

        assert!(error.to_string().contains("could not infer kind"));
        assert!(error.to_string().contains("--kind mutation"));
    }

    #[test]
    fn paginated_query_payload_uses_convex_named_paginated_shape() {
        let payload = super::RunFunctionKind::PaginatedQuery.payload(
            "messages:listPage",
            serde_json::json!({ "author": "Ada" }),
            25,
            Some("cursor-1"),
        );

        assert_eq!(
            payload,
            serde_json::json!({
                "name": "messages:listPage",
                "args": { "author": "Ada" },
                "page_size": 25,
                "cursor": "cursor-1"
            })
        );
    }

    #[tokio::test]
    async fn run_functions_uses_explicit_operator_policy_for_quota_admission() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let config_path = temp.path().join("nimbus.yaml");
        let policy_path = temp.path().join("nimbus.policy.yaml");
        fs::write(
            &config_path,
            r#"
functions:
  scaling:
    overrides:
      "messages:send":
        preset: latency
        min_warm: 2
        reason: "hot write path"
"#,
        )
        .expect("config fixture should write");
        fs::write(
            &policy_path,
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_resources:
    cpu_millicpus: 1000
    memory_bytes: 536870912
    storage_bytes: 10737418240
    host_cpu_reserve_millicpus: 250
    host_memory_reserve_bytes: 134217728
  runtime_safety:
    max_min_warm_total: 1
workloads:
  - kind: runtime_function
    name: messages:send
"#,
        )
        .expect("policy fixture should write");
        let target = crate::target_context::TargetContext {
            kind: crate::target_context::TargetContextKind::LocalDiscovery,
            source: crate::target_context::TargetContextSource::ImplicitLocalDefault,
        };

        let error = super::run_function_command(
            target,
            super::RunFunctionsCommand {
                selector: "messages:send".to_string(),
                json_args: Some("{\"body\":\"hello\"}".to_string()),
                kind: Some(super::RunFunctionKind::Mutation),
                tenant: "tenant-a".to_string(),
                app: None,
                page_size: 100,
                cursor: None,
                config: Some(config_path),
                policy: Some(policy_path),
            },
        )
        .await
        .expect_err("operator policy should reject over-limit min_warm");

        assert!(error.to_string().contains("requested min_warm=2"));
        assert!(
            error
                .to_string()
                .contains("runtime_safety.max_min_warm_total")
        );
    }
}
