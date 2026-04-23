use std::path::Path;
use std::sync::{Mutex, OnceLock};

fn process_cwd_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn with_current_dir<T>(path: &Path, operation: impl FnOnce() -> T) -> T {
    let _guard = process_cwd_lock()
        .lock()
        .expect("cwd test lock should not be poisoned");
    let previous = std::env::current_dir().expect("current directory should resolve");
    std::env::set_current_dir(path).expect("test current directory should update");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    std::env::set_current_dir(previous).expect("current directory should restore");
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[cfg(unix)]
pub(crate) fn write_executable_stub(path: &Path, contents: &str) {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::NamedTempFile;

    let parent = path.parent().unwrap_or_else(|| {
        panic!(
            "stub path {} should have a parent directory",
            path.display()
        );
    });
    let mut temp_file = NamedTempFile::new_in(parent).unwrap_or_else(|error| {
        panic!(
            "temporary executable stub for {} should write: {error}",
            path.display()
        );
    });
    temp_file
        .write_all(contents.as_bytes())
        .unwrap_or_else(|error| {
            panic!("executable stub {} should write: {error}", path.display());
        });
    temp_file.flush().unwrap_or_else(|error| {
        panic!("executable stub {} should flush: {error}", path.display());
    });

    let mut permissions = temp_file
        .as_file()
        .metadata()
        .unwrap_or_else(|error| {
            panic!(
                "executable stub {} metadata should exist: {error}",
                path.display()
            );
        })
        .permissions();
    permissions.set_mode(0o755);
    temp_file
        .as_file()
        .set_permissions(permissions)
        .unwrap_or_else(|error| {
            panic!(
                "executable stub {} should be executable: {error}",
                path.display()
            );
        });
    temp_file.as_file().sync_all().unwrap_or_else(|error| {
        panic!("executable stub {} should sync: {error}", path.display());
    });
    temp_file
        .into_temp_path()
        .persist(path)
        .unwrap_or_else(|error| {
            panic!(
                "executable stub {} should persist: {}",
                path.display(),
                error.error
            );
        });
}

#[cfg(not(unix))]
pub(crate) fn write_executable_stub(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap_or_else(|error| {
        panic!("executable stub {} should write: {error}", path.display());
    });
}
