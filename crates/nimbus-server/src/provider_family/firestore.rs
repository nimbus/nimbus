use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nimbus_core::{CollectionPath, DocumentLocator, DocumentPath, Error, Result, TableName};
use ring::digest::{SHA256, digest};

const DEFAULT_FIRESTORE_DATABASE_ID: &str = "(default)";

pub(crate) fn parse_document_path(path: &str, label: &str) -> Result<DocumentPath> {
    if path.is_empty() {
        return Err(Error::InvalidInput(format!("{label} cannot be empty")));
    }
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(Error::InvalidInput(format!(
            "{label} must not contain empty segments, double slashes, or trailing slashes"
        )));
    }
    DocumentPath::from_segments(segments.iter().copied())
        .map_err(|error| Error::InvalidInput(format!("{label}: {error}")))
}

pub(crate) fn validate_default_database_id(database_id: &str, label: &str) -> Result<()> {
    if database_id == DEFAULT_FIRESTORE_DATABASE_ID {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "{label} only supports the `(default)` Firestore database, got `{database_id}`"
    )))
}

pub(crate) fn locator_for_document_path(document_path: &DocumentPath) -> Result<DocumentLocator> {
    Ok(DocumentLocator::new(
        storage_table_for_collection_path(document_path.collection_path())?,
        document_path.document_id().clone(),
    ))
}

pub(crate) fn storage_table_for_collection_path(
    collection_path: &CollectionPath,
) -> Result<TableName> {
    let digest = digest(&SHA256, collection_path.to_string().as_bytes());
    TableName::new(format!(
        "firebase_collection_{}",
        URL_SAFE_NO_PAD.encode(digest.as_ref())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_document_path_rejects_empty_segments() {
        assert!(parse_document_path("cities//sf", "firestore path").is_err());
        assert!(parse_document_path("cities/sf/", "firestore path").is_err());
    }

    #[test]
    fn validate_default_database_id_rejects_non_default_database() {
        let error = validate_default_database_id("tenant-a", "firebase-admin/firestore database")
            .expect_err("non-default database id should be rejected");
        assert!(
            error
                .to_string()
                .contains("only supports the `(default)` Firestore database")
        );
    }
}
