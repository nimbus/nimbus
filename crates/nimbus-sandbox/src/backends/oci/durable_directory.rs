//! Crash-safe establishment of an owned provider-state directory chain.
//!
//! A leaf-directory `fsync` cannot make newly created ancestors durable. This
//! seam validates, creates, and synchronizes every trusted component before a
//! provider writes an authority-bearing commit point beneath the leaf.

use std::fs;
use std::path::{Component, Path};

use crate::error::{Result, SandboxError};

pub(crate) fn establish_durable_directory_chain_with<F>(
    state_root: &Path,
    owned_directory: &Path,
    resource_label: &str,
    mut directory_sync: F,
) -> Result<()>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    let relative_directory =
        owned_directory
            .strip_prefix(state_root)
            .map_err(|_| SandboxError::OperationFailed {
                message: format!(
                    "{resource_label} directory {} escapes configured state root {}; \
                     publication remains fenced",
                    owned_directory.display(),
                    state_root.display()
                ),
            })?;
    if relative_directory
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "{resource_label} directory {} is not a normalized descendant of configured \
                 state root {}; publication remains fenced",
                owned_directory.display(),
                state_root.display()
            ),
        });
    }
    if state_root == Path::new("/") {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "configured state root {} has no owned parent directory; {resource_label} \
                 publication remains fenced",
                state_root.display()
            ),
        });
    }
    let state_parent = state_root
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    require_directory_component(state_parent, resource_label)?;

    let mut durable_chain = vec![state_parent.to_path_buf(), state_root.to_path_buf()];
    let mut current = state_root.to_path_buf();
    for component in relative_directory.components() {
        let Component::Normal(component) = component else {
            unreachable!("non-normal components were rejected above");
        };
        current.push(component);
        durable_chain.push(current.clone());
    }

    for directory in durable_chain.iter().skip(1) {
        match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(non_directory_component(directory, resource_label)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_directory_component_with(directory, resource_label, |path| {
                    fs::create_dir(path)
                })?;
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to inspect {resource_label} directory component {}: {error}",
                        directory.display()
                    ),
                });
            }
        }
        require_directory_component(directory, resource_label)?;
    }

    for directory in durable_chain {
        directory_sync(&directory).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to durably establish {resource_label} directory {} before publication: \
                 {error}",
                directory.display()
            ),
        })?;
    }
    Ok(())
}

fn create_directory_component_with<F>(
    path: &Path,
    resource_label: &str,
    mut create: F,
) -> Result<()>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    match create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            require_directory_component(path, resource_label)
        }
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to create {resource_label} directory component {}: {error}",
                path.display()
            ),
        }),
    }
}

fn require_directory_component(path: &Path, resource_label: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to inspect {resource_label} directory component {}: {error}",
            path.display()
        ),
    })?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(non_directory_component(path, resource_label))
    }
}

fn non_directory_component(path: &Path, resource_label: &str) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "{resource_label} directory component {} is not a directory; publication remains \
             fenced",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_component_creation_revalidates_the_winning_directory() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let component = temp_dir.path().join("shared");

        create_directory_component_with(&component, "test provider state", |path| {
            fs::create_dir(path).expect("the concurrent winner should create the directory");
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        })
        .expect("a concurrently created real directory should satisfy the same invariant");
        assert!(
            fs::symlink_metadata(&component)
                .expect("winning component should exist")
                .file_type()
                .is_dir(),
            "the accepted concurrent winner must be a real directory"
        );
    }

    #[test]
    fn concurrent_component_creation_rejects_a_non_directory_winner() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let component = temp_dir.path().join("shared");

        let error = create_directory_component_with(&component, "test provider state", |path| {
            fs::write(path, b"not a directory")
                .expect("the concurrent non-directory winner should create");
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        })
        .expect_err("a concurrent non-directory must not satisfy the directory invariant");
        assert!(
            error.to_string().contains("is not a directory"),
            "the losing creator must report the violated directory invariant: {error}"
        );
    }
}
