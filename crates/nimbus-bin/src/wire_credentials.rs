use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const WIRE_CREDENTIALS_FILE_NAME: &str = "wire-credentials.json";
const MONGODB_DEV_USERNAME: &str = "nimbus";

/// Generated credentials for the wire-protocol listeners (MongoDB SCRAM,
/// DynamoDB SigV4), persisted per data dir so connection strings survive
/// restarts (decision D5) and shared by `nimbus dev` and `nimbus start`
/// (decision D7) — both commands load-or-generate the same store, so the
/// same data dir always speaks with the same secrets. Rotation = delete
/// the file and restart: a running session keeps serving with the
/// credentials it loaded at boot, and the next boot generates a fresh
/// set and refreshes the Nimbus-owned `.env.local` keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WireCredentials {
    pub(crate) mongodb_username: String,
    pub(crate) mongodb_password: String,
    pub(crate) dynamodb_access_key_id: String,
    pub(crate) dynamodb_secret_access_key: String,
}

pub(crate) fn wire_credentials_path(data_dir: &Path) -> PathBuf {
    data_dir.join(WIRE_CREDENTIALS_FILE_NAME)
}

/// Load the persisted wire credentials for `data_dir`, generating and
/// persisting a fresh set (owner-only file mode) when none exist yet. A
/// malformed store is an error, not a silent regeneration: regenerating
/// would invalidate credentials an app's `.env.local` may still carry.
pub(crate) fn load_or_generate(data_dir: &Path) -> io::Result<WireCredentials> {
    let path = wire_credentials_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "wire-credential store {} is malformed ({error}); \
                     delete it to regenerate dev credentials",
                    path.display()
                ),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let credentials = generate();
            persist(&path, &credentials)?;
            Ok(credentials)
        }
        Err(error) => Err(error),
    }
}

fn generate() -> WireCredentials {
    WireCredentials {
        mongodb_username: MONGODB_DEV_USERNAME.to_owned(),
        mongodb_password: random_hex(16),
        dynamodb_access_key_id: format!("AKIA{}", random_hex(8).to_uppercase()),
        dynamodb_secret_access_key: random_hex(20),
    }
}

fn random_hex(byte_len: usize) -> String {
    let mut bytes = vec![0_u8; byte_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(byte_len * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn persist(path: &Path, credentials: &WireCredentials) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path {} does not have a parent directory", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;

    let mut temp_file = NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp_file
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    let serialized = serde_json::to_string_pretty(credentials).map_err(io::Error::other)?;
    temp_file.write_all(serialized.as_bytes())?;
    temp_file.write_all(b"\n")?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;
    temp_file.into_temp_path().persist(path).map_err(|error| {
        io::Error::other(format!(
            "failed to atomically write {}: {}",
            path.display(),
            error.error
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_wire_credentials_persist_across_runs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_dir = temp.path().join(".nimbus").join("dev");

        let first = load_or_generate(&data_dir).expect("first run should generate");
        let second = load_or_generate(&data_dir).expect("second run should load");

        assert_eq!(
            first, second,
            "a restart must hand back the same credentials"
        );
        assert!(wire_credentials_path(&data_dir).is_file());
        assert_eq!(first.mongodb_username, "nimbus");
        assert_eq!(first.mongodb_password.len(), 32);
        assert!(first.dynamodb_access_key_id.starts_with("AKIA"));
        assert_eq!(first.dynamodb_access_key_id.len(), 20);
        assert_eq!(first.dynamodb_secret_access_key.len(), 40);
    }

    #[test]
    fn generated_credentials_differ_per_data_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first = load_or_generate(&temp.path().join("a")).expect("generate a");
        let second = load_or_generate(&temp.path().join("b")).expect("generate b");
        assert_ne!(first.mongodb_password, second.mongodb_password);
        assert_ne!(
            first.dynamodb_secret_access_key,
            second.dynamodb_secret_access_key
        );
    }

    #[cfg(unix)]
    #[test]
    fn credential_file_is_owner_only_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        load_or_generate(temp.path()).expect("generate");

        let mode = std::fs::metadata(wire_credentials_path(temp.path()))
            .expect("credential file metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "credential file must be owner-only");
    }

    #[test]
    fn malformed_credential_file_errors_with_rotation_hint() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(wire_credentials_path(temp.path()), "{ not json").expect("write garbage");

        let error = load_or_generate(temp.path()).expect_err("malformed store must error");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("delete it to regenerate"),
            "error must carry the rotation hint: {error}"
        );
    }
}
