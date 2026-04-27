use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mutation::CommitEntry;
use crate::{
    DocumentLocator, Error, NumericValue, ResourcePathBinding, Result, StoredValue, Timestamp,
};

/// Write target identity for protocol-neutral batch operations.
///
/// Native Neovex surfaces can address documents by storage locator alone,
/// while Firestore-style adapters can carry a full resource-path binding so
/// path metadata stays outside user document fields and table-name tricks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WriteKey {
    Locator { locator: DocumentLocator },
    Bound { binding: ResourcePathBinding },
}

impl WriteKey {
    pub fn locator(&self) -> &DocumentLocator {
        match self {
            Self::Locator { locator } => locator,
            Self::Bound { binding } => &binding.locator,
        }
    }

    pub fn resource_path_binding(&self) -> Option<&ResourcePathBinding> {
        match self {
            Self::Locator { .. } => None,
            Self::Bound { binding } => Some(binding),
        }
    }
}

impl From<DocumentLocator> for WriteKey {
    fn from(locator: DocumentLocator) -> Self {
        Self::Locator { locator }
    }
}

impl From<ResourcePathBinding> for WriteKey {
    fn from(binding: ResourcePathBinding) -> Self {
        Self::Bound { binding }
    }
}

/// Conditional requirement for one batch write.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WritePrecondition {
    #[serde(default)]
    pub exists: Option<bool>,
    #[serde(default)]
    pub update_time: Option<Timestamp>,
}

impl WritePrecondition {
    pub fn exists(exists: bool) -> Self {
        Self {
            exists: Some(exists),
            update_time: None,
        }
    }

    pub fn update_time(update_time: Timestamp) -> Self {
        Self {
            exists: None,
            update_time: Some(update_time),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.exists.is_none() && self.update_time.is_none()
    }

    pub fn validate(&self) -> Result<()> {
        if self.exists.is_some() && self.update_time.is_some() {
            return Err(Error::InvalidInput(
                "write precondition cannot set both exists and update_time".to_string(),
            ));
        }
        Ok(())
    }
}

/// Set semantics for a protocol-neutral batch write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteSetMode {
    Create,
    Overwrite,
    MergeAll,
    MergeFields(Vec<String>),
}

/// One requested field transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldTransform {
    pub field: String,
    pub transform: FieldTransformOperation,
}

/// Protocol-neutral transform operations modeled by Firestore writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldTransformOperation {
    ServerTimestamp,
    Increment { operand: NumericValue },
    Maximum { operand: NumericValue },
    Minimum { operand: NumericValue },
    AppendMissingElements { values: Vec<Value> },
    RemoveAllFromArray { values: Vec<Value> },
}

/// One ordered write in an atomic batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AtomicWrite {
    Set {
        key: WriteKey,
        document: serde_json::Map<String, Value>,
        mode: WriteSetMode,
        #[serde(default)]
        precondition: WritePrecondition,
        #[serde(default)]
        transforms: Vec<FieldTransform>,
    },
    Patch {
        key: WriteKey,
        field_patch: serde_json::Map<String, Value>,
        #[serde(default)]
        mask: Vec<String>,
        #[serde(default)]
        precondition: WritePrecondition,
        #[serde(default)]
        transforms: Vec<FieldTransform>,
    },
    Delete {
        key: WriteKey,
        #[serde(default)]
        precondition: WritePrecondition,
        #[serde(default)]
        missing_ok: bool,
    },
    Verify {
        key: WriteKey,
        #[serde(default)]
        precondition: WritePrecondition,
    },
    Transform {
        key: WriteKey,
        transforms: Vec<FieldTransform>,
        #[serde(default)]
        precondition: WritePrecondition,
    },
}

impl AtomicWrite {
    pub fn key(&self) -> &WriteKey {
        match self {
            Self::Set { key, .. }
            | Self::Patch { key, .. }
            | Self::Delete { key, .. }
            | Self::Verify { key, .. }
            | Self::Transform { key, .. } => key,
        }
    }
}

/// Ordered batch request surface shared across adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtomicWriteBatch {
    pub writes: Vec<AtomicWrite>,
}

impl AtomicWriteBatch {
    pub fn new(writes: Vec<AtomicWrite>) -> Result<Self> {
        if writes.is_empty() {
            return Err(Error::InvalidInput(
                "atomic write batch must contain at least one write".to_string(),
            ));
        }
        Ok(Self { writes })
    }
}

/// Result for one ordered write in a committed batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtomicWriteResult {
    #[serde(default)]
    pub update_time: Option<Timestamp>,
    #[serde(default)]
    pub transform_results: Vec<StoredValue>,
}

/// Commit outcome for an ordered atomic write batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtomicWriteBatchOutcome {
    pub commit: Option<CommitEntry>,
    pub commit_time: Timestamp,
    pub write_results: Vec<AtomicWriteResult>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AtomicWrite, AtomicWriteBatch, FieldTransform, FieldTransformOperation, WriteKey,
        WritePrecondition, WriteSetMode,
    };
    use crate::{DocumentId, DocumentLocator, DocumentPath, ResourcePathBinding, TableName};

    #[test]
    fn write_precondition_rejects_conflicting_constraints() {
        let error = WritePrecondition {
            exists: Some(true),
            update_time: Some(crate::Timestamp(7)),
        }
        .validate()
        .expect_err("conflicting preconditions should fail");

        assert!(matches!(error, crate::Error::InvalidInput(_)));
    }

    #[test]
    fn write_key_preserves_bound_resource_path_identity() {
        let key = WriteKey::from(ResourcePathBinding::new(
            DocumentLocator::new(
                TableName::new("cities_store").expect("table should parse"),
                DocumentId::from_key("internal-sf").expect("id should parse"),
            ),
            DocumentPath::from_segments(["cities", "SF"]).expect("path should parse"),
        ));

        assert_eq!(key.locator().table.as_str(), "cities_store");
        assert_eq!(
            key.resource_path_binding()
                .expect("bound key should keep the path binding")
                .document_path
                .to_string(),
            "cities/SF"
        );
    }

    #[test]
    fn atomic_write_batch_requires_at_least_one_write() {
        let error = AtomicWriteBatch::new(Vec::new()).expect_err("empty batch should fail");

        assert!(matches!(error, crate::Error::InvalidInput(_)));
    }

    #[test]
    fn atomic_write_serializes_set_patch_and_transform_shapes() {
        let key = WriteKey::from(DocumentLocator::new(
            TableName::new("messages").expect("table should parse"),
            DocumentId::from_key("hello.world").expect("id should parse"),
        ));
        let write = AtomicWrite::Set {
            key: key.clone(),
            document: serde_json::Map::from_iter([("body".to_string(), json!("hi"))]),
            mode: WriteSetMode::MergeFields(vec!["body".to_string()]),
            precondition: WritePrecondition::exists(true),
            transforms: vec![FieldTransform {
                field: "updatedAt".to_string(),
                transform: FieldTransformOperation::ServerTimestamp,
            }],
        };
        let encoded = serde_json::to_string(&write).expect("write should serialize");
        let decoded: AtomicWrite = serde_json::from_str(&encoded).expect("write should parse");

        assert_eq!(decoded.key(), &key);
        assert!(matches!(
            decoded,
            AtomicWrite::Set {
                transforms,
                precondition,
                ..
            } if transforms.len() == 1 && precondition == WritePrecondition::exists(true)
        ));
    }
}
