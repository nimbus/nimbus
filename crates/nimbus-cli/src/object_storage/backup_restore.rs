//! Backup/restore verbs for the object-storage byte plane.
//!
//! Both verbs run in the CIPHERTEXT domain end-to-end: content addresses
//! are ciphertext hashes, so export/restore verification only holds on
//! the raw leg, no plaintext ever reaches a backup artifact, and the
//! crypto-shred posture survives DR. Key custody travels as the tenant's
//! wrapped-DEK sidecar (the blob-key `KeyManifest`), escrowed at backup
//! and reinstalled — after full validation — at restore.

use std::error::Error;
use std::path::{Path, PathBuf};

use nimbus::{
    BackupBundle, BackupRequest, ErasureBlobStore, Error as NimbusError, KeyEscrow, LocalLeg,
    LocalPackStore, ObjectBackup, ObjectStorageConfig, PointInTimeRestoreArchive, TenantId,
    object_backup_roots, object_blob_root,
};
use nimbus_core::StorageErrorKind;

use super::{
    BackupObjectStoreCommand, RestoreObjectStoreCommand, emit_object_storage_info,
    erasure_config_from_env, local_leg_from_env, open_engine,
};

pub(super) async fn run_backup_object_store(
    command: BackupObjectStoreCommand,
) -> Result<(), Box<dyn Error>> {
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
    //
    // The open is WRITABLE for exclusive ownership, not for writing:
    // export_bundle requires its roots to stay live for the whole export,
    // and only exclusive ownership guarantees no concurrent release/GC/
    // compaction reclaims one mid-export. Offline maintenance — a running
    // server holds the flocks and this fails closed with Busy.
    let source = raw_local_leg_writable(&command.data_dir, &tenant)?;
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
    // Cryptographically validate the escrow UNCONDITIONALLY (even a
    // zero-blob tenant must not emit a bundle whose escrow restore will
    // reject), then prove the DEK actually decrypts the archived
    // ciphertext when any exists: a corrupted or regenerated sidecar (a
    // valid same-tenant manifest whose DEK is unrelated to the stored
    // data) must fail the BACKUP, not surface later as an unrestorable
    // bundle. One probe suffices — every blob is sealed under the single
    // tenant DEK, and export's content-hash verification covers
    // per-chunk integrity. The probe plaintext is discarded; nothing but
    // ciphertext enters the bundle.
    let data_key = unwrap_escrowed_data_key(
        &command.data_dir,
        &tenant,
        key_escrow.wrapped_key_material(),
        &command.key_escrow_file,
    )?;
    if let Some(probe_root) = roots.first() {
        let framed = source.get(probe_root).await?;
        nimbus_crypto::open_framed_blob(&nimbus_crypto::FramedBlobKey::new(data_key), &framed)
            .map_err(|err| -> Box<dyn Error> {
                format!(
                    "escrowed key material does not decrypt the tenant's stored ciphertext (blob {probe_root}) — the sidecar was likely regenerated after this data was written; this deployment's objects are not recoverable with this key: {err}"
                )
                .into()
            })?;
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

pub(super) async fn run_restore_object_store(
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
    drop(unwrap_escrowed_data_key(
        &command.data_dir,
        &tenant,
        escrow_bytes,
        &command.key_escrow_file,
    )?);
    let archive = serde_json::from_slice(bundle.manifest_snapshot())?;
    // Everything validated — now mutate, under exclusive byte-plane
    // ownership FIRST: the raw-leg flocks exclude a live server (and its
    // resolver, which acquires the leg before it touches key material),
    // so no reader can hold a DEK this install would contradict. Held
    // through metadata import + quiesce so no other process can acquire
    // the byte plane while bytes and metadata are at different restore
    // points. Restore writes the RAW leg: chunk bytes are ciphertext
    // under their own content addresses, and restore_bundle verifies
    // target.put round-trips each address — only the raw domain
    // satisfies that.
    let target = raw_local_leg_writable(&command.data_dir, &tenant)?;
    install_escrow_sidecar(&sidecar_path, escrow_bytes)?;
    let report = ObjectBackup::restore_bundle(target.as_ref(), &bundle, Some(&key_escrow)).await?;
    let engine = open_engine(&command.data_dir, command.provider).await?;
    match engine.create_tenant(tenant.clone()) {
        // tenant-lifecycle: embedded-only
        Ok(()) | Err(NimbusError::AlreadyExists(_)) => {}
        Err(error) => return Err(error.into()),
    }
    engine.import_point_in_time_restore_archive(&tenant, &archive)?;
    engine.quiesce().await;
    drop(target);
    emit_object_storage_info(format!(
        "restore-object-store tenant={} chunks={} bytes={} input={}",
        tenant,
        report.restored_chunks,
        report.restored_bytes,
        command.input.display()
    ));
    Ok(())
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

/// Unwraps escrowed blob-key material under THIS deployment's master
/// key, proving — in one step — that the escrow parses as a key
/// manifest and that its subject, cipher, provider identity (data-dir/
/// master-key layout), and wrapping key all match this deployment and
/// tenant. Returns the tenant DEK; callers either prove it against
/// stored ciphertext (backup) or discard it after validation (restore).
fn unwrap_escrowed_data_key(
    data_dir: &Path,
    tenant: &TenantId,
    escrow_bytes: &[u8],
    escrow_origin: &Path,
) -> Result<nimbus_crypto::DataEncryptionKey, Box<dyn Error>> {
    let sidecar_path =
        nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(data_dir, tenant));
    let escrow_manifest = nimbus_crypto::KeyManifest::from_bytes(escrow_bytes, escrow_origin)
        .map_err(|err| -> Box<dyn Error> {
            format!(
                "key escrow file {} is not a valid key manifest — escrow the tenant's blob-key sidecar ({}): {err}",
                escrow_origin.display(),
                sidecar_path.display()
            )
            .into()
        })?;
    let master_key_path = master_key_path_from_env(data_dir)?;
    let provider =
        nimbus_crypto::MasterKeyFileProvider::new(master_key_path.clone()).map_err(|err| {
            format!(
                "open master key file {} (the deployment's master key must be installed): {err}",
                master_key_path.display()
            )
        })?;
    // Subject naming matches the resolver's blob-key sidecar subject.
    let subject = nimbus_crypto::LocalKeySubject::object_blob_store(tenant.clone(), "blob-key");
    let protected_path = nimbus::object_blob_key_path(data_dir, tenant);
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
    })
}

/// Publishes the escrowed wrapped-DEK sidecar fail-closed and durably:
/// the sidecar path must be absent or an existing REGULAR file with
/// byte-identical content (idempotent re-restore) — symlinks are
/// refused outright (`symlink_metadata`, never followed), even ones
/// whose target currently matches, because the tenant's key material
/// must live in an independent file the deployment owns. New material
/// is staged in a private 0600 temp file, synced, published with
/// no-replace semantics (`hard_link`), and the directory entry is
/// fsynced — a torn write can never masquerade as the sidecar and a
/// crash after success cannot lose it (the resolver would otherwise
/// mint a fresh DEK and strand the restored ciphertext). Caller holds
/// exclusive raw-leg ownership, so no live resolver races this install.
fn install_escrow_sidecar(sidecar_path: &Path, escrow_bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    use std::io::Write;
    let parent = sidecar_path
        .parent()
        .ok_or("tenant blob-key sidecar path has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    // Clear any crash-leftover stage entry FIRST: a publish interrupted
    // between hard_link and stage removal legitimately leaves the sidecar
    // with two links, and the link-count guard below must not wedge the
    // retry on our own residue.
    let stage_path = parent.join(".blob-key.restore-stage");
    let _ = std::fs::remove_file(&stage_path);
    // Verifies an existing sidecar on a HELD DESCRIPTOR: open first, then
    // prove the handle IS the path's regular file (lstat type + dev/ino
    // identity on unix), and do the read and chmod through the handle. A
    // racing path swap can only produce a refusal, never a follow. On the
    // match path this also re-enforces permissions and re-syncs the parent
    // directory, so a retry after an interrupted install still upholds the
    // durability guarantee before reporting success.
    let existing_regular_file_matches = |context: &str| -> Result<bool, Box<dyn Error>> {
        use std::io::Read;
        let not_regular = || -> Box<dyn Error> {
            format!(
                "tenant blob-key sidecar path {} exists but is not a regular file (symlink?); refusing to touch it{context}",
                sidecar_path.display()
            )
            .into()
        };
        let path_meta = std::fs::symlink_metadata(sidecar_path)?;
        if !path_meta.is_file() {
            return Err(not_regular());
        }
        let mut file = std::fs::File::open(sidecar_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // The handle must be the exact inode the PRE-open lstat saw:
            // any path swap in between (e.g. to a symlink) yields a
            // different identity and refuses.
            let handle_meta = file.metadata()?;
            if handle_meta.dev() != path_meta.dev() || handle_meta.ino() != path_meta.ino() {
                return Err(not_regular());
            }
            // The key material must be an INDEPENDENT inode the deployment
            // owns: a pre-planted byte-identical hard link with a retained
            // link elsewhere would let that link's owner rewrite the
            // manifest after restore. (A crash-leftover stage link was
            // already removed at function entry.)
            if handle_meta.nlink() > 1 {
                return Err(format!(
                    "tenant blob-key sidecar {} has {} directory links; refusing key material with links outside the deployment{context}",
                    sidecar_path.display(),
                    handle_meta.nlink()
                )
                .into());
            }
        }
        let mut existing = Vec::new();
        file.read_to_end(&mut existing)?;
        if existing.as_slice() != escrow_bytes {
            return Ok(false);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(true)
    };
    match std::fs::symlink_metadata(sidecar_path) {
        Ok(_) => {
            // Off unix the inode-identity and single-link invariants
            // cannot be proven with stable std APIs — fail closed instead
            // of accepting key material that might be externally linked.
            #[cfg(not(unix))]
            return Err(format!(
                "tenant blob-key sidecar {} already exists and this platform cannot prove it is an independent single-linked file; remove it manually if this re-restore is intentional",
                sidecar_path.display()
            )
            .into());
            #[cfg(unix)]
            {
                if existing_regular_file_matches("")? {
                    return Ok(());
                }
                return Err(format!(
                    "tenant blob-key sidecar {} already exists with DIFFERENT key material; refusing to overwrite a live tenant's key",
                    sidecar_path.display()
                )
                .into());
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    // Stage privately (0600 at open), sync, then publish no-replace.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let staged = options
        .open(&stage_path)
        .map_err(Box::<dyn Error>::from)
        .and_then(|mut file| {
            file.write_all(escrow_bytes)?;
            file.sync_all()?;
            Ok(())
        });
    if let Err(err) = staged {
        let _ = std::fs::remove_file(&stage_path);
        return Err(err);
    }
    // hard_link never replaces and never follows a symlink at the
    // destination; racing installs (only another offline restore — the
    // leg lock excludes everything else) fall to the compare path.
    let publish = std::fs::hard_link(&stage_path, sidecar_path);
    let _ = std::fs::remove_file(&stage_path);
    match publish {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            if !existing_regular_file_matches(" (appeared mid-install)")? {
                return Err(format!(
                    "tenant blob-key sidecar {} already exists with DIFFERENT key material; refusing to overwrite a live tenant's key",
                    sidecar_path.display()
                )
                .into());
            }
        }
        Err(err) => return Err(err.into()),
    }
    // Verify the PUBLISHED entry through the same handle-based check
    // (type, inode identity, single link, exact bytes) — a swapped stage
    // pathname or foreign inode fails closed here — and let its match
    // path enforce 0600 on the held descriptor and fsync the parent
    // directory entry durable. Beyond this point an actor who can still
    // mutate the tenant directory owns the deployment's data dir outright
    // (packs, index, master key included); that boundary cannot be
    // defended from inside it and later swaps are out of scope.
    if !existing_regular_file_matches(" (verifying the published sidecar)")? {
        return Err(format!(
            "tenant blob-key sidecar {} changed during publication; refusing",
            sidecar_path.display()
        )
        .into());
    }
    #[cfg(not(unix))]
    {
        // The unix verification path fsyncs the parent inside the match
        // arm; elsewhere directory fsync is unavailable.
        super::set_private_file_permissions(sidecar_path)?;
    }
    Ok(())
}

/// The tenant's RAW byte-plane leg with exclusive ownership (backup
/// source / restore target): offline maintenance — the flocks fail
/// closed with Busy while a server runs.
fn raw_local_leg_writable(
    data_dir: &Path,
    tenant: &TenantId,
) -> Result<Box<dyn nimbus::BlobStore>, Box<dyn Error>> {
    let busy_hint = |error: NimbusError| -> Box<dyn Error> {
        if error.storage_kind() == Some(StorageErrorKind::Busy) {
            format!("{error}; stop the server first; backup and restore are offline maintenance")
                .into()
        } else {
            error.into()
        }
    };
    match local_leg_from_env()? {
        LocalLeg::Pack => Ok(Box::new(
            LocalPackStore::open_with_options(
                object_blob_root(data_dir, tenant),
                nimbus::LocalPackStoreOptions {
                    identity: Some(nimbus::tenant_root_identity(tenant)),
                    ..nimbus::LocalPackStoreOptions::default()
                },
            )
            .map_err(busy_hint)?,
        )),
        LocalLeg::Erasure(_) => Ok(Box::new(
            ErasureBlobStore::open(erasure_config_from_env(tenant)?).map_err(busy_hint)?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus::{Bytes, Engine, ObjectStorageResolver};

    use super::super::ObjectStorageProvider;
    use super::*;

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
    async fn backup_rejects_a_regenerated_sidecar_that_cannot_decrypt_the_data() {
        // A lost-and-regenerated sidecar is a VALID same-tenant manifest
        // whose DEK is unrelated to the stored ciphertext. The escrow
        // byte-compare alone would pass; the export-time decrypt probe
        // must fail the backup instead of emitting an unrestorable bundle.
        let dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(dir.path()).unwrap());
        let tenant = TenantId::new("rekeyed-tenant").unwrap();
        engine.create_tenant(tenant.clone()).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let store = resolver.blob_store(&tenant).unwrap();
        store
            .put(Bytes::from_static(b"sealed under the ORIGINAL key"))
            .await
            .unwrap();
        drop(store);
        drop(resolver);
        engine.quiesce().await;

        // Simulate sidecar loss: the resolver mints a fresh DEK on next
        // open, valid for the tenant but unrelated to the existing data.
        let sidecar_path =
            nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(dir.path(), &tenant));
        std::fs::remove_file(&sidecar_path).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let _store = resolver.blob_store(&tenant).unwrap();
        drop(_store);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);

        let escrow_file = dir.path().join("escrow.bin");
        std::fs::copy(&sidecar_path, &escrow_file).unwrap();
        let err = run_backup_object_store(BackupObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: dir.path().to_path_buf(),
            out: dir.path().join("backup.nobb"),
            key_escrow_id: "rekeyed-tenant".to_string(),
            key_escrow_file: escrow_file,
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect_err("a regenerated sidecar must fail the backup");
        assert!(err.to_string().contains("does not decrypt"), "{err}");
    }

    #[tokio::test]
    async fn backup_of_an_empty_tenant_still_validates_the_escrow() {
        // Zero live blobs means no decrypt probe — but a foreign or
        // corrupted sidecar escrow must STILL fail the backup, or the
        // emitted bundle is guaranteed to be rejected at restore.
        let dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(dir.path()).unwrap());
        let tenant_a = TenantId::new("empty-tenant").unwrap();
        let tenant_b = TenantId::new("donor-tenant").unwrap();
        engine.create_tenant(tenant_a.clone()).unwrap();
        engine.create_tenant(tenant_b.clone()).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let _store_a = resolver.blob_store(&tenant_a).unwrap();
        let _store_b = resolver.blob_store(&tenant_b).unwrap();
        drop(_store_a);
        drop(_store_b);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);

        // Replace tenant A's sidecar with tenant B's manifest and escrow
        // those same bytes: byte-compare passes, crypto validation must
        // not.
        let sidecar_a = nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(
            dir.path(),
            &tenant_a,
        ));
        let sidecar_b = nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(
            dir.path(),
            &tenant_b,
        ));
        std::fs::remove_file(&sidecar_a).unwrap();
        std::fs::copy(&sidecar_b, &sidecar_a).unwrap();
        let escrow_file = dir.path().join("escrow.bin");
        std::fs::copy(&sidecar_a, &escrow_file).unwrap();

        let err = run_backup_object_store(BackupObjectStoreCommand {
            tenant: tenant_a.as_str().to_string(),
            data_dir: dir.path().to_path_buf(),
            out: dir.path().join("backup.nobb"),
            key_escrow_id: "empty-tenant".to_string(),
            key_escrow_file: escrow_file,
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect_err("a foreign sidecar escrow must fail even with zero blobs");
        assert!(err.to_string().contains("not usable"), "{err}");
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

    #[cfg(unix)]
    #[tokio::test]
    async fn restore_refuses_a_symlinked_sidecar_path() {
        // A planted dangling symlink at the sidecar path must not redirect
        // the privileged install to an arbitrary target.
        let dir = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(dir.path()).unwrap());
        let tenant = TenantId::new("symlink-tenant").unwrap();
        engine.create_tenant(tenant.clone()).unwrap();
        let resolver = ObjectStorageResolver::new(engine.clone());
        let store = resolver.blob_store(&tenant).unwrap();
        store
            .put(Bytes::from_static(b"symlink payload"))
            .await
            .unwrap();
        drop(store);
        drop(resolver);
        engine.quiesce().await;
        drop(engine);

        let sidecar_path =
            nimbus::KeyManifest::manifest_path(&nimbus::object_blob_key_path(dir.path(), &tenant));
        let escrow_file = dir.path().join("escrow.bin");
        std::fs::copy(&sidecar_path, &escrow_file).unwrap();
        let bundle_path = dir.path().join("backup.nobb");
        run_backup_object_store(BackupObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: dir.path().to_path_buf(),
            out: bundle_path.clone(),
            key_escrow_id: "symlink-tenant".to_string(),
            key_escrow_file: escrow_file.clone(),
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .unwrap();

        // Wipe the byte plane, then plant a dangling symlink where the
        // sidecar would be installed.
        std::fs::remove_dir_all(dir.path().join("object-blobs")).unwrap();
        let attack_target = dir.path().join("attacker-controlled");
        std::fs::create_dir_all(sidecar_path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&attack_target, &sidecar_path).unwrap();

        let err = run_restore_object_store(RestoreObjectStoreCommand {
            tenant: tenant.as_str().to_string(),
            data_dir: dir.path().to_path_buf(),
            input: bundle_path,
            key_escrow_id: "symlink-tenant".to_string(),
            key_escrow_file: escrow_file.clone(),
            provider: ObjectStorageProvider::Sqlite,
        })
        .await
        .expect_err("a symlinked sidecar path must be refused");
        assert!(err.to_string().contains("not a regular file"), "{err}");
        assert!(
            !attack_target.exists(),
            "the symlink target must never be created"
        );

        // Even a symlink whose target holds the CORRECT bytes is refused:
        // the tenant's key material must be an independent regular file.
        let matching_copy = dir.path().join("matching-copy");
        std::fs::copy(&escrow_file, &matching_copy).unwrap();
        std::fs::remove_file(&sidecar_path).unwrap();
        std::os::unix::fs::symlink(&matching_copy, &sidecar_path).unwrap();
        let err = install_escrow_sidecar(&sidecar_path, &std::fs::read(&matching_copy).unwrap())
            .expect_err("a matching symlinked sidecar must still be refused");
        assert!(err.to_string().contains("not a regular file"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn install_refuses_a_hard_linked_sidecar_and_tolerates_stage_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tenant-root");
        std::fs::create_dir_all(&root).unwrap();
        let sidecar_path = root.join("blob-key.nimbus-enc");
        let escrow = b"escrowed manifest bytes".to_vec();

        // A pre-planted byte-identical file with a RETAINED outside hard
        // link is not independent key material — its other link's owner
        // could rewrite it after restore. Refuse.
        std::fs::write(&sidecar_path, &escrow).unwrap();
        let outside_link = dir.path().join("attacker-retained-link");
        std::fs::hard_link(&sidecar_path, &outside_link).unwrap();
        let err = install_escrow_sidecar(&sidecar_path, &escrow)
            .expect_err("a multiply-linked sidecar must be refused");
        assert!(err.to_string().contains("directory links"), "{err}");

        // A crash-leftover STAGE link (same inode, our own staging name)
        // must not wedge the retry: entry cleanup removes it first and the
        // idempotent path then accepts the single-linked sidecar.
        std::fs::remove_file(&outside_link).unwrap();
        let stage_leftover = root.join(".blob-key.restore-stage");
        std::fs::hard_link(&sidecar_path, &stage_leftover).unwrap();
        install_escrow_sidecar(&sidecar_path, &escrow)
            .expect("crash-leftover stage link must be cleaned and tolerated");
        assert!(
            !stage_leftover.exists(),
            "the stage leftover must be removed"
        );

        // Fresh install still works and publishes a single-linked 0600
        // regular file.
        std::fs::remove_file(&sidecar_path).unwrap();
        install_escrow_sidecar(&sidecar_path, &escrow).expect("fresh install must succeed");
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(&sidecar_path).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.nlink(), 1);
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read(&sidecar_path).unwrap(), escrow);
    }
}
