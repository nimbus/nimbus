use nimbus_core::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StorageFormatVersion(pub u16);

pub const CURRENT_STORAGE_FORMAT_VERSION: StorageFormatVersion = StorageFormatVersion(1);

pub fn storage_format_version() -> StorageFormatVersion {
    CURRENT_STORAGE_FORMAT_VERSION
}

pub fn validate_storage_format_version(version: StorageFormatVersion) -> Result<()> {
    if version == CURRENT_STORAGE_FORMAT_VERSION {
        return Ok(());
    }
    if version.0 > CURRENT_STORAGE_FORMAT_VERSION.0 {
        return Err(Error::Internal(format!(
            "unknown future storage format version {}; current version is {}",
            version.0, CURRENT_STORAGE_FORMAT_VERSION.0
        )));
    }
    Err(Error::Internal(format!(
        "unsupported old storage format version {}; current version is {}",
        version.0, CURRENT_STORAGE_FORMAT_VERSION.0
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_storage_format_version_is_rejected() {
        let err = validate_storage_format_version(StorageFormatVersion(
            CURRENT_STORAGE_FORMAT_VERSION.0 + 1,
        ))
        .expect_err("unknown future versions must fail closed");
        assert!(
            err.to_string()
                .contains("unknown future storage format version")
        );
    }
}
