use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use rand::RngCore;

use crate::compose::discovery::{ResolvedComposeSelection, resolve_compose_selection};
use crate::dirs;
use crate::network_composition::{PreparedLocalNetworkComposition, StagedLocalNetworkComposition};
use crate::start::{CliTenantProvider, StartCommand};

use super::adapter::{DevAdapter, detect_dev_adapter};
use super::firebase_project::{DEMO_TENANT, ProjectTenantMapping, discover_project_tenant};
use super::launch::{AutoOpenDecision, ProcessEnv, resolve_auto_open};
use super::surfaces::{WireSurfaces, detect_wire_surfaces};
#[cfg(test)]
use super::wire::reconstruct_direct_wire_plan_for_test;
use super::wire::{WirePlan, resolve_wire_plan};
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
    /// Resolved wire-listener ports + shared persisted credentials
    /// (D4/D5). Always resolved, because listeners are always available
    /// (D6) — detection only chooses port prominence and what
    /// `.env.local` carries.
    pub(super) wire: WirePlan,
    pub(super) once: bool,
    pub(super) tail_logs: DevTailLogsMode,
    pub(super) start_command: StartCommand,
    pub(super) auto_open_decision: AutoOpenDecision,
}

#[derive(Debug, Clone)]
pub(super) struct DevWatchPlan {
    pub(super) app_dir: PathBuf,
    pub(super) debug_node_apis: bool,
    pub(super) tail_logs: DevTailLogsMode,
    pub(super) local_url: String,
    pub(super) deploy_admin_token: String,
    pub(super) convex_silo: String,
}

impl DevPlan {
    pub(super) fn watch_plan(&self) -> DevWatchPlan {
        DevWatchPlan {
            app_dir: self.app_dir.clone(),
            debug_node_apis: self.start_command.debug_node_apis,
            tail_logs: self.tail_logs,
            local_url: self.local_url.clone(),
            deploy_admin_token: self
                .start_command
                .deploy_admin_token
                .clone()
                .expect("dev plan should configure deploy activation token"),
            convex_silo: self
                .start_command
                .auto_tenant
                .clone()
                .expect("dev plan should configure an auto tenant"),
        }
    }

    /// The boot-time source roots the codegen watch loop starts with. The
    /// roots are live state, not plan state: mid-session adapter adoption
    /// re-registers them through the watch channel seeded with this value.
    pub(super) fn initial_watch_roots(&self) -> Vec<PathBuf> {
        self.adapter
            .as_ref()
            .map(|adapter| adapter.source_roots().to_vec())
            .unwrap_or_default()
    }
}

pub(super) fn resolve_dev_plan_with_staged_network(
    command: DevCommand,
    cwd: &Path,
    staged_network: StagedLocalNetworkComposition,
) -> io::Result<(DevPlan, PreparedLocalNetworkComposition)> {
    let (plan, prepared_network) = resolve_dev_plan_inner(command, cwd, Some(staged_network))?;
    Ok((
        plan,
        prepared_network.expect("production dev planning must prepare the staged network"),
    ))
}

#[cfg(test)]
pub(super) fn resolve_dev_plan(command: DevCommand, cwd: &Path) -> io::Result<DevPlan> {
    resolve_dev_plan_inner(command, cwd, None).map(|(plan, _)| plan)
}

fn resolve_dev_plan_inner(
    command: DevCommand,
    cwd: &Path,
    staged_network: Option<StagedLocalNetworkComposition>,
) -> io::Result<(DevPlan, Option<PreparedLocalNetworkComposition>)> {
    let auto_open_decision =
        resolve_auto_open(command.no_open, io::stdout().is_terminal(), &ProcessEnv);
    let app_dir = resolve_app_dir(command.app_dir.as_deref(), cwd)?;
    let adapter = detect_dev_adapter(&app_dir)?;
    let wire_surfaces = detect_wire_surfaces(&app_dir);
    let deployment_slug =
        dirs::deployment_slug(&app_dir).map_err(|error| io::Error::other(error.to_string()))?;
    let explicit_compose_files = command.compose_file.as_slice();
    let compose_selection = if command.no_compose_discovery {
        None
    } else {
        resolve_compose_selection(explicit_compose_files, cwd)
            .map_err(|error| io::Error::other(error.to_string()))?
    };
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
    // A Firestore client app has no server-side authoring surface, and an
    // app with no detected adapter has none yet either (D8 keeps such a
    // session serving) — start gets no app dir in both shapes: the codegen
    // preflight and registry loads stay off and nothing ever writes
    // `_generated/` into the app. The dev-side app dir (env file,
    // scan-gated wiring, banner, live re-detection) is unaffected.
    let start_app_dir = match &adapter {
        Some(DevAdapter::FirestoreClient) | None => None,
        _ => Some(app_dir.clone()),
    };
    // The wire plan owns dev's port policy (D4: detected surfaces prefer
    // conventional ports, undetected go ephemeral) and the shared persisted
    // credentials (D5); the start command below serves exactly those
    // endpoints so `.env.local` and the run banner stay truthful.
    let prepared_network = staged_network
        .map(|staged| {
            PreparedLocalNetworkComposition::prepare(
                staged,
                compose_selection.as_ref(),
                &data_dir,
                nimbus_tenant::TenantIsolationMode::LocalDevelopment,
                nimbus_server::nimbus_owned_workload_ingress_registration(),
            )
            .map_err(|error| io::Error::other(error.to_string()))
        })
        .transpose()?;
    let prepared_wire = match prepared_network.as_ref() {
        Some(prepared) => resolve_wire_plan(wire_surfaces, &data_dir, prepared.authority())?,
        #[cfg(test)]
        None => reconstruct_direct_wire_plan_for_test(wire_surfaces, &data_dir)?,
        #[cfg(not(test))]
        None => unreachable!("production dev planning requires a manager-derived authority"),
    };
    let wire = prepared_wire.plan;
    let start_compose_files = compose_selection
        .as_ref()
        .map(|selection| selection.files.clone())
        .unwrap_or_else(|| command.compose_file.clone());
    let start_command = StartCommand {
        port: command.port,
        // Firestore-compatible routes are always-on in dev: they mount on
        // the main HTTP listener and are inert without callers, so no
        // Firebase markers are required to serve them.
        firestore: true,
        // Explicit ports pin start's listeners to the wire plan's choices —
        // start never re-probes or silently skips what dev advertised.
        mongodb_port: Some(wire.mongodb_port.port),
        dynamodb_port: Some(wire.dynamodb_port.port),
        s3_port: Some(wire.s3_port.port),
        // The store credentials back the listeners directly: MongoDB via
        // the store-only marker (ambient NIMBUS_MONGODB_* env in the
        // developer's shell must not desync the listener from what
        // `.env.local` advertises), DynamoDB/S3 via explicit bindings to
        // the dev auto-tenant (which shadows NIMBUS_DYNAMODB_ACCESS_KEYS).
        mongodb_credentials_from_store: true,
        dynamodb_access_key: vec![format!(
            "{}:{}:{}",
            wire.credentials.dynamodb_access_key_id,
            wire.credentials.dynamodb_secret_access_key,
            auto_tenant
        )],
        s3_access_key: vec![format!(
            "{}:{}:{}",
            wire.credentials.s3_access_key_id, wire.credentials.s3_secret_access_key, auto_tenant
        )],
        data_dir: Some(data_dir.clone()),
        control_data_dir: Some(data_dir.clone()),
        network_state_dir: prepared_network
            .as_ref()
            .map(|prepared| prepared.authority().state_root().to_path_buf()),
        tenant_provider: Some(CliTenantProvider::Sqlite),
        app_dir: start_app_dir,
        skip_codegen: command.skip_codegen,
        debug_node_apis: command.debug_node_apis,
        compose_file: start_compose_files,
        deploy_admin_token: Some(deploy_admin_token),
        auto_tenant: Some(auto_tenant),
        prebound_wire_listeners: Some(prepared_wire.listeners),
        tenant_isolation_mode: nimbus_tenant::TenantIsolationMode::LocalDevelopment,
        ..StartCommand::default()
    };

    Ok((
        DevPlan {
            app_dir,
            data_dir,
            deployment_slug,
            compose_selection,
            local_url,
            adapter,
            firestore_tenant,
            wire_surfaces,
            wire,
            once: command.once,
            tail_logs: command.tail_logs,
            start_command,
            auto_open_decision,
        },
        prepared_network,
    ))
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
