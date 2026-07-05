use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fs2::FileExt;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::paths::LocalServerPaths;

const LOCAL_ADMIN_TOKEN_VERSION: u32 = 1;
const LOCAL_ADMIN_TOKEN_PREFIX: &str = "nimbus_at_";

pub const LOCAL_ADMIN_TOKEN_SCOPE: &str = "local-admin";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAdminTokenRecord {
    pub version: u32,
    pub token: String,
    pub generation: u64,
    pub issued_at: String,
    pub scope: String,
    /// RFC 3339 timestamp of the last explicit rotation, or `None` for the
    /// auto-minted first-boot token. Used by the non-loopback bind tripwire
    /// in `nimbus start` to refuse exposing a never-rotated token on a
    /// public interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotated_at: Option<String>,
}

impl LocalAdminTokenRecord {
    pub fn authorize(&self, candidate: &str) -> bool {
        constant_time_eq(self.token.as_bytes(), candidate.as_bytes())
    }

    /// True when the record carries a parseable `rotated_at` timestamp that
    /// is no older than `max_age`. Never-rotated tokens (rotated_at = None)
    /// are always considered stale: the non-loopback bind tripwire must
    /// demand an explicit rotation before exposing the token publicly.
    pub fn rotation_is_fresh(&self, now: OffsetDateTime, max_age: time::Duration) -> bool {
        let Some(rotated_at) = self.rotated_at.as_deref() else {
            return false;
        };
        let Ok(rotated) = OffsetDateTime::parse(rotated_at, &Rfc3339) else {
            return false;
        };
        now - rotated < max_age
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&left, &right) in left.iter().zip(right) {
        diff |= left ^ right;
    }
    diff == 0
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
                    "local admin token file {} does not exist; run `nimbus start` once to create it",
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
        let generation = match read_local_admin_token_file(&paths.auth_token_path)? {
            Some(current) => current.generation.saturating_add(1),
            None => 1,
        };
        let mut rotated = generate_local_admin_token(generation)?;
        rotated.rotated_at = Some(now_rfc3339()?);
        write_local_admin_token_file(&paths.auth_token_path, &rotated)?;
        Ok(rotated)
    })
}

pub fn now_rfc3339() -> io::Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| io::Error::other(format!("failed to format timestamp: {error}")))
}

pub fn with_token_file_lock<T>(
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

pub fn read_local_admin_token_file(path: &Path) -> io::Result<Option<LocalAdminTokenRecord>> {
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

pub fn write_local_admin_token_file(path: &Path, record: &LocalAdminTokenRecord) -> io::Result<()> {
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

pub fn generate_local_admin_token(generation: u64) -> io::Result<LocalAdminTokenRecord> {
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
        rotated_at: None,
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
    fn offline_rotation_initializes_missing_token_as_explicit_rotation() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());

        let rotated =
            rotate_local_admin_token_offline(&paths).expect("offline rotation should succeed");

        assert_eq!(rotated.generation, 1);
        assert!(
            rotated.rotated_at.is_some(),
            "explicit offline rotation must populate rotated_at even without a prior first boot"
        );
        assert!(
            rotated.rotation_is_fresh(OffsetDateTime::now_utc(), time::Duration::days(30)),
            "freshly initialized rotation must pass the public bind freshness gate"
        );
        assert_eq!(
            load_local_admin_token(&paths).expect("rotated token should load"),
            rotated
        );
    }

    #[test]
    fn offline_rotation_populates_rotated_at_freshness_window() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());

        let first = load_or_create_local_admin_token(&paths).expect("token should be created");
        assert!(
            first.rotated_at.is_none(),
            "auto-minted first-boot token must leave rotated_at unset"
        );
        assert!(
            !first.rotation_is_fresh(OffsetDateTime::now_utc(), time::Duration::days(30)),
            "never-rotated token must report as stale to the bind tripwire"
        );

        let rotated =
            rotate_local_admin_token_offline(&paths).expect("offline rotation should succeed");
        let rotated_at = rotated
            .rotated_at
            .as_deref()
            .expect("offline rotation must populate rotated_at");
        OffsetDateTime::parse(rotated_at, &Rfc3339)
            .expect("rotated_at should round-trip as RFC 3339");
        assert!(
            rotated.rotation_is_fresh(OffsetDateTime::now_utc(), time::Duration::days(30)),
            "freshly rotated token must report fresh inside the 30-day window"
        );
    }

    #[test]
    fn records_persisted_without_rotated_at_load_with_none() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        paths
            .ensure_auth_parent_dir()
            .expect("auth directory should build");
        // Older on-disk format omitted `rotatedAt`. Confirm we still load it
        // and treat it as never-rotated (the bind tripwire then forces an
        // explicit rotation before exposing the token publicly).
        let legacy = serde_json::json!({
            "version": LOCAL_ADMIN_TOKEN_VERSION,
            "token": "nimbus_at_8j-X3yfa1RuC0WMNB7TtoFu1eK0vCSEhSAxwo0xPYcA",
            "generation": 1,
            "issuedAt": "2026-01-01T00:00:00Z",
            "scope": LOCAL_ADMIN_TOKEN_SCOPE,
        });
        fs::write(
            &paths.auth_token_path,
            serde_json::to_vec_pretty(&legacy).expect("legacy fixture should serialize"),
        )
        .expect("legacy token fixture should write");

        let record = load_local_admin_token(&paths).expect("legacy file should load");
        assert!(record.rotated_at.is_none());
        assert!(!record.rotation_is_fresh(OffsetDateTime::now_utc(), time::Duration::days(30)));
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
        let mut longer = record.token.clone();
        longer.push('x');

        assert!(record.authorize(&record.token));
        assert!(!record.authorize("nimbus_at_not-the-real-token"));
        assert!(!record.authorize(&record.token[..record.token.len() - 1]));
        assert!(!record.authorize(&longer));
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
    fn source_uses_direct_constant_time_compare_for_token_checks() {
        let crate_root = std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-operator tests");
        let source = fs::read_to_string(crate_root.join("src/token.rs"))
            .expect("token source should be readable");

        assert!(source.contains("fn constant_time_eq("));
        assert!(source.contains("diff |= left ^ right;"));
        let hmac_import = ["use ring", "::hmac;"].concat();
        assert!(!source.contains(&hmac_import));
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
