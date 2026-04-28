use std::env;
use std::path::{Path, PathBuf};

use neovex::Error;
use sha2::{Digest, Sha256};

const SLUG_HASH_HEX_LEN: usize = 8;

pub(crate) fn global_config_dir() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("neovex"));
    }
    Ok(resolve_home_dir()?.join(".config").join("neovex"))
}

pub(crate) fn deployment_slug(app_dir: &Path) -> Result<String, Error> {
    let canonical = app_dir.canonicalize().map_err(|e| {
        Error::InvalidInput(format!(
            "cannot canonicalize app directory {}: {e}",
            app_dir.display()
        ))
    })?;

    let dir_name = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app");
    let sanitized = sanitize_dir_name(dir_name);

    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let mut hash = String::with_capacity(SLUG_HASH_HEX_LEN);
    for byte in digest.iter().take(SLUG_HASH_HEX_LEN / 2) {
        hash.push_str(&format!("{byte:02x}"));
    }

    Ok(format!("{sanitized}-{hash}"))
}

fn sanitize_dir_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c == '-' {
                Some('-')
            } else {
                None
            }
        })
        .collect();
    if sanitized.is_empty() {
        "app".to_owned()
    } else {
        sanitized
    }
}

fn resolve_home_dir() -> Result<PathBuf, Error> {
    env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        Error::InvalidInput("HOME is not set; cannot resolve global config directory".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Mutex, OnceLock};

    use super::*;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn slug_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let slug_a = deployment_slug(dir.path()).unwrap();
        let slug_b = deployment_slug(dir.path()).unwrap();
        assert_eq!(slug_a, slug_b, "same path must produce the same slug");
    }

    #[test]
    fn slug_different_paths_differ() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let slug_a = deployment_slug(dir_a.path()).unwrap();
        let slug_b = deployment_slug(dir_b.path()).unwrap();
        assert_ne!(
            slug_a, slug_b,
            "different paths must produce different slugs"
        );
    }

    #[test]
    fn slug_format_matches_spec() {
        let dir = tempfile::tempdir().unwrap();
        let slug = deployment_slug(dir.path()).unwrap();

        let parts: Vec<&str> = slug.rsplitn(2, '-').collect();
        assert_eq!(parts.len(), 2, "slug must have name-hash format");

        let hash_part = parts[0];
        assert_eq!(
            hash_part.len(),
            SLUG_HASH_HEX_LEN,
            "hash must be {SLUG_HASH_HEX_LEN} hex chars"
        );
        assert!(
            hash_part.chars().all(|c| c.is_ascii_hexdigit()),
            "hash part must be hex digits, got: {hash_part}"
        );
    }

    #[test]
    fn slug_with_spaces_in_dir_name() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("my cool app");
        fs::create_dir(&dir).unwrap();
        let slug = deployment_slug(&dir).unwrap();
        assert!(
            slug.starts_with("mycoolapp-"),
            "spaces must be stripped, got: {slug}"
        );
    }

    #[test]
    fn slug_with_special_chars_in_dir_name() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("my_app@v2.0!");
        fs::create_dir(&dir).unwrap();
        let slug = deployment_slug(&dir).unwrap();
        assert!(
            slug.starts_with("myappv20-"),
            "special chars must be stripped, got: {slug}"
        );
    }

    #[test]
    fn slug_with_hyphens_preserved() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("my-cool-app");
        fs::create_dir(&dir).unwrap();
        let slug = deployment_slug(&dir).unwrap();
        assert!(
            slug.starts_with("my-cool-app-"),
            "hyphens must be preserved, got: {slug}"
        );
    }

    #[test]
    fn slug_all_special_chars_falls_back_to_app() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("@!#$%");
        fs::create_dir(&dir).unwrap();
        let slug = deployment_slug(&dir).unwrap();
        assert!(
            slug.starts_with("app-"),
            "all-special-char names must fall back to 'app', got: {slug}"
        );
    }

    #[test]
    fn slug_uppercase_is_lowercased() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("MyApp");
        fs::create_dir(&dir).unwrap();
        let slug = deployment_slug(&dir).unwrap();
        assert!(
            slug.starts_with("myapp-"),
            "uppercase must be lowercased, got: {slug}"
        );
    }

    #[test]
    fn slug_nonexistent_dir_returns_error() {
        let result = deployment_slug(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(result.is_err(), "nonexistent path must return an error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("cannot canonicalize"),
            "error must mention canonicalize, got: {err}"
        );
    }

    #[test]
    fn global_config_dir_respects_xdg_override() {
        let _lock = env_lock().lock().expect("env lock should not be poisoned");
        let _guard = EnvGuard::new("XDG_CONFIG_HOME", "/custom/config");
        let dir = global_config_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/custom/config/neovex"));
    }

    #[test]
    fn global_config_dir_falls_back_to_home() {
        let _lock = env_lock().lock().expect("env lock should not be poisoned");
        let _guard = EnvGuard::clear("XDG_CONFIG_HOME");
        let dir = global_config_dir().unwrap();
        let home = env::var_os("HOME").unwrap();
        assert_eq!(dir, PathBuf::from(home).join(".config").join("neovex"));
    }

    #[test]
    fn sanitize_dir_name_empty_input() {
        assert_eq!(sanitize_dir_name(""), "app");
    }

    #[test]
    fn sanitize_dir_name_unicode() {
        assert_eq!(sanitize_dir_name("日本語アプリ"), "app");
    }

    #[test]
    fn sanitize_dir_name_mixed_unicode_ascii() {
        assert_eq!(sanitize_dir_name("app-日本語"), "app-");
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn new(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            unsafe { env::set_var(key, value) };
            Self { key, previous }
        }

        fn clear(key: &'static str) -> Self {
            let previous = env::var_os(key);
            unsafe { env::remove_var(key) };
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { env::set_var(self.key, value) },
                None => unsafe { env::remove_var(self.key) },
            }
        }
    }
}
