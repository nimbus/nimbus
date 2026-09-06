use std::error::Error;
use std::path::Path;

use nimbus::EmbeddedProviderKind;

pub(crate) fn require_existing_control_plane(
    control_data_dir: &Path,
    operation: &str,
) -> Result<(), Box<dyn Error>> {
    let control_database =
        control_data_dir.join(EmbeddedProviderKind::Redb.control_database_filename());
    let metadata = std::fs::symlink_metadata(&control_database).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!(
                "{operation} requires an existing control-plane database at {}; pass the deployment's exact --control-data-dir for split-root storage",
                control_database.display()
            )
        } else {
            format!(
                "{operation} could not inspect control-plane database {}: {error}",
                control_database.display()
            )
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "{operation} requires a regular control-plane database at {}; refusing a directory or symlink",
            control_database.display()
        )
        .into());
    }
    Ok(())
}
