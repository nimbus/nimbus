use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use rand::RngCore;

use crate::compose::discovery::{ResolvedComposeSelection, resolve_compose_selection};
use crate::dirs;
use crate::start::{CliTenantProvider, StartCommand};

use super::adapter::{DevAdapter, detect_dev_adapter};
use super::firebase_project::{DEMO_TENANT, ProjectTenantMapping, discover_project_tenant};
use super::launch::{AutoOpenDecision, ProcessEnv, resolve_auto_open};
use super::surfaces::{WireSurfaces, detect_wire_surfaces};
use super::{DevCommand, DevTailLogsMode};

#[derive(Debug)]
pub(super) struct DevPlan {
    pub(super) app_dir: PathBuf,
    pub(super) data_dir: PathBuf,
    pub(super) deployment_slug: String,
    pub(super) compose_selection: Option<ResolvedComposeSelection>,
    pub(super) local_url: String,
    pub(super) adapter: Option<DevAdapter>,
    /// The projectId→tenant mapping a Firestore client app's requests will
    /// resolve to; `None` for every other adapter shape.
    pub(super) firestore_tenant: Option<ProjectTenantMapping>,
    pub(super) wire_surfaces: WireSurfaces,
    pub(super) once: bool,
    pub(super) tail_logs: DevTailLogsMode,
    pub(super) start_command: StartCommand,
    pub(super) auto_open_decision: AutoOpenDecision,
}

#[derive(Debug, Clone)]
pub(super) struct DevWatchPlan {
    pub(super) app_dir: PathBuf,
    pub(super) source_roots: Vec<PathBuf>,
    pub(super) debug_node_apis: bool,
    pub(super) tail_logs: DevTailLogsMode,
    pub(super) local_url: String,
    pub(super) deploy_admin_token: String,
}

impl DevPlan {
    pub(super) fn watch_plan(&self) -> DevWatchPlan {
        DevWatchPlan {
            app_dir: self.app_dir.clone(),
            source_roots: self
                .adapter
                .as_ref()
                .map(|adapter| adapter.source_roots().to_vec())
                .unwrap_or_default(),
            debug_node_apis: self.start_command.debug_node_apis,
            tail_logs: self.tail_logs,
            local_url: self.local_url.clone(),
            deploy_admin_token: self
                .start_command
                .deploy_admin_token
                .clone()
                .expect("dev plan should configure deploy activation token"),
        }
    }
}

pub(super) fn resolve_dev_plan(command: DevCommand, cwd: &Path) -> io::Result<DevPlan> {
    let auto_open_decision =
        resolve_auto_open(command.no_open, io::stdout().is_terminal(), &ProcessEnv);
    let app_dir = resolve_app_dir(command.app_dir.as_deref(), cwd)?;
    let adapter = detect_dev_adapter(&app_dir)?;
    let wire_surfaces = detect_wire_surfaces(&app_dir);
    let deployment_slug =
        dirs::deployment_slug(&app_dir).map_err(|error| io::Error::other(error.to_string()))?;
    let explicit_compose_files = command.compose_file.as_slice();
    let compose_selection = resolve_compose_selection(explicit_compose_files, cwd)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let data_dir = command
        .data_dir
        .as_deref()
        .map(|path| resolve_unchecked_path(path, cwd))
        .unwrap_or_else(|| app_dir.join(".nimbus").join("dev"));
    let local_url = format!("http://localhost:{}/", command.port);
    let deploy_admin_token = generate_dev_deploy_token();
    // A Firestore client app addresses `projects/{projectId}` directly, and
    // the serve side resolves that segment to the tenant of the same name —
    // so the auto-created tenant must be the discovered project id, not a
    // fixed default. Every other adapter shape keeps the standard demo
    // tenant.
    let firestore_tenant = match &adapter {
        Some(DevAdapter::FirestoreClient) => Some(discover_project_tenant(&app_dir)?),
        _ => None,
    };
    let auto_tenant = firestore_tenant
        .as_ref()
        .map(|mapping| mapping.tenant.clone())
        .unwrap_or_else(|| DEMO_TENANT.to_string());
    // A Firestore client app has no server-side authoring surface, so start
    // gets no app dir: the codegen preflight and registry loads stay off and
    // nothing ever writes `_generated/` into a client app. The dev-side app
    // dir (env file, scan-gated wiring, banner) is unaffected.
    let start_app_dir = match &adapter {
        Some(DevAdapter::FirestoreClient) => None,
        _ => Some(app_dir.clone()),
    };
    // Detected wire surfaces do not flip listener flags here yet: listeners
    // are deny-by-default on credentials, and the generated-credential story
    // (DXW2/DXW3) is what turns a detected surface into an enabled one.
    let start_command = StartCommand {
        port: command.port,
        // Firestore-compatible routes are always-on in dev: they mount on
        // the main HTTP listener and are inert without callers, so no
        // Firebase markers are required to serve them.
        firestore: true,
        data_dir: Some(data_dir.clone()),
        control_data_dir: Some(data_dir.clone()),
        tenant_provider: Some(CliTenantProvider::Sqlite),
        app_dir: start_app_dir,
        skip_codegen: command.skip_codegen,
        debug_node_apis: command.debug_node_apis,
        compose_file: command.compose_file,
        deploy_admin_token: Some(deploy_admin_token),
        auto_tenant: Some(auto_tenant),
        tenant_isolation_mode: nimbus_server::TenantIsolationMode::LocalDevelopment,
        ..StartCommand::default()
    };

    Ok(DevPlan {
        app_dir,
        data_dir,
        deployment_slug,
        compose_selection,
        local_url,
        adapter,
        firestore_tenant,
        wire_surfaces,
        once: command.once,
        tail_logs: command.tail_logs,
        start_command,
        auto_open_decision,
    })
}

pub(super) fn resolve_app_dir(explicit_app_dir: Option<&Path>, cwd: &Path) -> io::Result<PathBuf> {
    let selected = explicit_app_dir
        .map(|path| resolve_unchecked_path(path, cwd))
        .unwrap_or_else(|| detect_app_dir(cwd));
    canonicalize_dir(&selected)
}

pub(super) fn detect_app_dir(cwd: &Path) -> PathBuf {
    for candidate in cwd.ancestors() {
        if candidate.join("nimbus").is_dir()
            || candidate.join("convex").is_dir()
            || candidate
                .join(".nimbus")
                .join("convex")
                .join("functions.json")
                .is_file()
            || candidate.join("firebase.json").is_file()
            // The Firebase CLI's own project-root marker; a Firestore
            // client app may carry no other recognizable root file.
            || candidate.join(".firebaserc").is_file()
        {
            return candidate.to_path_buf();
        }
        // Stop the walk-up at the project's `.git` boundary so a sibling
        // `nimbus/`, `convex/`, or `firebase.json` *outside* the repo can
        // never accidentally become the app dir. See
        // `docs/private/plans/cli-daemon-canonicalization-plan.md` CD2.
        if crate::path_boundary::at_git_boundary(candidate) {
            break;
        }
    }
    cwd.to_path_buf()
}

fn resolve_unchecked_path(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn canonicalize_dir(path: &Path) -> io::Result<PathBuf> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("app directory {} is not readable: {error}", path.display()),
        )
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "app path {} is not a directory",
            path.display()
        )));
    }
    path.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to resolve app directory {}: {error}",
                path.display()
            ),
        )
    })
}

fn generate_dev_deploy_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut token, "{byte:02x}");
    }
    token
}
