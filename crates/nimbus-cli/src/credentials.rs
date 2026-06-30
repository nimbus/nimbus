use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::dirs::global_config_dir;

const CREDENTIALS_FILE_NAME: &str = "credentials";

/// On-disk shape of `~/.config/nimbus/credentials`.
///
/// TOML, keyed by daemon URL. Mirrors the Podman `connections.conf` and
/// Fly `~/.config/fly/auth.toml` precedents. See DEP2.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CredentialsFile {
    #[serde(default)]
    pub(crate) connection: BTreeMap<String, ConnectionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ConnectionEntry {
    pub(crate) bearer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_used_at: Option<String>,
}

/// Resolve `~/.config/nimbus/credentials`, honoring `XDG_CONFIG_HOME`.
pub(crate) fn default_credentials_path() -> Result<PathBuf, io::Error> {
    let dir = global_config_dir()
        .map_err(|error| io::Error::new(io::ErrorKind::NotFound, error.to_string()))?;
    Ok(dir.join(CREDENTIALS_FILE_NAME))
}

pub(crate) fn read_credentials_file(path: &Path) -> io::Result<CredentialsFile> {
    let bytes = match fs::read_to_string(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(CredentialsFile::default());
        }
        Err(error) => return Err(error),
    };
    toml::from_str(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "credentials file {} is not valid TOML: {error}",
                path.display()
            ),
        )
    })
}

pub(crate) fn write_credentials_file(path: &Path, file: &CredentialsFile) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "credentials path {} does not have a parent directory",
                path.display()
            ),
        )
    })?;
    fs::create_dir_all(parent)?;
    set_secure_dir_permissions(parent)?;
    let bytes = toml::to_string_pretty(file).map_err(|error| {
        io::Error::other(format!(
            "failed to serialize credentials file {}: {error}",
            path.display()
        ))
    })?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(bytes.as_bytes())?;
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

/// Normalize a daemon URL so credentials and lookups share one canonical
/// spelling. Strips trailing slashes; everything else is left to the user.
pub(crate) fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// Upsert a connection entry. Sets `last_used_at` to now when the caller
/// passes `mark_used = true`. Preserves an existing `expires_at` unless the
/// caller supplies a new one.
pub(crate) fn upsert_connection(
    file: &mut CredentialsFile,
    url: &str,
    bearer: String,
    expires_at: Option<String>,
    mark_used: bool,
) -> io::Result<()> {
    let key = normalize_url(url);
    let now = if mark_used {
        Some(now_rfc3339()?)
    } else {
        None
    };
    let entry = file.connection.entry(key).or_insert(ConnectionEntry {
        bearer: String::new(),
        expires_at: None,
        last_used_at: None,
    });
    entry.bearer = bearer;
    if expires_at.is_some() {
        entry.expires_at = expires_at;
    }
    if let Some(stamp) = now {
        entry.last_used_at = Some(stamp);
    }
    Ok(())
}

pub(crate) fn remove_connection(file: &mut CredentialsFile, url: &str) -> bool {
    let key = normalize_url(url);
    file.connection.remove(&key).is_some()
}

pub(crate) fn find_connection<'a>(
    file: &'a CredentialsFile,
    url: &str,
) -> Option<&'a ConnectionEntry> {
    let key = normalize_url(url);
    file.connection.get(&key)
}

fn now_rfc3339() -> io::Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| io::Error::other(format!("failed to format timestamp: {error}")))
}

/// Mask a bearer for `nimbus auth status` output: show the prefix up to
/// (and including) the trailing underscore plus the last four characters.
pub(crate) fn mask_bearer(bearer: &str) -> String {
    let trimmed = bearer.trim();
    if trimmed.is_empty() {
        return "(empty)".to_string();
    }
    let (prefix, rest) = match trimmed.rfind('_') {
        Some(index) => trimmed.split_at(index + 1),
        None => ("", trimmed),
    };
    if rest.len() <= 4 {
        return format!("{prefix}…{rest}");
    }
    let tail_start = rest.len() - 4;
    format!("{prefix}…{}", &rest[tail_start..])
}

#[cfg(unix)]
fn set_secure_file_permissions(file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_secure_file_permissions(_file: &std::fs::File) -> io::Result<()> {
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

#[cfg(unix)]
fn set_secure_dir_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_secure_dir_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_round_trips_through_read() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("credentials");
        let mut file = CredentialsFile::default();
        upsert_connection(
            &mut file,
            "https://nimbus.example.com",
            "deploy_tok_abcdef0123456789".to_string(),
            Some("2026-12-01T00:00:00Z".to_string()),
            true,
        )
        .expect("upsert should succeed");
        write_credentials_file(&path, &file).expect("write should succeed");
        let loaded = read_credentials_file(&path).expect("read should succeed");
        assert_eq!(loaded.connection.len(), 1);
        let entry = loaded
            .connection
            .get("https://nimbus.example.com")
            .expect("entry should exist");
        assert_eq!(entry.bearer, "deploy_tok_abcdef0123456789");
        assert_eq!(entry.expires_at.as_deref(), Some("2026-12-01T00:00:00Z"));
        assert!(
            entry.last_used_at.is_some(),
            "mark_used should set timestamp"
        );
    }

    #[test]
    fn read_missing_file_returns_empty_credentials() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("does-not-exist");
        let loaded = read_credentials_file(&path).expect("missing file should read as empty");
        assert!(loaded.connection.is_empty());
    }

    #[test]
    fn upsert_overwrites_bearer_and_preserves_expires_at_when_unset() {
        let mut file = CredentialsFile::default();
        upsert_connection(
            &mut file,
            "http://localhost:3210",
            "deploy_tok_one".to_string(),
            Some("2026-12-01T00:00:00Z".to_string()),
            false,
        )
        .expect("first upsert should succeed");
        upsert_connection(
            &mut file,
            "http://localhost:3210",
            "deploy_tok_two".to_string(),
            None,
            false,
        )
        .expect("second upsert should succeed");
        let entry = file
            .connection
            .get("http://localhost:3210")
            .expect("entry should exist");
        assert_eq!(entry.bearer, "deploy_tok_two");
        assert_eq!(
            entry.expires_at.as_deref(),
            Some("2026-12-01T00:00:00Z"),
            "expires_at must persist when the second upsert passes None"
        );
    }

    #[test]
    fn remove_connection_drops_entry_and_reports_presence() {
        let mut file = CredentialsFile::default();
        upsert_connection(
            &mut file,
            "http://localhost:3210",
            "deploy_tok_one".to_string(),
            None,
            false,
        )
        .unwrap();
        assert!(remove_connection(&mut file, "http://localhost:3210"));
        assert!(!remove_connection(&mut file, "http://localhost:3210"));
        assert!(file.connection.is_empty());
    }

    #[test]
    fn normalize_url_strips_trailing_slash_and_whitespace() {
        assert_eq!(
            normalize_url("  http://localhost:3210/  "),
            "http://localhost:3210"
        );
        assert_eq!(
            normalize_url("http://localhost:3210"),
            "http://localhost:3210"
        );
    }

    #[test]
    fn lookup_matches_normalized_url() {
        let mut file = CredentialsFile::default();
        upsert_connection(
            &mut file,
            "http://localhost:3210",
            "deploy_tok_xyz".to_string(),
            None,
            false,
        )
        .unwrap();
        assert!(
            find_connection(&file, "http://localhost:3210/").is_some(),
            "trailing-slash URL should match the canonical key"
        );
    }

    #[test]
    fn mask_bearer_keeps_prefix_and_last_four() {
        assert_eq!(
            mask_bearer("deploy_tok_abcdef0123456789"),
            "deploy_tok_…6789"
        );
        assert_eq!(mask_bearer("noprefix"), "…efix");
        assert_eq!(mask_bearer(""), "(empty)");
    }

    #[cfg(unix)]
    #[test]
    fn write_credentials_file_sets_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("credentials");
        let mut file = CredentialsFile::default();
        upsert_connection(
            &mut file,
            "http://localhost:3210",
            "deploy_tok_xyz".to_string(),
            None,
            false,
        )
        .unwrap();
        write_credentials_file(&path, &file).expect("write should succeed");
        let mode = fs::metadata(&path)
            .expect("file should exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "credentials file must be mode 0600");
    }
}
