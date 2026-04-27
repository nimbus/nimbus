use neovex_core::{CollectionName, CollectionPath, DocumentPath};
use thiserror::Error;

const PROJECTS_LITERAL: &str = "projects";
const DATABASES_LITERAL: &str = "databases";
const DOCUMENTS_LITERAL: &str = "documents";
const DEFAULT_DATABASE_ID: &str = "(default)";

/// Firestore resource-name parsing stays adapter-local because the wire shapes
/// are protocol contracts, but the parsed path outputs use the shared
/// `neovex-core` raw path primitives from `F0.2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FirestoreDatabaseName {
    pub project_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FirestoreDocumentName {
    pub database: FirestoreDatabaseName,
    pub document_path: DocumentPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FirestoreParentName {
    pub database: FirestoreDatabaseName,
    pub parent_document_path: Option<DocumentPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FirestoreCollectionTarget {
    pub database: FirestoreDatabaseName,
    pub parent_document_path: Option<DocumentPath>,
    pub collection_path: CollectionPath,
    pub collection_group: CollectionName,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreResourceNameError {
    #[error("invalid Firestore {kind}: {reason}")]
    InvalidResource { kind: &'static str, reason: String },
    #[error("only the `(default)` Firestore database is supported, got `{0}`")]
    UnsupportedDatabase(String),
}

pub(crate) fn parse_database_name(
    resource_name: &str,
) -> Result<FirestoreDatabaseName, FirestoreResourceNameError> {
    let segments = split_resource_name(resource_name, "database resource name")?;
    parse_database_segments(&segments, "database resource name")
}

pub(crate) fn parse_document_name(
    resource_name: &str,
) -> Result<FirestoreDocumentName, FirestoreResourceNameError> {
    let segments = split_resource_name(resource_name, "document resource name")?;
    if segments.len() < 6 {
        return Err(invalid_resource(
            "document resource name",
            "must include `projects/{project}/databases/{database}/documents/{document_path}`",
        ));
    }
    if segments[4] != DOCUMENTS_LITERAL {
        return Err(invalid_resource(
            "document resource name",
            "must include the `documents` collection root",
        ));
    }

    let database = parse_database_segments(&segments[..4], "document resource name")?;
    let document_path = parse_raw_document_path_segments(&segments[5..], "document resource name")?;

    Ok(FirestoreDocumentName {
        database,
        document_path,
    })
}

pub(crate) fn parse_parent_name(
    resource_name: &str,
) -> Result<FirestoreParentName, FirestoreResourceNameError> {
    let segments = split_resource_name(resource_name, "parent resource name")?;
    if segments.len() < 5 {
        return Err(invalid_resource(
            "parent resource name",
            "must include `projects/{project}/databases/{database}/documents`",
        ));
    }
    if segments[4] != DOCUMENTS_LITERAL {
        return Err(invalid_resource(
            "parent resource name",
            "must include the `documents` collection root",
        ));
    }

    let database = parse_database_segments(&segments[..4], "parent resource name")?;
    let parent_document_path = if segments.len() == 5 {
        None
    } else {
        Some(parse_raw_document_path_segments(
            &segments[5..],
            "parent resource name",
        )?)
    };

    Ok(FirestoreParentName {
        database,
        parent_document_path,
    })
}

pub(crate) fn parse_collection_target(
    parent_resource_name: &str,
    collection_id: &str,
) -> Result<FirestoreCollectionTarget, FirestoreResourceNameError> {
    let parent = parse_parent_name(parent_resource_name)?;
    let collection_group =
        CollectionName::new(collection_id.to_string()).map_err(invalid_neovex_path)?;
    let collection_path = match &parent.parent_document_path {
        Some(parent_document_path) => {
            let mut segments = parent_document_path.segments();
            segments.push(collection_group.to_string());
            CollectionPath::from_segments(segments.iter().map(String::as_str))
                .map_err(invalid_neovex_path)?
        }
        None => CollectionPath::root(collection_group.clone()),
    };

    Ok(FirestoreCollectionTarget {
        database: parent.database,
        parent_document_path: parent.parent_document_path,
        collection_path,
        collection_group,
    })
}

pub(crate) fn decode_rest_database(
    project_id: &str,
    database_id: &str,
) -> Result<FirestoreDatabaseName, FirestoreResourceNameError> {
    let decoded_project_id = decode_percent_segment(project_id, "REST project id segment")?;
    let decoded_database_id = decode_percent_segment(database_id, "REST database id segment")?;
    validate_database_id(&decoded_database_id)?;

    Ok(FirestoreDatabaseName {
        project_id: decoded_project_id,
    })
}

pub(crate) fn decode_rest_document_path(
    path: &str,
) -> Result<DocumentPath, FirestoreResourceNameError> {
    let segments = split_resource_name(path, "REST document path")?;
    let decoded_segments = segments
        .iter()
        .map(|segment| decode_percent_segment(segment, "REST document path segment"))
        .collect::<Result<Vec<_>, _>>()?;
    DocumentPath::from_segments(decoded_segments.iter().map(String::as_str))
        .map_err(invalid_neovex_path)
}

fn parse_database_segments(
    segments: &[&str],
    kind: &'static str,
) -> Result<FirestoreDatabaseName, FirestoreResourceNameError> {
    if segments.len() != 4 {
        return Err(invalid_resource(
            kind,
            "must include `projects/{project}/databases/{database}`",
        ));
    }
    if segments[0] != PROJECTS_LITERAL {
        return Err(invalid_resource(kind, "must start with `projects`"));
    }
    if segments[2] != DATABASES_LITERAL {
        return Err(invalid_resource(kind, "must include `databases`"));
    }

    let project_id = validate_identifier_segment(segments[1], "project id segment")?;
    validate_database_id(segments[3])?;

    Ok(FirestoreDatabaseName { project_id })
}

fn parse_raw_document_path_segments(
    segments: &[&str],
    kind: &'static str,
) -> Result<DocumentPath, FirestoreResourceNameError> {
    DocumentPath::from_segments(segments.iter().copied())
        .map_err(|error| invalid_resource(kind, format!("invalid document path segments: {error}")))
}

fn split_resource_name<'a>(
    resource_name: &'a str,
    kind: &'static str,
) -> Result<Vec<&'a str>, FirestoreResourceNameError> {
    if resource_name.is_empty() {
        return Err(invalid_resource(kind, "cannot be empty"));
    }
    let segments = resource_name.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(invalid_resource(
            kind,
            "must not contain empty segments, double slashes, or trailing slashes",
        ));
    }
    Ok(segments)
}

fn validate_identifier_segment(
    value: &str,
    kind: &'static str,
) -> Result<String, FirestoreResourceNameError> {
    if value.is_empty() {
        return Err(invalid_resource(kind, "cannot be empty"));
    }
    if value.contains('/') {
        return Err(invalid_resource(kind, "cannot contain `/`"));
    }
    if value.bytes().any(|byte| byte == 0) {
        return Err(invalid_resource(kind, "cannot contain NUL bytes"));
    }
    Ok(value.to_string())
}

fn validate_database_id(database_id: &str) -> Result<(), FirestoreResourceNameError> {
    validate_identifier_segment(database_id, "database id segment")?;
    if database_id == DEFAULT_DATABASE_ID {
        Ok(())
    } else {
        Err(FirestoreResourceNameError::UnsupportedDatabase(
            database_id.to_string(),
        ))
    }
}

fn decode_percent_segment(
    value: &str,
    kind: &'static str,
) -> Result<String, FirestoreResourceNameError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(invalid_resource(
                    kind,
                    "contains an incomplete percent escape",
                ));
            }
            decoded.push(
                (decode_hex(bytes[index + 1], kind)? << 4) | decode_hex(bytes[index + 2], kind)?,
            );
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    let decoded =
        String::from_utf8(decoded).map_err(|_| invalid_resource(kind, "contains invalid UTF-8"))?;
    validate_identifier_segment(&decoded, kind)
}

fn decode_hex(value: u8, kind: &'static str) -> Result<u8, FirestoreResourceNameError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(invalid_resource(kind, "contains a non-hex percent escape")),
    }
}

fn invalid_resource(kind: &'static str, reason: impl Into<String>) -> FirestoreResourceNameError {
    FirestoreResourceNameError::InvalidResource {
        kind,
        reason: reason.into(),
    }
}

fn invalid_neovex_path(error: neovex_core::Error) -> FirestoreResourceNameError {
    invalid_resource("path", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_database_name_accepts_default_database() {
        let database = parse_database_name("projects/demo/databases/(default)")
            .expect("default database should parse");

        assert_eq!(database.project_id, "demo");
    }

    #[test]
    fn parse_database_name_rejects_named_databases() {
        let error = parse_database_name("projects/demo/databases/custom")
            .expect_err("named database should be rejected");

        assert!(matches!(
            error,
            FirestoreResourceNameError::UnsupportedDatabase(database_id)
                if database_id == "custom"
        ));
    }

    #[test]
    fn parse_document_name_preserves_nested_paths_and_collection_group() {
        let document =
            parse_document_name("projects/demo/databases/(default)/documents/a/1/b/2/c/3")
                .expect("nested document resource should parse");

        assert_eq!(document.database.project_id, "demo");
        assert_eq!(document.document_path.to_string(), "a/1/b/2/c/3");
        assert_eq!(
            document.document_path.collection_path().to_string(),
            "a/1/b/2/c"
        );
        assert_eq!(document.document_path.collection_group().as_str(), "c");
    }

    #[test]
    fn parse_document_name_accepts_dotted_and_unicode_segments() {
        let document = parse_document_name(
            "projects/demo/databases/(default)/documents/日本語/東京/cities.v2/SF__1",
        )
        .expect("unicode and dotted document resource should parse");

        assert_eq!(
            document.document_path.to_string(),
            "日本語/東京/cities.v2/SF__1"
        );
    }

    #[test]
    fn parse_parent_name_accepts_root_and_document_parents() {
        let root_parent = parse_parent_name("projects/demo/databases/(default)/documents")
            .expect("database root parent should parse");
        assert!(root_parent.parent_document_path.is_none());

        let document_parent =
            parse_parent_name("projects/demo/databases/(default)/documents/cities/SF")
                .expect("document parent should parse");
        assert_eq!(
            document_parent
                .parent_document_path
                .expect("document parent should be present")
                .to_string(),
            "cities/SF"
        );
    }

    #[test]
    fn parse_collection_target_preserves_collection_path_and_group() {
        let target = parse_collection_target(
            "projects/demo/databases/(default)/documents/cities/SF",
            "__landmarks.v2__",
        )
        .expect("collection target should parse");

        assert_eq!(target.database.project_id, "demo");
        assert_eq!(target.collection_group.as_str(), "__landmarks.v2__");
        assert_eq!(
            target.collection_path.to_string(),
            "cities/SF/__landmarks.v2__"
        );
    }

    #[test]
    fn decode_rest_path_segments_supports_url_escaped_unicode() {
        let path = decode_rest_document_path("%E6%97%A5%E6%9C%AC%E8%AA%9E/%E6%9D%B1%E4%BA%AC")
            .expect("encoded unicode path should parse");

        assert_eq!(path.to_string(), "日本語/東京");
    }

    #[test]
    fn decode_rest_segments_rejects_trailing_slashes_and_encoded_slashes() {
        assert!(decode_rest_document_path("cities/SF/").is_err());
        assert!(decode_rest_document_path("cities/SF%2Fbay").is_err());
    }

    #[test]
    fn parse_resource_names_rejects_malformed_shapes() {
        assert!(parse_document_name("projects/demo/databases/(default)").is_err());
        assert!(parse_parent_name("projects/demo/databases/(default)/documents/cities").is_err());
        assert!(
            parse_document_name("projects/demo/databases/(default)/missing/cities/SF").is_err()
        );
    }
}
