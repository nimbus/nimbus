use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fs2::FileExt;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::paths::LocalServerPaths;

const LOCAL_ADMIN_TOKEN_VERSION: u32 = 1;
const LOCAL_ADMIN_TOKEN_PREFIX: &str = "neovex_at_";

pub const LOCAL_ADMIN_TOKEN_SCOPE: &str = "local-admin";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAdminTokenRecord {
    pub version: u32,
    pub token: String,
    pub generation: u64,
    pub issued_at: String,
    pub scope: String,
}

impl LocalAdminTokenRecord {
    pub fn authorize(&self, candidate: &str) -> bool {
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.token.as_bytes());
        let expected = hmac::sign(&key, self.token.as_bytes());
        hmac::verify(&key, candidate.as_bytes(), expected.as_ref()).is_ok()
    }
}

pub fn load_or_create_local_admin_token(
    paths: &LocalServerPaths,
) -> io::Result<LocalAdminTokenRecord> {
    paths.ensure_auth_parent_dir()?;
    with_token_file_lock(paths, || {
        match read_local_admin_token_file(&paths.auth_token_path)? {
            Some(record) => Ok(record),
            None => {
                let record = generate_local_admin_token(1)?;
                write_local_admin_token_file(&paths.auth_token_path, &record)?;
                Ok(record)
            }
        }
    })
}

pub fn load_local_admin_token(paths: &LocalServerPaths) -> io::Result<LocalAdminTokenRecord> {
    paths.ensure_auth_parent_dir()?;
    with_token_file_lock(paths, || {
        read_local_admin_token_file(&paths.auth_token_path)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "local admin token file {} does not exist; run `neovex start` once to create it",
                    paths.auth_token_path.display()
                ),
            )
        })
    })
}

pub fn rotate_local_admin_token_offline(
    paths: &LocalServerPaths,
) -> io::Result<LocalAdminTokenRecord> {
    paths.ensure_auth_parent_dir()?;
    with_token_file_lock(paths, || {
        let current = read_local_admin_token_file(&paths.auth_token_path)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "local admin token file {} does not exist; run `neovex start` once to create it",
                    paths.auth_token_path.display()
                ),
            )
        })?;
        let rotated = generate_local_admin_token(current.generation.saturating_add(1))?;
        write_local_admin_token_file(&paths.auth_token_path, &rotated)?;
        Ok(rotated)
    })
}

pub(crate) fn with_token_file_lock<T>(
    paths: &LocalServerPaths,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    paths.ensure_auth_parent_dir()?;
    let lock_path = token_lock_path(&paths.auth_token_path);
    let parent = lock_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "lock path {} does not have a parent directory",
                lock_path.display()
            ),
        )
    })?;
    fs::create_dir_all(parent)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    file.lock_exclusive()?;
    let result = operation();
    let unlock_result = file.unlock();
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

pub(crate) fn read_local_admin_token_file(
    path: &Path,
) -> io::Result<Option<LocalAdminTokenRecord>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let record = serde_json::from_slice::<LocalAdminTokenRecord>(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "local admin token file {} is not valid JSON: {error}",
                path.display()
            ),
        )
    })?;
    validate_local_admin_token_record(&record, path)?;
    Ok(Some(record))
}

pub(crate) fn write_local_admin_token_file(
    path: &Path,
    record: &LocalAdminTokenRecord,
) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "token path {} does not have a parent directory",
                path.display()
            ),
        )
    })?;
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
        io::Error::other(format!(
            "failed to serialize local admin token file {}: {error}",
            path.display()
        ))
    })?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(&bytes)?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;
    set_secure_file_permissions(temp_file.as_file())?;
    temp_file.into_temp_path().persist(path).map_err(|error| {
        io::Error::other(format!(
            "failed to atomically replace {}: {}",
            path.display(),
            error.error
        ))
    })?;
    set_secure_path_permissions(path)?;
    Ok(())
}

pub(crate) fn generate_local_admin_token(generation: u64) -> io::Result<LocalAdminTokenRecord> {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .map_err(|_| io::Error::other("failed to generate local admin token bytes"))?;
    let token = format!(
        "{LOCAL_ADMIN_TOKEN_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes)
    );
    let issued_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| io::Error::other(format!("failed to format token issue time: {error}")))?;
    Ok(LocalAdminTokenRecord {
        version: LOCAL_ADMIN_TOKEN_VERSION,
        token,
        generation,
        issued_at,
        scope: LOCAL_ADMIN_TOKEN_SCOPE.to_string(),
    })
}

fn validate_local_admin_token_record(
    record: &LocalAdminTokenRecord,
    path: &Path,
) -> io::Result<()> {
    if record.version != LOCAL_ADMIN_TOKEN_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "local admin token file {} uses unsupported version {}; expected {}",
                path.display(),
                record.version,
                LOCAL_ADMIN_TOKEN_VERSION
            ),
        ));
    }
    if record.scope != LOCAL_ADMIN_TOKEN_SCOPE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "local admin token file {} uses unsupported scope {:?}; expected {:?}",
                path.display(),
                record.scope,
                LOCAL_ADMIN_TOKEN_SCOPE
            ),
        ));
    }
    if record.generation == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "local admin token file {} must use a generation greater than zero",
                path.display()
            ),
        ));
    }
    let encoded = record
        .token
        .strip_prefix(LOCAL_ADMIN_TOKEN_PREFIX)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "local admin token file {} contains a token without the {} prefix",
                    path.display(),
                    LOCAL_ADMIN_TOKEN_PREFIX
                ),
            )
        })?;
    let decoded = URL_SAFE_NO_PAD.decode(encoded).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "local admin token file {} contains an invalid base64url token payload: {error}",
                path.display()
            ),
        )
    })?;
    if decoded.len() != 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "local admin token file {} must decode to 32 token bytes; found {}",
                path.display(),
                decoded.len()
            ),
        ));
    }
    Ok(())
}

fn token_lock_path(token_path: &Path) -> PathBuf {
    token_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".token.lock")
}

#[cfg(unix)]
fn set_secure_file_permissions(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_secure_file_permissions(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_secure_path_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_secure_path_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_paths(root: &Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[test]
    fn load_or_create_creates_and_reuses_token_file() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());

        let first = load_or_create_local_admin_token(&paths).expect("token should be created");
        let second = load_or_create_local_admin_token(&paths).expect("token should be reused");

        assert_eq!(first, second);
        assert_eq!(first.generation, 1);
        assert!(
            paths.auth_token_path.exists(),
            "token file should exist after first start"
        );
    }

    #[test]
    fn offline_rotation_bumps_generation() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());

        let first = load_or_create_local_admin_token(&paths).expect("token should be created");
        let rotated =
            rotate_local_admin_token_offline(&paths).expect("offline rotation should succeed");

        assert_eq!(rotated.generation, first.generation + 1);
        assert_ne!(rotated.token, first.token);
        assert_eq!(
            load_local_admin_token(&paths).expect("rotated token should load"),
            rotated
        );
    }

    #[test]
    fn corrupt_token_file_errors_clearly() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        paths
            .ensure_auth_parent_dir()
            .expect("auth directory should build");
        fs::write(&paths.auth_token_path, b"not-json").expect("corrupt token file should write");

        let error = load_local_admin_token(&paths).expect_err("corrupt token file should not load");
        assert!(
            error.to_string().contains("is not valid JSON"),
            "error should explain why the token file is unreadable: {error}"
        );
    }

    #[test]
    fn token_authorization_accepts_only_exact_token_matches() {
        let record = generate_local_admin_token(1).expect("token should generate");

        assert!(record.authorize(&record.token));
        assert!(!record.authorize("neovex_at_not-the-real-token"));
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_written_with_user_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());

        let _record = load_or_create_local_admin_token(&paths).expect("token should be created");
        let mode = fs::metadata(&paths.auth_token_path)
            .expect("token metadata should load")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, 0o600);
    }

    #[test]
    fn source_uses_ring_constant_time_compare_for_token_checks() {
        let source = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/local_server/token.rs"),
        )
        .expect("token source should be readable");

        assert!(source.contains("hmac::verify("));
    }

    #[test]
    fn invalid_scope_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        paths
            .ensure_auth_parent_dir()
            .expect("auth directory should build");
        let mut record = generate_local_admin_token(1).expect("token should generate");
        record.scope = "unexpected".to_string();
        write_local_admin_token_file(&paths.auth_token_path, &record)
            .expect("invalid token fixture should write");

        let error = load_local_admin_token(&paths).expect_err("invalid token file should not load");
        assert!(error.to_string().contains("unsupported scope"));
    }
}
