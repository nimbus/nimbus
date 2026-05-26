use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

const NIMBUS_DEPLOYMENT_KEY: &str = "NIMBUS_DEPLOYMENT";

pub(super) fn write_env_local_deployment(app_dir: &Path, slug: &str) -> io::Result<()> {
    let env_path = app_dir.join(".env.local");
    let deployment_value = format!("local:{slug}");
    let target_line = format!("{NIMBUS_DEPLOYMENT_KEY}={deployment_value}");
    let key_prefix = format!("{NIMBUS_DEPLOYMENT_KEY}=");

    let content = match std::fs::read_to_string(&env_path) {
        Ok(existing) => normalize_env_local_content(&existing, &target_line, &key_prefix),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Some(format!("{target_line}\n")),
        Err(e) => return Err(e),
    };

    if let Some(content) = content {
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
