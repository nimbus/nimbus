use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, Subcommand, ValueEnum};
use nimbus::{
    BackupBundle, BackupRequest, EmbeddedProviderKind, Engine, EnginePersistenceConfig,
    ErasureBlobStore, ErasureConfig, ErasureHealer, Error as NimbusError, HealPacing, HealReport,
    KeyEscrow, LocalLeg, LocalPackStore, ObjectBackup, ObjectPlacement, ObjectStorageConfig,
    ObjectStorePlacementTarget, ObjectStoreProviderCredentials, ObjectStoreProviderKind,
    PlacementPolicy, PointInTimeRestoreArchive, TenantId, object_backup_roots, object_blob_root,
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
    after_help = "Exit codes:\n  0  Heal completed: nothing deferred, no beyond-repair blobs\n  1  Operational error\n  3  One or more blobs are beyond repair\n  4  Byte budget exhausted before all repairs ran — re-run or raise --max-bytes\n  (2 is reserved by the CLI parser for usage errors)"
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
    let roots = backup_roots_from_archive_or_local(&archive, &command.data_dir, &tenant).await?;
    // Export reads the RAW byte-plane leg: roots are ciphertext content
    // addresses, and export_bundle verifies each chunk re-hashes to its
    // address — the resolver's encrypted store serves PLAINTEXT for those
    // addresses, which can never verify (and would leak plaintext into the
    // backup artifact if it did). Chunks therefore stay ciphertext and the
    // bundle is unreadable without the escrowed key material.
    let source = raw_local_leg_read_only(&command.data_dir, &tenant)?;
    let key_escrow = read_key_escrow(&command.key_escrow_id, &command.key_escrow_file)?;
    // Fail EARLY if the operator escrowed the wrong material: the escrow
    // must be the tenant's wrapped-DEK sidecar, or the restored ciphertext
    // will never open. (Restore installs exactly these bytes.)
    let sidecar_path = nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(
        &command.data_dir,
        &tenant,
    ));
    let sidecar = std::fs::read(&sidecar_path).map_err(|err| -> Box<dyn Error> {
        format!(
            "read tenant blob-key sidecar {} (required to validate escrow): {err}",
            sidecar_path.display()
        )
        .into()
    })?;
    if key_escrow.wrapped_key_material() != sidecar.as_slice() {
        return Err(format!(
            "key escrow does not match the tenant's wrapped-DEK sidecar {} — escrow \
             exactly that file's bytes, or the restored ciphertext will be unreadable",
            sidecar_path.display()
        )
        .into());
    }
    let request = BackupRequest::new(
        roots,
        archive_bytes.clone().into(),
        archive_bytes.into(),
        key_escrow,
    )?;
    let bundle = ObjectBackup::export_bundle(source.as_ref(), request).await?;
    drop(source);
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
    // Fail BEFORE any filesystem mutation if the presented escrow is not
    // the one recorded in this bundle: installing a different (even valid)
    // manifest would poison a fresh target — the failed restore leaves the
    // wrong sidecar behind and the differing-sidecar check then rejects
    // the correct retry.
    if &key_escrow != bundle.key_escrow() {
        return Err(format!(
            "key escrow {} does not match the escrow recorded in the backup bundle — supply the escrow captured by the matching backup run",
            command.key_escrow_file.display()
        )
        .into());
    }
    let engine = open_engine(&command.data_dir, command.provider).await?;
    // Install the escrowed wrapped-DEK sidecar BEFORE restoring bytes: the
    // bundle's chunks are ciphertext sealed under that key, and a fresh
    // tenant would otherwise mint a NEW DEK and never be able to read
    // them. A pre-existing DIFFERENT sidecar fails closed — restore must
    // not clobber a live tenant's key material.
    let sidecar_path = nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(
        &command.data_dir,
        &tenant,
    ));
    let escrow_bytes = key_escrow.wrapped_key_material();
    // Validate the escrow BEFORE touching the target deployment: it must
    // parse as a key manifest, be wrapped under THIS deployment's
    // master-key path (the resolver refuses provider-identity mismatches
    // at open, so installing a foreign-path manifest would only defer the
    // failure), and actually unwrap under the LOCAL master key for THIS
    // tenant's blob-key subject — proving key bytes, tenant binding, and
    // manifest integrity in one step. Parse the SAME captured bytes that
    // will be installed — never a second read of the escrow path.
    let escrow_manifest = nimbus_crypto::KeyManifest::from_bytes(
        escrow_bytes,
        &command.key_escrow_file,
    )
    .map_err(|err| -> Box<dyn Error> {
        format!(
            "key escrow file {} is not a valid key manifest — escrow the tenant's blob-key sidecar ({}): {err}",
            command.key_escrow_file.display(),
            sidecar_path.display()
        )
        .into()
    })?;
    let master_key_path = master_key_path_from_env(&command.data_dir)?;
    let provider =
        nimbus_crypto::MasterKeyFileProvider::new(master_key_path.clone()).map_err(|err| {
            format!(
                "open master key file {} (install the source deployment's master key before restore): {err}",
                master_key_path.display()
            )
        })?;
    // Subject naming matches the resolver's blob-key sidecar subject.
    let subject = nimbus_crypto::LocalKeySubject::object_blob_store(tenant.clone(), "blob-key");
    let protected_path = nimbus::object_blob_key_path(&command.data_dir, &tenant);
    // Full validation in one step: subject + cipher + provider-identity
    // checks, then an actual unwrap under the LOCAL master key — proving
    // key bytes, tenant binding, layout, and manifest integrity before any
    // bytes land in the target deployment.
    nimbus_crypto::unwrap_key_manifest(
        &escrow_manifest,
        &provider,
        &subject,
        nimbus_crypto::ManifestCipher::FramedBlobAes256GcmSiv,
        &protected_path,
    )
    .map_err(|err| -> Box<dyn Error> {
        format!(
            "escrowed key manifest is not usable by this deployment for tenant {tenant} — wrong escrow, wrong tenant, different data-dir/master-key layout, or wrong master key: {err}"
        )
        .into()
    })?;
    match std::fs::read(&sidecar_path) {
        Ok(existing) if existing.as_slice() == escrow_bytes => {}
        Ok(_) => {
            return Err(format!(
                "tenant blob-key sidecar {} already exists with DIFFERENT key material; \
                 refusing to overwrite a live tenant's key",
                sidecar_path.display()
            )
            .into());
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = sidecar_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&sidecar_path, escrow_bytes)?;
            set_private_file_permissions(&sidecar_path)?;
        }
        Err(err) => return Err(err.into()),
    }
    // Restore writes the RAW leg: chunk bytes are ciphertext under their
    // own content addresses, and restore_bundle verifies target.put
    // round-trips each address — only the raw domain satisfies that.
    let target = raw_local_leg_writable(&command.data_dir, &tenant)?;
    let report = ObjectBackup::restore_bundle(target.as_ref(), &bundle, Some(&key_escrow)).await?;
    drop(target);
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
    if report.beyond_repair.is_empty() && report.exhausted {
        // Deferred repairs are NOT a clean outcome: the budget stopped the
        // run before every degraded blob was handled — automation must
        // re-run (or raise --max-bytes), so the exit code says so.
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        std::process::exit(4);
    }
    if !report.beyond_repair.is_empty() {
        std::io::Write::flush(&mut std::io::stdout())?;
        std::process::exit(3);
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

/// The master-key file this deployment resolves: the configured override
/// (`NIMBUS_OBJECT_STORAGE_MASTER_KEY_FILE`) when set, matching
/// `ObjectStorageResolver::object_master_key_path`, else the data-dir
/// default.
fn master_key_path_from_env(data_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    Ok(ObjectStorageConfig::from_env(None)?
        .master_key_file()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| nimbus::object_master_key_path(data_dir)))
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
    // Ownership is HELD through the deletion (not probe-and-drop): a
    // server starting between a released probe and remove_dir_all would
    // race the unlink. Unlinking directories whose flocks our own process
    // holds is safe on Unix; the guard drops after the trees are gone.
    // Unconditional in erasure mode: locking only EXISTING trees leaves a
    // window where a running server creates and writes the tenant's roots
    // after the check — opening writable creates AND locks every drive
    // path before any destructive step.
    let _erasure_ownership = if matches!(&local_leg, LocalLeg::Erasure(_)) {
        Some(
            ErasureBlobStore::open(erasure_config_from_env(&tenant)?).map_err(
                |err| -> Box<dyn Error> {
                    format!(
                        "tenant rm requires exclusive ownership of the erasure drives \
                         (stop the server first — this is offline maintenance): {err}"
                    )
                    .into()
                },
            )?,
        )
    } else {
        None
    };
    // Destruction holds every ownership guard THROUGH the deletion:
    //
    // - Both legs lock UNCONDITIONALLY (opening creates absent roots and
    //   locks them, closing the create-after-check window a running server
    //   could exploit).
    // - The control-plane tenant is deleted first (TenantNotFound is
    //   tolerated: re-running after a partial prior failure is the
    //   recovery path) — once it is gone, no request can route to these
    //   roots, so nothing new can be written to them through the engine.
    // - remove_dir_all runs WHILE the flocks are held (unlinking our own
    //   held lock files is safe on Unix; the flock stays valid on the
    //   unlinked inode until the guard drops). A pathological external
    //   recreation after deletion yields an empty, unroutable directory —
    //   benign residue, no data.
    // Every failure mode is resumable by a plain re-run.
    let _pack_ownership = if matches!(&local_leg, LocalLeg::Pack) {
        Some(
            LocalPackStore::open_with_options(
                &root,
                nimbus::LocalPackStoreOptions {
                    identity: Some(nimbus::tenant_root_identity(&tenant)),
                    ..nimbus::LocalPackStoreOptions::default()
                },
            )
            .map_err(|err| -> Box<dyn Error> {
                format!(
                    "tenant rm requires exclusive ownership of the tenant blob root \
                     (stop the server first — this is offline maintenance): {err}"
                )
                .into()
            })?,
        )
    } else {
        None
    };

    let engine = open_engine(&command.data_dir, command.provider).await?;
    match engine.delete_tenant_async(tenant.clone()).await {
        Ok(()) => {}
        // Idempotent re-run after a partial prior failure.
        Err(NimbusError::TenantNotFound(_)) => {}
        Err(err) => return Err(err.into()),
    }

    // Contents first (locks + markers retained), roots last (guards
    // dropped): deleting an flock'd file is refused on some filesystems
    // (Windows), and unlinking held locks violates the ownership protocol
    // — so everything EXCEPT `lock` and `format.nblfmt` is deleted while
    // the guards are authoritative, then the guards drop and the
    // near-empty roots are removed. The post-drop window is benign: the
    // control-plane tenant is already gone, so a recreated root is empty,
    // unroutable residue.
    let mut erasure_trees_removed = 0usize;
    for (index, path) in std::iter::once(&root)
        .chain(erasure_trees.iter())
        .enumerate()
    {
        if !path.exists() {
            continue;
        }
        remove_root_contents_except_ownership(path)?;
        if index > 0 {
            erasure_trees_removed += 1;
        }
    }
    drop(_erasure_ownership);
    drop(_pack_ownership);
    for path in std::iter::once(&root).chain(erasure_trees.iter()) {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
    }
    engine.quiesce().await;
    emit_object_storage_info(format!(
        "tenant rm tenant={} object_blobs_removed=true erasure_trees_removed={}",
        tenant, erasure_trees_removed
    ));
    Ok(())
}

/// Deletes everything under a byte-plane root EXCEPT the ownership files
/// (`lock`, `format.nblfmt`), which stay until their guards drop — some
/// filesystems refuse deleting open/locked files, and the ownership
/// protocol keeps the root identity authoritative through the deletion.
/// The tenant's RAW byte-plane leg, read-only (backup export source):
/// content addresses are ciphertext hashes and export verification only
/// holds in the raw domain.
fn raw_local_leg_read_only(
    data_dir: &Path,
    tenant: &TenantId,
) -> Result<Box<dyn nimbus::BlobStore>, Box<dyn Error>> {
    match local_leg_from_env()? {
        LocalLeg::Pack => Ok(Box::new(LocalPackStore::open_read_only_with_identity(
            object_blob_root(data_dir, tenant),
            Some(nimbus::tenant_root_identity(tenant)),
        )?)),
        LocalLeg::Erasure(_) => Ok(Box::new(ErasureBlobStore::open_read_only(
            erasure_config_from_env(tenant)?,
        )?)),
    }
}

/// The tenant's RAW byte-plane leg, writable (restore target): offline
/// maintenance — the flocks fail closed with Busy while a server runs.
fn raw_local_leg_writable(
    data_dir: &Path,
    tenant: &TenantId,
) -> Result<Box<dyn nimbus::BlobStore>, Box<dyn Error>> {
    match local_leg_from_env()? {
        LocalLeg::Pack => Ok(Box::new(LocalPackStore::open_with_options(
            object_blob_root(data_dir, tenant),
            nimbus::LocalPackStoreOptions {
                identity: Some(nimbus::tenant_root_identity(tenant)),
                ..nimbus::LocalPackStoreOptions::default()
            },
        )?)),
        LocalLeg::Erasure(_) => Ok(Box::new(ErasureBlobStore::open(erasure_config_from_env(
            tenant,
        )?)?)),
    }
}

fn remove_root_contents_except_ownership(root: &Path) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == std::ffi::OsStr::new(nimbus::LOCK_FILE)
            || name == std::ffi::OsStr::new(nimbus::FORMAT_FILE)
        {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
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

fn read_key_escrow(id: &str, path: &Path) -> Result<KeyEscrow, Box<dyn Error>> {
    Ok(KeyEscrow::new(id, std::fs::read(path)?.into())?)
}

async fn backup_roots_from_archive_or_local(
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
                return Ok(erasure.visible_blob_hashes().await?);
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
    use nimbus::{Bytes, ObjectStorageResolver};

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

    #[tokio::test]
    async fn backup_restore_round_trips_ciphertext_for_encrypted_tenant() {
        // Regression for the encryption-domain defect: the verb pair used
        // the resolver's ENCRYPTED store, whose plaintext reads can never
        // verify against ciphertext content addresses — every non-empty
        // backup of an encrypted tenant failed, and restore minted a fresh
        // DEK that could never read the bundle's ciphertext. The verbs now
        // run raw-leg ciphertext end-to-end and escrow installs the
        // wrapped-DEK sidecar at restore.
        let source_dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(source_dir.path()).unwrap());
        let tenant = TenantId::new("backup-tenant").unwrap();
        engine.create_tenant(tenant.clone()).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let store = resolver.blob_store(&tenant).unwrap();
        let plaintext = Bytes::from_static(b"SECRET-PLAINTEXT-MARKER payload");
        let address = store.put(plaintext.clone()).await.unwrap();
        drop(store);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);

        // Escrow = the tenant's wrapped-DEK sidecar bytes.
        let sidecar = std::fs::read(nimbus::KeyManifest::manifest_path(
            &nimbus::object_blob_key_path(source_dir.path(), &tenant),
        ))
        .unwrap();
        let escrow_file = source_dir.path().join("escrow.bin");
        std::fs::write(&escrow_file, &sidecar).unwrap();

        let bundle_path = source_dir.path().join("backup.nobb");
        run_backup_object_store(BackupObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: source_dir.path().to_path_buf(),
            out: bundle_path.clone(),
            key_escrow_id: "backup-tenant".to_string(),
            key_escrow_file: escrow_file.clone(),
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect("backup of an encrypted tenant with data must succeed");

        // The bundle holds ciphertext: the plaintext marker must not appear.
        let bundle_bytes = std::fs::read(&bundle_path).unwrap();
        assert!(
            !bundle_bytes
                .windows(b"SECRET-PLAINTEXT-MARKER".len())
                .any(|window| window == b"SECRET-PLAINTEXT-MARKER"),
            "backup artifact must never contain plaintext"
        );

        // Disaster recovery in place: the byte plane (ciphertext + wrapped-DEK
        // sidecar) is lost; the deployment's master key and layout survive.
        // The key-manifest provider identity records the master-key path, so
        // restore requires the same data-dir layout as the source.
        std::fs::remove_dir_all(source_dir.path().join("object-blobs")).unwrap();

        run_restore_object_store(RestoreObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: source_dir.path().to_path_buf(),
            input: bundle_path,
            key_escrow_id: "backup-tenant".to_string(),
            key_escrow_file: escrow_file,
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect("restore must succeed with matching escrow");

        // The restored tenant READS its plaintext through the normal
        // encrypted composition — escrow made the ciphertext readable.
        let engine = Arc::new(Engine::new(source_dir.path()).unwrap());
        let resolver = ObjectStorageResolver::new(engine.clone());
        let restored = resolver.blob_store(&tenant).unwrap();
        assert_eq!(restored.get(&address).await.unwrap(), plaintext);
        drop(restored);
        engine.quiesce().await;
    }

    #[tokio::test]
    async fn backup_rejects_escrow_that_does_not_match_the_sidecar() {
        // Fail EARLY: escrowing the wrong material must error at backup
        // time, not surface as an unreadable restore later.
        let dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(dir.path()).unwrap());
        let tenant = TenantId::new("escrow-tenant").unwrap();
        engine.create_tenant(tenant.clone()).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let store = resolver.blob_store(&tenant).unwrap();
        store
            .put(Bytes::from_static(b"some payload"))
            .await
            .unwrap();
        drop(store);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);

        let escrow_file = dir.path().join("wrong-escrow.bin");
        std::fs::write(&escrow_file, b"not the sidecar bytes").unwrap();
        let err = run_backup_object_store(BackupObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: dir.path().to_path_buf(),
            out: dir.path().join("backup.nobb"),
            key_escrow_id: "escrow-tenant".to_string(),
            key_escrow_file: escrow_file,
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect_err("mismatched escrow must fail the backup");
        assert!(err.to_string().contains("does not match"), "{err}");
    }

    #[tokio::test]
    async fn restore_rejects_a_deployment_with_a_different_master_key_layout() {
        // The escrowed manifest is AAD-bound to the source deployment's
        // master-key path; restoring into a different layout must fail
        // EARLY with an actionable message, not install a sidecar the
        // resolver will refuse at first open.
        let source_dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(source_dir.path()).unwrap());
        let tenant = TenantId::new("layout-tenant").unwrap();
        engine.create_tenant(tenant.clone()).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let store = resolver.blob_store(&tenant).unwrap();
        store
            .put(Bytes::from_static(b"layout payload"))
            .await
            .unwrap();
        drop(store);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);

        let sidecar_path = nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(
            source_dir.path(),
            &tenant,
        ));
        let escrow_file = source_dir.path().join("escrow.bin");
        std::fs::copy(&sidecar_path, &escrow_file).unwrap();
        let bundle_path = source_dir.path().join("backup.nobb");
        run_backup_object_store(BackupObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: source_dir.path().to_path_buf(),
            out: bundle_path.clone(),
            key_escrow_id: "layout-tenant".to_string(),
            key_escrow_file: escrow_file.clone(),
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .unwrap();

        // Different data dir = different master-key path, even with the
        // same master key bytes installed.
        let target_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(target_dir.path().join("keys")).unwrap();
        std::fs::copy(
            nimbus::object_master_key_path(source_dir.path()),
            nimbus::object_master_key_path(target_dir.path()),
        )
        .unwrap();
        let err = run_restore_object_store(RestoreObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: target_dir.path().to_path_buf(),
            input: bundle_path,
            key_escrow_id: "layout-tenant".to_string(),
            key_escrow_file: escrow_file,
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect_err("foreign layout must fail early");
        assert!(err.to_string().contains("wrapped by"), "{err}");
        // Fail-closed means NOTHING was installed in the target.
        assert!(
            !nimbus::object_blob_root(target_dir.path(), &tenant).exists(),
            "restore must not leave partial state behind on validation failure"
        );
    }

    #[tokio::test]
    async fn restore_rejects_a_valid_escrow_that_is_not_the_bundles() {
        // A DIFFERENT tenant's sidecar is a perfectly valid manifest for
        // this deployment — restore must still refuse it BEFORE installing
        // anything, or the poisoned sidecar would block the correct retry.
        let dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(dir.path()).unwrap());
        let tenant_a = TenantId::new("bundle-tenant").unwrap();
        let tenant_b = TenantId::new("other-tenant").unwrap();
        engine.create_tenant(tenant_a.clone()).unwrap();
        engine.create_tenant(tenant_b.clone()).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let store_a = resolver.blob_store(&tenant_a).unwrap();
        store_a.put(Bytes::from_static(b"payload a")).await.unwrap();
        let _store_b = resolver.blob_store(&tenant_b).unwrap();
        drop(store_a);
        drop(_store_b);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);

        let sidecar_a = nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(
            dir.path(),
            &tenant_a,
        ));
        let escrow_a = dir.path().join("escrow-a.bin");
        std::fs::copy(&sidecar_a, &escrow_a).unwrap();
        let bundle_path = dir.path().join("backup.nobb");
        run_backup_object_store(BackupObjectStoreCommand {
            tenant: tenant_a.as_str().to_string(),
            data_dir: dir.path().to_path_buf(),
            out: bundle_path.clone(),
            key_escrow_id: "bundle-tenant".to_string(),
            key_escrow_file: escrow_a,
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .unwrap();

        // Simulate loss, then present tenant B's (valid) sidecar as escrow.
        std::fs::remove_dir_all(dir.path().join("object-blobs")).unwrap();
        let sidecar_b_escrow = dir.path().join("escrow-b.bin");
        // tenant B's sidecar was wiped with the byte plane; recreate a
        // valid foreign manifest by re-opening tenant B's store.
        let engine = Arc::new(Engine::new(dir.path()).unwrap());
        let resolver = ObjectStorageResolver::new(engine.clone());
        let _store_b = resolver.blob_store(&tenant_b).unwrap();
        drop(_store_b);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);
        std::fs::copy(
            nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(
                dir.path(),
                &tenant_b,
            )),
            &sidecar_b_escrow,
        )
        .unwrap();
        // Remove tenant A's freshly-minted state so install-nothing is
        // observable.
        std::fs::remove_dir_all(nimbus::object_blob_root(dir.path(), &tenant_a)).ok();

        let err = run_restore_object_store(RestoreObjectStoreCommand {
            tenant: tenant_a.as_str().to_string(),
            data_dir: dir.path().to_path_buf(),
            input: bundle_path,
            key_escrow_id: "bundle-tenant".to_string(),
            key_escrow_file: sidecar_b_escrow,
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect_err("an escrow that is not the bundle's must be refused");
        assert!(
            err.to_string().contains("recorded in the backup bundle"),
            "{err}"
        );
        assert!(
            !nimbus::object_blob_root(dir.path(), &tenant_a).exists(),
            "refusal must install nothing for the target tenant"
        );
    }
}
