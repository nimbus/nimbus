use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

const NIMBUS_DEPLOYMENT_KEY: &str = "NIMBUS_DEPLOYMENT";
const NIMBUS_KEY_PREFIX: &str = "NIMBUS_";

pub(super) fn write_env_local_deployment(app_dir: &Path, slug: &str) -> io::Result<()> {
    write_env_local_nimbus_keys(app_dir, &[(NIMBUS_DEPLOYMENT_KEY, format!("local:{slug}"))])
}

/// Write Nimbus-owned keys into `.env.local`: each key's existing line is
/// replaced in place (duplicates deduped) and missing keys are appended;
/// every other line is preserved byte-for-byte. Only `NIMBUS_*` keys may
/// flow through this writer — a user-owned key (e.g. `MONGODB_URI`) is
/// refused with `InvalidInput` before anything touches the file.
pub(super) fn write_env_local_nimbus_keys(
    app_dir: &Path,
    entries: &[(&str, String)],
) -> io::Result<()> {
    for (key, _) in entries {
        if !key.starts_with(NIMBUS_KEY_PREFIX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the .env.local writer owns only {NIMBUS_KEY_PREFIX}* keys; \
                     refusing to write {key}"
                ),
            ));
        }
    }
    if entries.is_empty() {
        return Ok(());
    }

    let env_path = app_dir.join(".env.local");
    let existing = match std::fs::read_to_string(&env_path) {
        Ok(existing) => existing,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };

    let mut content = existing.clone();
    for (key, value) in entries {
        let target_line = format!("{key}={value}");
        let key_prefix = format!("{key}=");
        if let Some(updated) = normalize_env_local_content(&content, &target_line, &key_prefix) {
            content = updated;
        }
    }

    if content != existing {
        write_text_file_atomically(&env_path, &content)?;
    }
    Ok(())
}

fn normalize_env_local_content(
    existing: &str,
    target_line: &str,
    key_prefix: &str,
) -> Option<String> {
    let line_ending = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = existing.ends_with('\n');
    let mut found = false;
    let mut updated = Vec::new();

    for line in existing.lines() {
        if line.starts_with(key_prefix) {
            if !found {
                updated.push(target_line.to_owned());
                found = true;
            }
        } else {
            updated.push(line.to_owned());
        }
    }

    let normalized = if found {
        let mut result = updated.join(line_ending);
        if had_trailing_newline {
            result.push_str(line_ending);
        }
        result
    } else {
        let mut result = existing.to_owned();
        if !result.ends_with('\n') && !result.is_empty() {
            result.push_str(line_ending);
        }
        result.push_str(target_line);
        result.push_str(line_ending);
        result
    };

    (normalized != existing).then_some(normalized)
}

fn write_text_file_atomically(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path {} does not have a parent directory", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;

    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;
    temp_file.into_temp_path().persist(path).map_err(|error| {
        io::Error::other(format!(
            "failed to atomically replace {}: {}",
            path.display(),
            error.error
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_local_writes_only_nimbus_owned_keys() {
        let temp = tempfile::tempdir().expect("tempdir");

        write_env_local_nimbus_keys(
            temp.path(),
            &[
                (
                    "NIMBUS_MONGODB_URL",
                    "mongodb://u:p@127.0.0.1:27017/".to_owned(),
                ),
                (
                    "NIMBUS_DYNAMODB_ENDPOINT",
                    "http://127.0.0.1:8000".to_owned(),
                ),
            ],
        )
        .expect("nimbus-owned keys should write");

        let content =
            std::fs::read_to_string(temp.path().join(".env.local")).expect("env file exists");
        assert!(
            content.lines().all(|line| line.starts_with("NIMBUS_")),
            "every written line must be Nimbus-owned: {content}"
        );
        assert!(content.contains("NIMBUS_MONGODB_URL=mongodb://u:p@127.0.0.1:27017/"));
        assert!(content.contains("NIMBUS_DYNAMODB_ENDPOINT=http://127.0.0.1:8000"));

        // A user-owned key is refused before anything touches the file.
        let error = write_env_local_nimbus_keys(
            temp.path(),
            &[("MONGODB_URI", "mongodb://elsewhere/".to_owned())],
        )
        .expect_err("a non-Nimbus key must be refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let after =
            std::fs::read_to_string(temp.path().join(".env.local")).expect("env file exists");
        assert_eq!(content, after, "a refusal must not modify the file");
    }

    #[test]
    fn user_owned_env_keys_are_never_clobbered() {
        let temp = tempfile::tempdir().expect("tempdir");
        let user_content = "MONGODB_URI=mongodb://prod.example.com/\nAPI_KEY=user-secret\n";
        std::fs::write(temp.path().join(".env.local"), user_content).expect("seed user env");

        write_env_local_nimbus_keys(
            temp.path(),
            &[(
                "NIMBUS_MONGODB_URL",
                "mongodb://u:p@127.0.0.1:27017/".to_owned(),
            )],
        )
        .expect("nimbus key should append");

        let content =
            std::fs::read_to_string(temp.path().join(".env.local")).expect("env file exists");
        assert!(
            content.starts_with(user_content),
            "user-owned lines must stay byte-identical: {content}"
        );
        assert!(content.contains("NIMBUS_MONGODB_URL=mongodb://u:p@127.0.0.1:27017/"));

        // A re-run with a changed value (e.g. port-conflict fallback)
        // updates only the Nimbus-owned line.
        write_env_local_nimbus_keys(
            temp.path(),
            &[(
                "NIMBUS_MONGODB_URL",
                "mongodb://u:p@127.0.0.1:54321/".to_owned(),
            )],
        )
        .expect("nimbus key should update in place");

        let updated =
            std::fs::read_to_string(temp.path().join(".env.local")).expect("env file exists");
        assert!(updated.starts_with(user_content));
        assert!(updated.contains("NIMBUS_MONGODB_URL=mongodb://u:p@127.0.0.1:54321/"));
        assert!(
            !updated.contains(":27017/"),
            "the stale Nimbus-owned value must be replaced: {updated}"
        );
    }
}
