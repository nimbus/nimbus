use std::path::Path;

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
