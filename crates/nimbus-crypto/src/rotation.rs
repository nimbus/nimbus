//! Crash-recoverable publication for redb DEK rotation artifacts.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use nimbus_core::{Error, Result};

use super::KeyManifest;

const MARKER_BYTES: &[u8] = b"nimbus-redb-dek-rotation-v1\n";

pub fn dek_rotation_data_stage_path(protected_path: &Path) -> PathBuf {
    append_suffix(protected_path, ".rotating")
}

pub fn dek_rotation_manifest_stage_path(protected_path: &Path) -> PathBuf {
    append_suffix(&KeyManifest::manifest_path(protected_path), ".rotating")
}

pub fn recover_interrupted_dek_rotation(protected_path: &Path) -> Result<bool> {
    let paths = RedbDekRotationPaths::new(protected_path);
    if !try_exists(&paths.marker_path)? {
        return Ok(false);
    }
    validate_marker(&paths.marker_path)?;
    finish_committed_rotation(&paths)?;
    Ok(true)
}

pub fn commit_staged_dek_rotation(protected_path: &Path) -> Result<()> {
    let paths = RedbDekRotationPaths::new(protected_path);
    require_staged_artifact(&paths.database_stage_path, "redb rotation database")?;
    require_staged_artifact(&paths.manifest_stage_path, "redb rotation manifest")?;
    write_marker(&paths)?;
    finish_committed_rotation(&paths)
}

struct RedbDekRotationPaths {
    protected_path: PathBuf,
    manifest_path: PathBuf,
    database_stage_path: PathBuf,
    manifest_stage_path: PathBuf,
    marker_path: PathBuf,
}

impl RedbDekRotationPaths {
    fn new(protected_path: &Path) -> Self {
        Self {
            protected_path: protected_path.to_path_buf(),
            manifest_path: KeyManifest::manifest_path(protected_path),
            database_stage_path: dek_rotation_data_stage_path(protected_path),
            manifest_stage_path: dek_rotation_manifest_stage_path(protected_path),
            marker_path: append_suffix(protected_path, ".dek-rotation"),
        }
    }
}

fn write_marker(paths: &RedbDekRotationPaths) -> Result<()> {
    if let Some(parent) = paths.marker_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create redb DEK rotation marker parent {}: {error}",
                parent.display()
            ))
        })?;
    }
    {
        let mut marker = File::create(&paths.marker_path).map_err(|error| {
            Error::Internal(format!(
                "failed to create redb DEK rotation marker {}: {error}",
                paths.marker_path.display()
            ))
        })?;
        marker.write_all(MARKER_BYTES).map_err(|error| {
            Error::Internal(format!(
                "failed to write redb DEK rotation marker {}: {error}",
                paths.marker_path.display()
            ))
        })?;
        marker.sync_all().map_err(|error| {
            Error::Internal(format!(
                "failed to sync redb DEK rotation marker {}: {error}",
                paths.marker_path.display()
            ))
        })?;
    }
    sync_parent_dir(&paths.marker_path)
}

fn validate_marker(marker_path: &Path) -> Result<()> {
    let bytes = fs::read(marker_path).map_err(|error| {
        Error::Internal(format!(
            "failed to read redb DEK rotation marker {}: {error}",
            marker_path.display()
        ))
    })?;
    if bytes == MARKER_BYTES {
        Ok(())
    } else {
        Err(Error::Internal(format!(
            "redb DEK rotation marker {} is not a Nimbus rotation marker",
            marker_path.display()
        )))
    }
}

fn finish_committed_rotation(paths: &RedbDekRotationPaths) -> Result<()> {
    if try_exists(&paths.database_stage_path)? {
        replace_file(&paths.database_stage_path, &paths.protected_path).map_err(|error| {
            Error::Internal(format!(
                "failed to publish staged redb database {} over {} during DEK rotation: {error}",
                paths.database_stage_path.display(),
                paths.protected_path.display()
            ))
        })?;
    }
    if try_exists(&paths.manifest_stage_path)? {
        replace_file(&paths.manifest_stage_path, &paths.manifest_path).map_err(|error| {
            Error::Internal(format!(
                "failed to publish staged redb manifest {} over {} during DEK rotation: {error}",
                paths.manifest_stage_path.display(),
                paths.manifest_path.display()
            ))
        })?;
    }
    sync_parent_dir(&paths.protected_path)?;
    remove_marker(&paths.marker_path)?;
    sync_parent_dir(&paths.marker_path)
}

fn remove_marker(marker_path: &Path) -> Result<()> {
    match fs::remove_file(marker_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Internal(format!(
            "failed to remove redb DEK rotation marker {}: {error}",
            marker_path.display()
        ))),
    }
}

fn require_staged_artifact(path: &Path, label: &str) -> Result<()> {
    if try_exists(path)? {
        Ok(())
    } else {
        Err(Error::Internal(format!(
            "missing staged {label} for redb DEK rotation: {}",
            path.display()
        )))
    }
}

fn try_exists(path: &Path) -> Result<bool> {
    path.try_exists().map_err(|error| {
        Error::Internal(format!(
            "failed to inspect redb DEK rotation path {}: {error}",
            path.display()
        ))
    })
}

fn sync_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        let dir = File::open(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to open redb DEK rotation parent directory {}: {error}",
                parent.display()
            ))
        })?;
        dir.sync_all().map_err(|error| {
            Error::Internal(format!(
                "failed to sync redb DEK rotation parent directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let mut source_wide: Vec<u16> = source.as_os_str().encode_wide().collect();
    source_wide.push(0);
    let mut destination_wide: Vec<u16> = destination.as_os_str().encode_wide().collect();
    destination_wide.push(0);
    let ok = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LocalKeySubject, ManifestCipher, MasterKeyFileProvider, generate_key_manifest,
        resolve_subject_encryption_key,
    };
    use nimbus_core::TenantId;
    use tempfile::tempdir;

    #[test]
    fn commit_staged_dek_rotation_replaces_database_and_manifest_pair() {
        let dir = tempdir().expect("tempdir should create");
        let protected_path = dir.path().join("tenant.redb");
        let manifest_path = KeyManifest::manifest_path(&protected_path);
        fs::write(&protected_path, b"old-db").expect("old db should write");
        fs::write(&manifest_path, b"old-manifest").expect("old manifest should write");
        fs::write(dek_rotation_data_stage_path(&protected_path), b"new-db")
            .expect("staged db should write");
        fs::write(
            dek_rotation_manifest_stage_path(&protected_path),
            b"new-manifest",
        )
        .expect("staged manifest should write");

        commit_staged_dek_rotation(&protected_path).expect("commit should complete");

        assert_eq!(
            fs::read(&protected_path).expect("db should read"),
            b"new-db"
        );
        assert_eq!(
            fs::read(&manifest_path).expect("manifest should read"),
            b"new-manifest"
        );
        assert!(
            !append_suffix(&protected_path, ".dek-rotation").exists(),
            "marker should be removed after successful commit"
        );
    }

    #[test]
    fn recover_dek_rotation_finishes_before_any_artifact_is_published() {
        let dir = tempdir().expect("tempdir should create");
        let protected_path = dir.path().join("tenant.redb");
        let manifest_path = KeyManifest::manifest_path(&protected_path);
        fs::write(&protected_path, b"old-db").expect("old db should write");
        fs::write(&manifest_path, b"old-manifest").expect("old manifest should write");
        fs::write(dek_rotation_data_stage_path(&protected_path), b"new-db")
            .expect("staged db should write");
        fs::write(
            dek_rotation_manifest_stage_path(&protected_path),
            b"new-manifest",
        )
        .expect("staged manifest should write");
        write_marker(&RedbDekRotationPaths::new(&protected_path)).expect("marker should write");

        assert!(
            recover_interrupted_dek_rotation(&protected_path).expect("recovery should complete"),
            "marker should trigger recovery"
        );

        assert_eq!(
            fs::read(&protected_path).expect("db should read"),
            b"new-db"
        );
        assert_eq!(
            fs::read(&manifest_path).expect("manifest should read"),
            b"new-manifest"
        );
        assert!(
            !dek_rotation_data_stage_path(&protected_path).exists(),
            "staged db should be consumed"
        );
        assert!(
            !dek_rotation_manifest_stage_path(&protected_path).exists(),
            "staged manifest should be consumed"
        );
    }

    #[test]
    fn recover_dek_rotation_finishes_after_data_publish() {
        let dir = tempdir().expect("tempdir should create");
        let protected_path = dir.path().join("tenant.redb");
        let manifest_path = KeyManifest::manifest_path(&protected_path);
        fs::write(&protected_path, b"new-db").expect("published db should write");
        fs::write(&manifest_path, b"old-manifest").expect("old manifest should write");
        fs::write(
            dek_rotation_manifest_stage_path(&protected_path),
            b"new-manifest",
        )
        .expect("staged manifest should write");
        write_marker(&RedbDekRotationPaths::new(&protected_path)).expect("marker should write");

        recover_interrupted_dek_rotation(&protected_path).expect("recovery should complete");

        assert_eq!(
            fs::read(&protected_path).expect("db should read"),
            b"new-db"
        );
        assert_eq!(
            fs::read(&manifest_path).expect("manifest should read"),
            b"new-manifest"
        );
    }

    #[test]
    fn recover_dek_rotation_removes_marker_after_both_artifacts_are_published() {
        let dir = tempdir().expect("tempdir should create");
        let protected_path = dir.path().join("tenant.redb");
        let manifest_path = KeyManifest::manifest_path(&protected_path);
        fs::write(&protected_path, b"new-db").expect("published db should write");
        fs::write(&manifest_path, b"new-manifest").expect("published manifest should write");
        let marker_path = append_suffix(&protected_path, ".dek-rotation");
        write_marker(&RedbDekRotationPaths::new(&protected_path)).expect("marker should write");

        recover_interrupted_dek_rotation(&protected_path).expect("recovery should complete");

        assert_eq!(
            fs::read(&protected_path).expect("db should read"),
            b"new-db"
        );
        assert_eq!(
            fs::read(&manifest_path).expect("manifest should read"),
            b"new-manifest"
        );
        assert!(
            !marker_path.exists(),
            "fully published rotation marker should be removed"
        );
    }

    #[test]
    fn resolve_redb_key_recovers_committed_rotation_before_reading_manifest() {
        let dir = tempdir().expect("tempdir should create");
        let protected_path = dir.path().join("tenant.redb");
        fs::write(&protected_path, b"old-db").expect("old db should write");

        let key_path = dir.path().join("master.key");
        fs::write(&key_path, [0x42u8; 32]).expect("master key should write");
        let provider = MasterKeyFileProvider::new(key_path).expect("provider should create");
        let tenant_id = TenantId::new("demo".to_string()).expect("tenant id should build");
        let subject = LocalKeySubject::redb_tenant(tenant_id, "tenant.redb");

        let (old_manifest, _) =
            generate_key_manifest(&provider, &subject, ManifestCipher::RedbAes256GcmSiv)
                .expect("old manifest should generate");
        old_manifest
            .write_for(&protected_path)
            .expect("old manifest should write");
        let (new_manifest, new_key) =
            generate_key_manifest(&provider, &subject, ManifestCipher::RedbAes256GcmSiv)
                .expect("new manifest should generate");
        new_manifest
            .write(&dek_rotation_manifest_stage_path(&protected_path))
            .expect("new manifest stage should write");
        fs::write(dek_rotation_data_stage_path(&protected_path), b"new-db")
            .expect("new db stage should write");
        write_marker(&RedbDekRotationPaths::new(&protected_path)).expect("marker should write");

        let resolved = resolve_subject_encryption_key(
            &protected_path,
            &provider,
            &subject,
            ManifestCipher::RedbAes256GcmSiv,
        )
        .expect("key resolution should recover and unwrap new manifest");

        assert_eq!(resolved.as_bytes(), new_key.plaintext());
        assert_eq!(
            fs::read(&protected_path).expect("db should read"),
            b"new-db"
        );
        assert!(
            !append_suffix(&protected_path, ".dek-rotation").exists(),
            "marker should be removed by key-resolution recovery"
        );
    }
}
