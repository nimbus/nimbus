use std::env;
use std::fs;
use std::io::{self, Write};
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
use crate::local_server_client::discover_local_server_base_url;
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
    write_run_result(&response, &mut io::stdout().lock())
}

/// Render the function result to `out` as pretty JSON and nothing else. This is
/// the stdout payload: the banner never travels this path, so a consumer that
/// pipes stdout gets clean JSON.
fn write_run_result(response: &Value, out: &mut impl Write) -> Result<(), Error> {
    let rendered = serde_json::to_string_pretty(response)
        .map_err(|error| Error::Internal(format!("failed to render function result: {error}")))?;
    writeln!(out, "{rendered}")
        .map_err(|error| Error::Internal(format!("failed to write function result: {error}")))
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
            invoke_local_run_function(target, &paths, &path, payload).await
        }
        TargetContextKind::RemoteUrl(base_url) => {
            emit_run_target_banner(target, base_url);
            invoke_remote_run_function(base_url, &path, payload).await
        }
        TargetContextKind::NamedTarget(name) => {
            let base_url = crate::targets::resolve_named_target_url(name)?;
            emit_run_target_banner(target, &base_url);
            invoke_remote_run_function(&base_url, &path, payload).await
        }
    }
}

async fn invoke_local_run_function(
    target: &TargetContext,
    paths: &LocalServerPaths,
    path: &str,
    payload: &Value,
) -> Result<Value, Error> {
    let base_url = discover_local_server_base_url(paths)?.ok_or_else(|| {
        Error::InvalidInput(
            "no running local Nimbus server was found; start one with `nimbus dev` or `nimbus start`, or pass a TARGET URL".to_string(),
        )
    })?;
    emit_run_target_banner(target, &base_url);
    invoke_remote_run_function(&base_url, path, payload).await
}

/// Print the resolved-target banner to stderr (never stdout, which carries the
/// function result JSON) so the destination is explicit even when implicit.
fn emit_run_target_banner(target: &TargetContext, resolved_url: &str) {
    let _ = write_run_banner(target, resolved_url, &mut io::stderr().lock());
}

/// Render the resolved-target banner to `out` (the stderr sink in production).
/// Kept separate from [`write_run_result`] so a test can prove the banner and
/// the result JSON go to different sinks and never contaminate each other.
fn write_run_banner(
    target: &TargetContext,
    resolved_url: &str,
    out: &mut impl Write,
) -> io::Result<()> {
    let banner = crate::targets::resolved_target_banner("Running against", target, resolved_url);
    writeln!(out, "{banner}")
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
    use std::sync::{Arc, Mutex};

    use axum::extract::{Request, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use axum::{Json, Router};
    use clap::Parser;
    use nimbus_operator::{
        LOCAL_ADMIN_HEADER_NAME, LocalServerPaths, load_or_create_local_admin_token,
    };
    use nimbus_server::ServerDiscoveryLease;

    use super::RunResource;
    use crate::{Cli, Command};

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct CapturedRunRequest {
        path: String,
        local_admin: Option<String>,
        authorization: Option<String>,
    }

    #[derive(Clone)]
    struct RunContractState {
        requests: Arc<Mutex<Vec<CapturedRunRequest>>>,
    }

    fn sample_local_server_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    async fn run_contract_handler(
        State(state): State<RunContractState>,
        request: Request,
    ) -> Response {
        let local_admin = request
            .headers()
            .get(LOCAL_ADMIN_HEADER_NAME)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let authorization = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let path = request.uri().path().to_owned();
        state
            .requests
            .lock()
            .expect("request capture should lock")
            .push(CapturedRunRequest {
                path: path.clone(),
                local_admin: local_admin.clone(),
                authorization: authorization.clone(),
            });

        if authorization.is_some() {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": { "message": "invalid application bearer" }
                })),
            )
                .into_response();
        }
        if path != "/convex/demo/query" {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": { "message": "caller cannot select this silo" }
                })),
            )
                .into_response();
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({ "value": [{ "title": "same result" }] })),
        )
            .into_response()
    }

    async fn start_run_contract_server() -> (
        String,
        Arc<Mutex<Vec<CapturedRunRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = RunContractState {
            requests: Arc::clone(&requests),
        };
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("run contract listener should bind");
        let address = listener
            .local_addr()
            .expect("run contract listener address should resolve");
        let router = Router::new()
            .route("/convex/{silo}/query", post(run_contract_handler))
            .with_state(state);
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("run contract server should keep serving");
        });
        (format!("http://{address}"), requests, task)
    }

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

    #[tokio::test]
    async fn isolated_legacy_admin_bearer_reproduces_fail_before_unauthorized() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_local_server_paths(temp.path());
        let admin = load_or_create_local_admin_token(&paths)
            .expect("isolated local admin token should initialize");
        let (base_url, requests, server_task) = start_run_contract_server().await;
        let address = base_url
            .strip_prefix("http://")
            .expect("fixture URL should have the HTTP scheme")
            .parse()
            .expect("fixture address should parse");
        let discovery = ServerDiscoveryLease::acquire(&paths, address)
            .expect("isolated discovery should initialize");
        let payload = serde_json::json!({ "name": "tasks:list", "args": {} });

        let response = reqwest::Client::new()
            .post(format!("{base_url}/convex/demo/query"))
            .bearer_auth(&admin.token)
            .json(&payload)
            .send()
            .await
            .expect("legacy local bearer request should send");
        let error = super::decode_run_function_response(response)
            .await
            .expect_err("the old host-admin-as-application-bearer path must reproduce 401");

        assert!(
            matches!(error, nimbus::Error::PermissionDenied(_)),
            "legacy credential collision should be unauthorized: {error}"
        );
        assert_eq!(
            *requests.lock().expect("request capture should lock"),
            vec![CapturedRunRequest {
                path: "/convex/demo/query".to_owned(),
                local_admin: None,
                authorization: Some(format!("Bearer {}", admin.token)),
            }]
        );

        drop(discovery);
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn bare_local_target_matches_explicit_target() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_local_server_paths(temp.path());
        let (base_url, requests, server_task) = start_run_contract_server().await;
        let address = base_url
            .strip_prefix("http://")
            .expect("fixture URL should have the HTTP scheme")
            .parse()
            .expect("fixture address should parse");
        let discovery = ServerDiscoveryLease::acquire(&paths, address)
            .expect("local server discovery should initialize");
        let payload = serde_json::json!({ "name": "tasks:list", "args": {} });
        let path = "/convex/demo/query";

        let explicit = super::invoke_remote_run_function(&base_url, path, &payload)
            .await
            .expect("explicit target should return a result");
        let local_target = crate::target_context::TargetContext {
            kind: crate::target_context::TargetContextKind::LocalDiscovery,
            source: crate::target_context::TargetContextSource::ImplicitLocalDefault,
        };
        let local = super::invoke_local_run_function(&local_target, &paths, path, &payload)
            .await
            .expect("bare-local target should return a result");

        assert_eq!(local, explicit);
        assert_eq!(
            *requests.lock().expect("request capture should lock"),
            vec![
                CapturedRunRequest {
                    path: path.to_owned(),
                    local_admin: None,
                    authorization: None,
                },
                CapturedRunRequest {
                    path: path.to_owned(),
                    local_admin: None,
                    authorization: None,
                },
            ]
        );

        drop(discovery);
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn bare_local_target_rejects_wrong_silo_and_invalid_credentials() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_local_server_paths(temp.path());
        let (base_url, requests, server_task) = start_run_contract_server().await;
        let address = base_url
            .strip_prefix("http://")
            .expect("fixture URL should have the HTTP scheme")
            .parse()
            .expect("fixture address should parse");
        let discovery = ServerDiscoveryLease::acquire(&paths, address)
            .expect("local server discovery should initialize");
        let target = crate::target_context::TargetContext {
            kind: crate::target_context::TargetContextKind::LocalDiscovery,
            source: crate::target_context::TargetContextSource::ImplicitLocalDefault,
        };
        let payload = serde_json::json!({ "name": "tasks:list", "args": {} });

        let wrong_silo =
            super::invoke_local_run_function(&target, &paths, "/convex/not-demo/query", &payload)
                .await
                .expect_err("local discovery must not authorize another silo");
        assert!(
            matches!(wrong_silo, nimbus::Error::PermissionDenied(_)),
            "wrong-silo selection should fail closed: {wrong_silo}"
        );

        let response = reqwest::Client::new()
            .post(format!("{base_url}/convex/demo/query"))
            .bearer_auth("invalid.application.credential")
            .json(&payload)
            .send()
            .await
            .expect("invalid application credential request should send");
        let invalid_credential = super::decode_run_function_response(response)
            .await
            .expect_err("an invalid application credential must fail closed");
        assert!(
            matches!(invalid_credential, nimbus::Error::PermissionDenied(_)),
            "invalid application credential should fail closed: {invalid_credential}"
        );

        {
            let captured = requests.lock().expect("request capture should lock");
            assert_eq!(captured.len(), 2);
            assert_eq!(captured[0].path, "/convex/not-demo/query");
            assert_eq!(captured[0].local_admin, None);
            assert_eq!(captured[0].authorization, None);
            assert_eq!(captured[1].path, "/convex/demo/query");
            assert_eq!(captured[1].local_admin, None);
            assert_eq!(
                captured[1].authorization.as_deref(),
                Some("Bearer invalid.application.credential")
            );
        }

        drop(discovery);
        server_task.abort();
        let _ = server_task.await;
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
    fn run_banner_goes_to_its_own_sink_and_result_stdout_stays_clean_json() {
        use crate::target_context::{TargetContext, TargetContextKind, TargetContextSource};

        let target = TargetContext {
            kind: TargetContextKind::RemoteUrl("https://nimbus.example.test".to_owned()),
            source: TargetContextSource::PositionalUrl,
        };

        // The banner sink carries only the one-line banner.
        let mut banner_sink = Vec::new();
        super::write_run_banner(&target, "https://nimbus.example.test", &mut banner_sink)
            .expect("banner should write");
        let banner = String::from_utf8(banner_sink).unwrap();
        assert_eq!(
            banner.trim_end(),
            "Running against https://nimbus.example.test (from TARGET)"
        );

        // The result sink carries only the JSON — it parses, and never carries
        // the banner text, so piping stdout yields clean JSON.
        let mut result_sink = Vec::new();
        super::write_run_result(
            &serde_json::json!({ "ok": true, "value": 7 }),
            &mut result_sink,
        )
        .expect("result should write");
        let stdout = String::from_utf8(result_sink).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&stdout).expect("stdout must be valid JSON only");
        assert_eq!(parsed, serde_json::json!({ "ok": true, "value": 7 }));
        assert!(
            !stdout.contains("Running against"),
            "result stdout must never carry the banner: {stdout}"
        );
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
