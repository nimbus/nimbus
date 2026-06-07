use nimbus_core::{Error, HistoricalReadErrorKind, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StorageFormatVersion(pub u16);

pub const CURRENT_STORAGE_FORMAT_VERSION: StorageFormatVersion = StorageFormatVersion(1);
pub const CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT: StorageFormatVersion = StorageFormatVersion(1);
pub const DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY: &str = "document_versions.storage_format";
pub const CURRENT_INDEX_VERSION_STORAGE_FORMAT: StorageFormatVersion = StorageFormatVersion(1);
pub const INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY: &str = "index_versions.storage_format";

pub fn storage_format_version() -> StorageFormatVersion {
    CURRENT_STORAGE_FORMAT_VERSION
}

pub fn validate_storage_format_version(version: StorageFormatVersion) -> Result<()> {
    validate_named_storage_format_version("storage format", version, CURRENT_STORAGE_FORMAT_VERSION)
}

pub fn validate_document_version_storage_format(version: StorageFormatVersion) -> Result<()> {
    validate_named_historical_storage_format_version(
        "document-version storage format",
        version,
        CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT,
    )
}

pub fn validate_index_version_storage_format(version: StorageFormatVersion) -> Result<()> {
    validate_named_historical_storage_format_version(
        "index-version storage format",
        version,
        CURRENT_INDEX_VERSION_STORAGE_FORMAT,
    )
}

pub fn storage_format_version_from_u64(value: u64) -> Result<StorageFormatVersion> {
    let version = u16::try_from(value).map_err(|_| {
        Error::Internal(format!(
            "storage format version {value} exceeds the supported u16 range"
        ))
    })?;
    Ok(StorageFormatVersion(version))
}

pub fn validate_document_version_storage_format_state(
    version: Option<StorageFormatVersion>,
    has_versions: bool,
) -> Result<()> {
    match version {
        Some(version) => validate_document_version_storage_format(version),
        None if has_versions => Err(Error::historical_read(
            HistoricalReadErrorKind::FormatMismatch,
            "document-version rows exist without a storage format marker",
        )),
        None => Ok(()),
    }
}

pub fn validate_index_version_storage_format_state(
    version: Option<StorageFormatVersion>,
    has_versions: bool,
) -> Result<()> {
    match version {
        Some(version) => validate_index_version_storage_format(version),
        None if has_versions => Err(Error::historical_read(
            HistoricalReadErrorKind::FormatMismatch,
            "index-version rows exist without a storage format marker",
        )),
        None => Ok(()),
    }
}

fn validate_named_storage_format_version(
    label: &str,
    version: StorageFormatVersion,
    current: StorageFormatVersion,
) -> Result<()> {
    if version == current {
        return Ok(());
    }
    if version.0 > current.0 {
        return Err(Error::Internal(format!(
            "unknown future {label} version {}; current version is {}",
            version.0, current.0
        )));
    }
    Err(Error::Internal(format!(
        "unsupported old {label} version {}; current version is {}",
        version.0, current.0
    )))
}

fn validate_named_historical_storage_format_version(
    label: &str,
    version: StorageFormatVersion,
    current: StorageFormatVersion,
) -> Result<()> {
    if version == current {
        return Ok(());
    }
    if version.0 > current.0 {
        return Err(Error::historical_read(
            HistoricalReadErrorKind::FormatMismatch,
            format!(
                "unknown future {label} version {}; current version is {}",
                version.0, current.0
            ),
        ));
    }
    Err(Error::historical_read(
        HistoricalReadErrorKind::FormatMismatch,
        format!(
            "unsupported old {label} version {}; current version is {}",
            version.0, current.0
        ),
    ))
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

    #[test]
    fn unknown_document_version_storage_format_is_rejected() {
        let err = validate_document_version_storage_format(StorageFormatVersion(
            CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0 + 1,
        ))
        .expect_err("unknown future document-version versions must fail closed");
        assert_eq!(
            err.historical_read_kind(),
            Some(HistoricalReadErrorKind::FormatMismatch)
        );
        assert!(
            err.to_string()
                .contains("unknown future document-version storage format version")
        );
    }

    #[test]
    fn unknown_index_version_storage_format_is_rejected() {
        let err = validate_index_version_storage_format(StorageFormatVersion(
            CURRENT_INDEX_VERSION_STORAGE_FORMAT.0 + 1,
        ))
        .expect_err("unknown future index-version versions must fail closed");
        assert_eq!(
            err.historical_read_kind(),
            Some(HistoricalReadErrorKind::FormatMismatch)
        );
        assert!(
            err.to_string()
                .contains("unknown future index-version storage format version")
        );
    }

    #[test]
    fn version_rows_without_history_format_marker_are_typed_format_mismatches() {
        let document_err = validate_document_version_storage_format_state(None, true)
            .expect_err("document-version rows without marker must fail closed");
        assert_eq!(
            document_err.historical_read_kind(),
            Some(HistoricalReadErrorKind::FormatMismatch)
        );

        let index_err = validate_index_version_storage_format_state(None, true)
            .expect_err("index-version rows without marker must fail closed");
        assert_eq!(
            index_err.historical_read_kind(),
            Some(HistoricalReadErrorKind::FormatMismatch)
        );
    }
}
