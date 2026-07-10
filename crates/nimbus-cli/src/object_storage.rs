use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, Subcommand, ValueEnum};
use nimbus::{
    BackupBundle, BackupRequest, EmbeddedProviderKind, Engine, EnginePersistenceConfig,
    ErasureBlobStore, ErasureConfig, ErasureHealer, Error as NimbusError, HealPacing, HealReport,
    KeyEscrow, LocalLeg, LocalPackStore, ObjectBackup, ObjectPlacement, ObjectStorageConfig,
    ObjectStorageResolver, ObjectStorePlacementTarget, ObjectStoreProviderCredentials,
    ObjectStoreProviderKind, PlacementPolicy, PointInTimeRestoreArchive, TenantId,
    object_backup_roots, object_blob_root,
};
use nimbus_core::StorageErrorKind;
use rand::RngCore;
use serde::Serialize;

use crate::cli_ux;

#[derive(Debug, Subcommand)]
pub(crate) enum ObjectStorageCommand {
    /// Persist the byte-plane placement policy for a tenant.
    #[command(name = "set-placement")]
    SetPlacement(SetPlacementCommand),
    /// Write a single-file object-store bundle for one tenant.
    #[command(name = "backup-object-store")]
    BackupObjectStore(BackupObjectStoreCommand),
    /// Restore one tenant from an object-store bundle.
    #[command(name = "restore-object-store")]
    RestoreObjectStore(RestoreObjectStoreCommand),
    /// Create the local object-storage master key file with private permissions.
    #[command(name = "bootstrap-master-key")]
    BootstrapMasterKey(BootstrapMasterKeyCommand),
    /// Inspect local byte-plane GC status for a tenant.
    #[command(name = "gc-status")]
    GcStatus(GcStatusCommand),
    /// Inspect one tenant's erasure leg without taking its drive locks.
    #[command(name = "erasure-status")]
    ErasureStatus(ErasureStatusCommand),
    /// Heal one tenant's erasure leg as offline maintenance.
    #[command(name = "erasure-heal")]
    ErasureHeal(ErasureHealCommand),
    /// Tenant object-storage lifecycle commands.
    #[command(subcommand)]
    Tenant(TenantObjectStorageCommand),
}

#[derive(Debug, Subcommand)]
pub(crate) enum TenantObjectStorageCommand {
    /// Remove a tenant's metadata and local object byte plane.
    #[command(name = "rm")]
    Rm(TenantRemoveCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ObjectStorageProvider {
    Sqlite,
    Redb,
}

impl ObjectStorageProvider {
    fn embedded_kind(self) -> EmbeddedProviderKind {
        match self {
            Self::Sqlite => EmbeddedProviderKind::Sqlite,
            Self::Redb => EmbeddedProviderKind::Redb,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ObjectPlacementMode {
    Local,
    Mirror,
    Tier,
    CloudPrimary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ObjectTargetProvider {
    S3,
    Gcs,
    Azure,
    Local,
    Memory,
}

impl ObjectTargetProvider {
    fn into_storage(self) -> ObjectStoreProviderKind {
        match self {
            Self::S3 => ObjectStoreProviderKind::S3,
            Self::Gcs => ObjectStoreProviderKind::Gcs,
            Self::Azure => ObjectStoreProviderKind::Azure,
            Self::Local => ObjectStoreProviderKind::Local,
            Self::Memory => ObjectStoreProviderKind::Memory,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ObjectCredentialSource {
    Anonymous,
    Environment,
    SecretRef,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct SetPlacementCommand {
    /// Local data directory of the deployment.
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,
    /// Embedded tenant persistence provider.
    #[arg(long, value_enum, default_value_t = ObjectStorageProvider::Sqlite)]
    pub(crate) provider: ObjectStorageProvider,
    /// Tenant whose object placement policy is being changed.
    #[arg(long)]
    pub(crate) tenant: String,
    /// Placement mode to persist.
    #[arg(long, value_enum)]
    pub(crate) mode: ObjectPlacementMode,
    /// Cloud/object-store provider for non-local placement modes.
    #[arg(long, value_enum, default_value_t = ObjectTargetProvider::S3)]
    pub(crate) target_provider: ObjectTargetProvider,
    /// Provider bucket/container for non-local placement modes.
    #[arg(long)]
    pub(crate) bucket: Option<String>,
    /// Optional provider region.
    #[arg(long)]
    pub(crate) region: Option<String>,
    /// Optional custom endpoint URL.
    #[arg(long)]
    pub(crate) endpoint: Option<String>,
    /// Optional provider prefix below the bucket/container.
    #[arg(long, default_value = "")]
    pub(crate) prefix: String,
    /// Credential source for non-local placement modes.
    #[arg(long, value_enum, default_value_t = ObjectCredentialSource::Environment)]
    pub(crate) credentials: ObjectCredentialSource,
    /// Secret reference id when --credentials=secret-ref.
    #[arg(long)]
    pub(crate) secret_ref: Option<String>,
    /// Mirror writes must wait for the remote leg before succeeding.
    #[arg(long, default_value_t = false)]
    pub(crate) require_ack: bool,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct BackupObjectStoreCommand {
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,
    #[arg(long, value_enum, default_value_t = ObjectStorageProvider::Sqlite)]
    pub(crate) provider: ObjectStorageProvider,
    #[arg(long)]
    pub(crate) tenant: String,
    #[arg(long)]
    pub(crate) out: PathBuf,
    #[arg(long)]
    pub(crate) key_escrow_id: String,
    #[arg(long)]
    pub(crate) key_escrow_file: PathBuf,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct RestoreObjectStoreCommand {
    #[arg(long = "in")]
    pub(crate) input: PathBuf,
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,
    #[arg(long, value_enum, default_value_t = ObjectStorageProvider::Sqlite)]
    pub(crate) provider: ObjectStorageProvider,
    #[arg(long)]
    pub(crate) tenant: String,
    #[arg(long)]
    pub(crate) key_escrow_id: String,
    #[arg(long)]
    pub(crate) key_escrow_file: PathBuf,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct BootstrapMasterKeyCommand {
    /// Master-key file to create. The command refuses to overwrite it.
    #[arg(long)]
    pub(crate) path: PathBuf,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct GcStatusCommand {
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,
    #[arg(long)]
    pub(crate) tenant: String,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct ErasureStatusCommand {
    /// Tenant whose deployment-level erasure leg is being inspected.
    #[arg(long)]
    pub(crate) tenant: String,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = "Exit codes:\n  0  Heal completed with no beyond-repair blobs\n  1  Operational error\n  2  One or more blobs are beyond repair"
)]
pub(crate) struct ErasureHealCommand {
    /// Tenant whose deployment-level erasure leg is being healed.
    #[arg(long)]
    pub(crate) tenant: String,
    /// Maximum erasure payload bytes repaired during this run.
    #[arg(long)]
    pub(crate) max_bytes: Option<u64>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct TenantRemoveCommand {
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,
    #[arg(long, value_enum, default_value_t = ObjectStorageProvider::Sqlite)]
    pub(crate) provider: ObjectStorageProvider,
    #[arg(long)]
    pub(crate) tenant: String,
    /// Required confirmation for destructive tenant removal.
    #[arg(long)]
    pub(crate) yes: bool,
}

pub(crate) async fn run_object_storage_command(
    command: ObjectStorageCommand,
) -> Result<(), Box<dyn Error>> {
    match command {
        ObjectStorageCommand::SetPlacement(command) => run_set_placement(command).await,
        ObjectStorageCommand::BackupObjectStore(command) => run_backup_object_store(command).await,
        ObjectStorageCommand::RestoreObjectStore(command) => {
            run_restore_object_store(command).await
        }
        ObjectStorageCommand::BootstrapMasterKey(command) => run_bootstrap_master_key(command),
        ObjectStorageCommand::GcStatus(command) => run_gc_status(command),
        ObjectStorageCommand::ErasureStatus(command) => run_erasure_status(command).await,
        ObjectStorageCommand::ErasureHeal(command) => run_erasure_heal(command).await,
        ObjectStorageCommand::Tenant(TenantObjectStorageCommand::Rm(command)) => {
            run_tenant_rm(command).await
        }
    }
}

async fn run_set_placement(command: SetPlacementCommand) -> Result<(), Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant.clone())?;
    let policy = placement_policy_from_command(&command)?;
    let engine = open_engine(&command.data_dir, command.provider).await?;
    let placement = ObjectPlacement::new(tenant.clone(), policy, current_unix_ms()?);
    engine.set_object_placement(placement)?;
    engine.quiesce().await;
    emit_object_storage_info(format!(
        "set-placement tenant={} data_dir={}",
        tenant,
        command.data_dir.display()
    ));
    Ok(())
}

async fn run_backup_object_store(command: BackupObjectStoreCommand) -> Result<(), Box<dyn Error>> {
    if command.out.exists() {
        return Err(format!(
            "backup-object-store target {} already exists; refusing to overwrite",
            command.out.display()
        )
        .into());
    }
    let tenant = TenantId::new(command.tenant)?;
    let engine = open_engine(&command.data_dir, command.provider).await?;
    let archive = engine.export_latest_point_in_time_restore_archive(&tenant)?;
    let archive_bytes = serde_json::to_vec(&archive)?;
    let roots = backup_roots_from_archive_or_local(&archive, &command.data_dir, &tenant)?;
    let source = object_storage_resolver(engine.clone())?.blob_store(&tenant)?;
    let key_escrow = read_key_escrow(&command.key_escrow_id, &command.key_escrow_file)?;
    let request = BackupRequest::new(
        roots,
        archive_bytes.clone().into(),
        archive_bytes.into(),
        key_escrow,
    )?;
    let bundle = ObjectBackup::export_bundle(source.as_ref(), request).await?;
    let encoded = bundle.encode();
    if let Some(parent) = command.out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&command.out, &encoded)?;
    engine.quiesce().await;
    emit_object_storage_info(format!(
        "backup-object-store tenant={} chunks={} bytes={} out={}",
        tenant,
        bundle.chunks().len(),
        encoded.len(),
        command.out.display()
    ));
    Ok(())
}

async fn run_restore_object_store(
    command: RestoreObjectStoreCommand,
) -> Result<(), Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant)?;
    let raw = std::fs::read(&command.input)?;
    let bundle = BackupBundle::decode(raw.into())?;
    let key_escrow = read_key_escrow(&command.key_escrow_id, &command.key_escrow_file)?;
    let engine = open_engine(&command.data_dir, command.provider).await?;
    let target = object_storage_resolver(engine.clone())?.blob_store(&tenant)?;
    let report = ObjectBackup::restore_bundle(target.as_ref(), &bundle, Some(&key_escrow)).await?;
    let archive = serde_json::from_slice(bundle.manifest_snapshot())?;
    match engine.create_tenant(tenant.clone()) {
        Ok(()) | Err(NimbusError::AlreadyExists(_)) => {}
        Err(error) => return Err(error.into()),
    }
    engine.import_point_in_time_restore_archive(&tenant, &archive)?;
    engine.quiesce().await;
    emit_object_storage_info(format!(
        "restore-object-store tenant={} chunks={} bytes={} input={}",
        tenant,
        report.restored_chunks,
        report.restored_bytes,
        command.input.display()
    ));
    Ok(())
}

fn run_bootstrap_master_key(command: BootstrapMasterKeyCommand) -> Result<(), Box<dyn Error>> {
    bootstrap_object_master_key(&command.path)?;
    emit_object_storage_info(format!(
        "bootstrap-master-key path={}",
        command.path.display()
    ));
    Ok(())
}

fn run_gc_status(command: GcStatusCommand) -> Result<(), Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant)?;
    if matches!(local_leg_from_env()?, LocalLeg::Erasure(_)) {
        return Err(
            "gc-status inspects the pack leg; this deployment's local leg is erasure — \
             use `object-storage erasure-status` instead"
                .into(),
        );
    }
    let root = object_blob_root(&command.data_dir, &tenant);
    // Read-only inspection: coexists with a running server that holds the
    // root's exclusive write lock.
    let store = LocalPackStore::open_read_only(&root)?;
    let live = store.live_entries()?;
    emit_object_storage_info(format!(
        "gc-status tenant={} live_blobs={} root={}",
        tenant,
        live.len(),
        root.display()
    ));
    Ok(())
}

async fn run_erasure_status(command: ErasureStatusCommand) -> Result<(), Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant)?;
    let config = erasure_config_from_env(&tenant)?;
    let roots = config.drives.clone();
    let stats = ErasureBlobStore::open_read_only(config)?.stats().await?;
    let view = ErasureStatusView::new(&tenant, &roots, &stats);
    if command.json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        // last_heal is deliberately NOT shown: the summary lives in the
        // process-local leg registry, so a one-shot CLI invocation can
        // never observe a previous process's heal — erasure-heal itself
        // prints its full report.
        println!(
            "erasure-status tenant={} blob_count={}",
            tenant, stats.blob_count
        );
        let rows = view
            .drives
            .iter()
            .map(|drive| {
                vec![
                    drive.index.to_string(),
                    drive.root.clone(),
                    drive.live_bytes.to_string(),
                    drive.reclaimable_bytes.to_string(),
                    drive.quarantined_bytes.to_string(),
                    drive.pack_count.to_string(),
                ]
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            cli_ux::render_table_with_options(
                &[
                    cli_ux::TableColumn::right("DRIVE", 5),
                    cli_ux::TableColumn::left("ROOT", 10),
                    cli_ux::TableColumn::right("LIVE_BYTES", 10),
                    cli_ux::TableColumn::right("RECLAIMABLE_BYTES", 17),
                    cli_ux::TableColumn::right("QUARANTINED_BYTES", 17),
                    cli_ux::TableColumn::right("PACKS", 5),
                ],
                &rows,
                cli_ux::TableRenderOptions::default(),
            )
        );
    }
    Ok(())
}

async fn run_erasure_heal(command: ErasureHealCommand) -> Result<(), Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant)?;
    let config = erasure_config_from_env(&tenant)?;
    let store = match ErasureBlobStore::open(config) {
        Ok(store) => store,
        Err(error) if error.storage_kind() == Some(StorageErrorKind::Busy) => {
            return Err(format!(
                "{error}; stop the server or run via the server; erasure heal is offline maintenance"
            )
            .into());
        }
        Err(error) => return Err(error.into()),
    };
    let mut healer = ErasureHealer::new(store);
    if let Some(max_bytes) = command.max_bytes {
        healer = healer.with_pacing(HealPacing::max_bytes_per_run(max_bytes)?);
    }
    let report = healer.heal().await?;
    let view = HealReportView::from(&report);
    if command.json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        println!(
            "erasure-heal tenant={} blobs_examined={} stripes_repaired={} shards_rewritten={} degraded={} beyond_repair={} exhausted={} at_millis={}",
            tenant,
            report.blobs_examined,
            report.stripes_repaired,
            report.shards_rewritten,
            report.degraded,
            report.beyond_repair.len(),
            report.exhausted,
            report.at_millis,
        );
        for hash in &report.beyond_repair {
            println!("beyond_repair={}", hash.to_hex());
        }
    }
    if !report.beyond_repair.is_empty() {
        std::io::Write::flush(&mut std::io::stdout())?;
        std::process::exit(2);
    }
    Ok(())
}

/// The deployment's configured local leg (env-resolved) — maintenance verbs
/// must follow the SAME layout the server uses, or refuse explicitly;
/// succeeding with pack semantics against an erasure deployment produced
/// incomplete backups and incomplete tenant deletion.
fn local_leg_from_env() -> Result<LocalLeg, Box<dyn Error>> {
    Ok(ObjectStorageConfig::from_env(None)?.local_leg().clone())
}

fn erasure_config_from_env(tenant: &TenantId) -> Result<ErasureConfig, Box<dyn Error>> {
    let config = ObjectStorageConfig::from_env(None)?;
    let LocalLeg::Erasure(erasure) = config.local_leg() else {
        return Err(
            "object-storage local leg is not erasure; set NIMBUS_OBJECT_STORAGE_LOCAL_LEG=erasure"
                .into(),
        );
    };
    let tenant_leaf = object_blob_root(Path::new(""), tenant)
        .file_name()
        .ok_or("tenant object blob root has no directory-name component")?
        .to_os_string();
    let roots = erasure
        .drives
        .iter()
        .map(|drive| drive.join(&tenant_leaf))
        .collect();
    Ok(ErasureConfig::new(
        tenant.as_str(),
        roots,
        erasure.data_shards,
        erasure.parity_shards,
        erasure.stripe_width,
    )?)
}

#[derive(Serialize)]
struct ErasureStatusView {
    tenant: String,
    blob_count: usize,
    drives: Vec<ErasureDriveStatusView>,
}

impl ErasureStatusView {
    fn new(tenant: &TenantId, roots: &[PathBuf], stats: &nimbus::ErasureStats) -> Self {
        let drives = roots
            .iter()
            .zip(&stats.per_drive)
            .enumerate()
            .map(|(index, (root, drive))| ErasureDriveStatusView {
                index,
                root: root.display().to_string(),
                live_bytes: drive.live_bytes,
                reclaimable_bytes: drive.reclaimable_bytes,
                quarantined_bytes: drive.quarantined_bytes,
                pack_count: drive.pack_count,
            })
            .collect();
        Self {
            tenant: tenant.to_string(),
            blob_count: stats.blob_count,
            drives,
        }
    }
}

#[derive(Serialize)]
struct ErasureDriveStatusView {
    index: usize,
    root: String,
    live_bytes: u64,
    reclaimable_bytes: u64,
    quarantined_bytes: u64,
    pack_count: usize,
}

#[derive(Serialize)]
struct HealReportView {
    blobs_examined: usize,
    stripes_repaired: usize,
    shards_rewritten: usize,
    degraded: usize,
    beyond_repair: Vec<String>,
    exhausted: bool,
    at_millis: u64,
}

impl From<&HealReport> for HealReportView {
    fn from(report: &HealReport) -> Self {
        Self {
            blobs_examined: report.blobs_examined,
            stripes_repaired: report.stripes_repaired,
            shards_rewritten: report.shards_rewritten,
            degraded: report.degraded,
            beyond_repair: report
                .beyond_repair
                .iter()
                .map(|hash| hash.to_hex())
                .collect(),
            exhausted: report.exhausted,
            at_millis: report.at_millis,
        }
    }
}

async fn run_tenant_rm(command: TenantRemoveCommand) -> Result<(), Box<dyn Error>> {
    if !command.yes {
        return Err("tenant rm requires --yes".into());
    }
    let tenant = TenantId::new(command.tenant)?;
    // Resolve configuration and PROVE exclusive ownership of every byte-
    // plane root BEFORE any destructive step: config errors must not strike
    // after partial deletion, and unlinking a tree a running server still
    // owns (per-drive flocks) would corrupt its live state. Opening the
    // stores writable takes the same flocks the server holds — Busy here
    // means offline maintenance is required.
    let local_leg = local_leg_from_env()?;
    let root = object_blob_root(&command.data_dir, &tenant);
    let erasure_trees: Vec<PathBuf> = match &local_leg {
        LocalLeg::Pack => Vec::new(),
        LocalLeg::Erasure(erasure) => {
            let tenant_leaf = root
                .file_name()
                .ok_or("tenant object blob root has no directory-name component")?
                .to_os_string();
            erasure
                .drives
                .iter()
                .map(|drive| drive.join(&tenant_leaf))
                .collect()
        }
    };
    if let LocalLeg::Erasure(_) = &local_leg {
        let existing: Vec<PathBuf> = erasure_trees
            .iter()
            .filter(|tree| tree.exists())
            .cloned()
            .collect();
        if !existing.is_empty() {
            let ownership = ErasureBlobStore::open(erasure_config_from_env(&tenant)?).map_err(
                |err| -> Box<dyn Error> {
                    format!(
                        "tenant rm requires exclusive ownership of the erasure drives \
                         (stop the server first — this is offline maintenance): {err}"
                    )
                    .into()
                },
            )?;
            drop(ownership);
        }
    }
    let engine = open_engine(&command.data_dir, command.provider).await?;
    engine.delete_tenant_async(tenant.clone()).await?;
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    // Erasure deployments keep the tenant's byte plane under
    // <drive_i>/<tenant leaf>; removing only the sidecar root while
    // reporting success would leave every shard and manifest behind.
    let mut erasure_trees_removed = 0usize;
    for tree in &erasure_trees {
        if tree.exists() {
            std::fs::remove_dir_all(tree)?;
            erasure_trees_removed += 1;
        }
    }
    engine.quiesce().await;
    emit_object_storage_info(format!(
        "tenant rm tenant={} object_blobs_removed=true erasure_trees_removed={}",
        tenant, erasure_trees_removed
    ));
    Ok(())
}

fn placement_policy_from_command(
    command: &SetPlacementCommand,
) -> Result<PlacementPolicy, Box<dyn Error>> {
    match command.mode {
        ObjectPlacementMode::Local => Ok(PlacementPolicy::LocalOnly),
        ObjectPlacementMode::Mirror => Ok(PlacementPolicy::Mirror {
            target: placement_target_from_command(command)?,
            require_ack: command.require_ack,
        }),
        ObjectPlacementMode::Tier => Ok(PlacementPolicy::Tier {
            target: placement_target_from_command(command)?,
        }),
        ObjectPlacementMode::CloudPrimary => Ok(PlacementPolicy::CloudPrimary {
            target: placement_target_from_command(command)?,
        }),
    }
}

fn placement_target_from_command(
    command: &SetPlacementCommand,
) -> Result<ObjectStorePlacementTarget, Box<dyn Error>> {
    let bucket = command
        .bucket
        .as_ref()
        .ok_or("non-local object placement requires --bucket")?;
    let mut target = ObjectStorePlacementTarget::new(
        command.target_provider.into_storage(),
        bucket,
        credentials_from_command(command)?,
    )?
    .with_prefix(command.prefix.clone());
    if let Some(region) = &command.region {
        target = target.with_region(region.clone());
    }
    if let Some(endpoint) = &command.endpoint {
        target = target.with_endpoint(endpoint.clone());
    }
    Ok(target)
}

fn credentials_from_command(
    command: &SetPlacementCommand,
) -> Result<ObjectStoreProviderCredentials, Box<dyn Error>> {
    match command.credentials {
        ObjectCredentialSource::Anonymous => Ok(ObjectStoreProviderCredentials::Anonymous),
        ObjectCredentialSource::Environment => Ok(ObjectStoreProviderCredentials::Environment),
        ObjectCredentialSource::SecretRef => {
            let id = command
                .secret_ref
                .clone()
                .ok_or("--credentials=secret-ref requires --secret-ref")?;
            Ok(ObjectStoreProviderCredentials::SecretRef { id })
        }
    }
}

async fn open_engine(
    data_dir: &Path,
    provider: ObjectStorageProvider,
) -> Result<Arc<Engine>, Box<dyn Error>> {
    let config = EnginePersistenceConfig::embedded(data_dir, provider.embedded_kind());
    Ok(Arc::new(Engine::new_with_persistence_config(config).await?))
}

fn object_storage_resolver(engine: Arc<Engine>) -> Result<ObjectStorageResolver, Box<dyn Error>> {
    Ok(ObjectStorageResolver::with_config(
        engine,
        ObjectStorageConfig::from_env(None)?,
    ))
}

fn read_key_escrow(id: &str, path: &Path) -> Result<KeyEscrow, Box<dyn Error>> {
    Ok(KeyEscrow::new(id, std::fs::read(path)?.into())?)
}

fn backup_roots_from_archive_or_local(
    archive: &PointInTimeRestoreArchive,
    data_dir: &Path,
    tenant: &TenantId,
) -> Result<Vec<nimbus::BlobHash>, Box<dyn Error>> {
    match object_backup_roots(archive) {
        Ok(roots) if !roots.is_empty() => Ok(roots),
        Ok(_) | Err(_) => {
            // Read-only enumeration: must not contend for the root's
            // exclusive write lock. The fallback must follow the
            // DEPLOYMENT'S local leg — walking the (empty) pack root in an
            // erasure deployment silently produced complete-looking but
            // empty backup root sets.
            if matches!(local_leg_from_env()?, LocalLeg::Erasure(_)) {
                let erasure = ErasureBlobStore::open_read_only(erasure_config_from_env(tenant)?)?;
                let runtime = tokio::runtime::Handle::try_current();
                let hashes = match runtime {
                    Ok(handle) => tokio::task::block_in_place(|| {
                        handle.block_on(erasure.visible_blob_hashes())
                    }),
                    Err(_) => tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?
                        .block_on(erasure.visible_blob_hashes()),
                };
                return Ok(hashes?);
            }
            let local = LocalPackStore::open_read_only(object_blob_root(data_dir, tenant))?;
            Ok(local
                .live_entries()?
                .into_iter()
                .map(|entry| entry.hash)
                .collect())
        }
    }
}

pub(crate) fn bootstrap_object_master_key(path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
        set_private_dir_permissions(parent)?;
    }
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(&mut options.open(path)?, &key)?;
    set_private_file_permissions(path)?;
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> std::io::Result<()> {
    // Off unix the key file relies on the platform's default ACLs; Windows
    // ACL tightening is windows-machine-support-plan territory.
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn set_private_dir_permissions(path: &Path) -> std::io::Result<()> {
    // Off unix the key directory relies on the platform's default ACLs;
    // Windows ACL tightening is windows-machine-support-plan territory.
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn current_unix_ms() -> Result<u64, Box<dyn Error>> {
    Ok(nimbus_core::clock::system_now_millis())
}

fn emit_object_storage_info(message: impl AsRef<str>) {
    if cli_ux::info_output_enabled() {
        let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
    } else {
        println!("{}", message.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Command};

    #[test]
    fn cli_parses_object_storage_operator_verbs() {
        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "set-placement",
            "--tenant",
            "tenant-a",
            "--mode",
            "mirror",
            "--bucket",
            "tenant-bucket",
            "--credentials",
            "secret-ref",
            "--secret-ref",
            "s3/tenant-a",
        ]);
        let Command::ObjectStorage(ObjectStorageCommand::SetPlacement(command)) = cli.command
        else {
            panic!("set-placement should parse");
        };
        assert_eq!(command.tenant, "tenant-a");
        assert_eq!(command.mode, ObjectPlacementMode::Mirror);

        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "backup-object-store",
            "--tenant",
            "tenant-a",
            "--out",
            "bundle.nobb",
            "--key-escrow-id",
            "tenant-a",
            "--key-escrow-file",
            "tenant-a.key",
        ]);
        assert!(matches!(
            cli.command,
            Command::ObjectStorage(ObjectStorageCommand::BackupObjectStore(_))
        ));

        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "restore-object-store",
            "--tenant",
            "tenant-a",
            "--in",
            "bundle.nobb",
            "--key-escrow-id",
            "tenant-a",
            "--key-escrow-file",
            "tenant-a.key",
        ]);
        assert!(matches!(
            cli.command,
            Command::ObjectStorage(ObjectStorageCommand::RestoreObjectStore(_))
        ));

        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "bootstrap-master-key",
            "--path",
            "object.master",
        ]);
        assert!(matches!(
            cli.command,
            Command::ObjectStorage(ObjectStorageCommand::BootstrapMasterKey(_))
        ));

        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "gc-status",
            "--tenant",
            "tenant-a",
        ]);
        assert!(matches!(
            cli.command,
            Command::ObjectStorage(ObjectStorageCommand::GcStatus(_))
        ));

        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "erasure-status",
            "--tenant",
            "tenant-a",
            "--json",
        ]);
        assert!(matches!(
            cli.command,
            Command::ObjectStorage(ObjectStorageCommand::ErasureStatus(_))
        ));

        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "erasure-heal",
            "--tenant",
            "tenant-a",
            "--max-bytes",
            "1048576",
            "--json",
        ]);
        assert!(matches!(
            cli.command,
            Command::ObjectStorage(ObjectStorageCommand::ErasureHeal(_))
        ));

        let cli = Cli::parse_from([
            "nimbus",
            "object-storage",
            "tenant",
            "rm",
            "--tenant",
            "tenant-a",
            "--yes",
        ]);
        assert!(matches!(
            cli.command,
            Command::ObjectStorage(ObjectStorageCommand::Tenant(
                TenantObjectStorageCommand::Rm(_)
            ))
        ));
    }

    #[test]
    #[cfg(unix)]
    fn bootstrap_object_master_key_creates_0600_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir should create");
        let key_path = temp.path().join("keys/object.master");
        bootstrap_object_master_key(&key_path).expect("master key should bootstrap");
        let metadata = std::fs::metadata(&key_path).expect("master key metadata should stat");
        assert_eq!(metadata.len(), 32);
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
}
