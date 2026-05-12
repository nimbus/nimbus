use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use crate::{DocumentId, Error, Result, TableName};

/// Raw collection/path segment text.
///
/// This is intentionally distinct from [`TableName`]. `TableName` remains the
/// logical storage/schema identifier used by existing Nimbus surfaces, while
/// raw protocol path segments live here so Firestore-style collection IDs do
/// not have to satisfy the `TableName` validation contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CollectionName(String);

impl CollectionName {
    /// Creates a validated raw collection segment.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }

    /// Returns the collection segment as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for CollectionName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for CollectionName {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for CollectionName {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        validate_path_segment(&value, "collection name")?;
        Ok(Self(value))
    }
}

impl From<CollectionName> for String {
    fn from(value: CollectionName) -> Self {
        value.0
    }
}

/// One ancestor hop inside a nested collection path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CollectionPathSegment {
    pub document_id: DocumentId,
    pub collection: CollectionName,
}

/// Full collection ancestry for a document path, ending at the leaf
/// collection group and retaining every ancestor document id explicitly.
///
/// For `a/1/b/2/c/3`, the collection path is `a/1/b/2/c` and the leaf
/// document id is `3`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CollectionPath {
    root: CollectionName,
    #[serde(default)]
    descendants: Vec<CollectionPathSegment>,
}

impl CollectionPath {
    pub fn root(root: CollectionName) -> Self {
        Self {
            root,
            descendants: Vec::new(),
        }
    }

    pub fn new(root: CollectionName, descendants: Vec<CollectionPathSegment>) -> Self {
        Self { root, descendants }
    }

    pub fn from_segments<I, S>(segments: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let segments = segments
            .into_iter()
            .map(|segment| segment.as_ref().to_string())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            return Err(Error::InvalidInput(
                "collection path must include at least one collection segment".to_string(),
            ));
        }
        if segments.len() % 2 == 0 {
            return Err(Error::InvalidInput(
                "collection path must end with a collection segment".to_string(),
            ));
        }

        let root = CollectionName::new(segments[0].clone())?;
        let mut descendants = Vec::with_capacity((segments.len() - 1) / 2);
        for pair in segments[1..].chunks_exact(2) {
            descendants.push(CollectionPathSegment {
                document_id: DocumentId::from_key(pair[0].clone())?,
                collection: CollectionName::new(pair[1].clone())?,
            });
        }

        Ok(Self { root, descendants })
    }

    pub fn root_collection(&self) -> &CollectionName {
        &self.root
    }

    pub fn descendants(&self) -> &[CollectionPathSegment] {
        &self.descendants
    }

    pub fn collection_group(&self) -> &CollectionName {
        self.descendants
            .last()
            .map(|segment| &segment.collection)
            .unwrap_or(&self.root)
    }

    pub fn parent_document_path(&self) -> Option<DocumentPath> {
        let (parent, rest) = self.descendants.split_last()?;
        Some(DocumentPath {
            collection_path: Self {
                root: self.root.clone(),
                descendants: rest.to_vec(),
            },
            document_id: parent.document_id.clone(),
        })
    }

    pub fn segments(&self) -> Vec<String> {
        let mut segments = Vec::with_capacity(1 + self.descendants.len() * 2);
        segments.push(self.root.to_string());
        for segment in &self.descendants {
            segments.push(segment.document_id.to_string());
            segments.push(segment.collection.to_string());
        }
        segments
    }
}

impl Display for CollectionPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.segments().join("/"))
    }
}

/// Full document path with collection ancestry plus the leaf document id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentPath {
    collection_path: CollectionPath,
    document_id: DocumentId,
}

impl DocumentPath {
    pub fn new(collection_path: CollectionPath, document_id: DocumentId) -> Self {
        Self {
            collection_path,
            document_id,
        }
    }

    pub fn from_segments<I, S>(segments: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let segments = segments
            .into_iter()
            .map(|segment| segment.as_ref().to_string())
            .collect::<Vec<_>>();
        if segments.len() < 2 {
            return Err(Error::InvalidInput(
                "document path must include at least one collection and one document id"
                    .to_string(),
            ));
        }
        if segments.len() % 2 != 0 {
            return Err(Error::InvalidInput(
                "document path must end with a document id".to_string(),
            ));
        }

        let document_id = DocumentId::from_key(
            segments
                .last()
                .expect("document path should have a trailing document id")
                .clone(),
        )?;
        let collection_path = CollectionPath::from_segments(
            segments[..segments.len() - 1]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;

        Ok(Self {
            collection_path,
            document_id,
        })
    }

    pub fn collection_path(&self) -> &CollectionPath {
        &self.collection_path
    }

    pub fn document_id(&self) -> &DocumentId {
        &self.document_id
    }

    pub fn collection_group(&self) -> &CollectionName {
        self.collection_path.collection_group()
    }

    pub fn parent_document_path(&self) -> Option<DocumentPath> {
        self.collection_path.parent_document_path()
    }

    /// Returns the immediate child collection that this document belongs to
    /// beneath the provided ancestor document, or the root collection when no
    /// ancestor is provided.
    pub fn direct_child_collection_for_ancestor(
        &self,
        ancestor: Option<&DocumentPath>,
    ) -> Option<&CollectionName> {
        match ancestor {
            None => Some(self.collection_path.root_collection()),
            Some(ancestor) => {
                if self.collection_path.root_collection()
                    != ancestor.collection_path.root_collection()
                {
                    return None;
                }

                let ancestor_descendants = ancestor.collection_path.descendants();
                let descendant_segment = self
                    .collection_path
                    .descendants()
                    .get(ancestor_descendants.len())?;
                if descendant_segment.document_id != ancestor.document_id {
                    return None;
                }
                if self
                    .collection_path
                    .descendants()
                    .get(..ancestor_descendants.len())
                    != Some(ancestor_descendants)
                {
                    return None;
                }
                Some(&descendant_segment.collection)
            }
        }
    }

    pub fn segments(&self) -> Vec<String> {
        let mut segments = self.collection_path.segments();
        segments.push(self.document_id.to_string());
        segments
    }
}

impl Display for DocumentPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.segments().join("/"))
    }
}

/// Firestore-style document trigger pattern.
///
/// Patterns alternate collection and document segments exactly like
/// [`DocumentPath`], but each segment may be either a literal or a wildcard
/// capture such as `{userId}`. Patterns must always terminate at a document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentTriggerPattern {
    segments: Vec<TriggerPathPatternSegment>,
}

impl DocumentTriggerPattern {
    pub fn from_segments<I, S>(segments: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let segments = segments
            .into_iter()
            .map(|segment| segment.as_ref().to_string())
            .collect::<Vec<_>>();
        if segments.len() < 2 {
            return Err(Error::InvalidInput(
                "document trigger pattern must include at least one collection and one document segment"
                    .to_string(),
            ));
        }
        if segments.len() % 2 != 0 {
            return Err(Error::InvalidInput(
                "document trigger pattern must end with a document segment".to_string(),
            ));
        }

        let mut wildcard_names = BTreeSet::new();
        let mut parsed = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            if let Some(name) = parse_wildcard_name(segment) {
                validate_wildcard_name(name)?;
                if !wildcard_names.insert(name.to_string()) {
                    return Err(Error::InvalidInput(format!(
                        "document trigger pattern cannot reuse wildcard `{name}`"
                    )));
                }
                parsed.push(TriggerPathPatternSegment::Wildcard(name.to_string()));
                continue;
            }

            if index % 2 == 0 {
                CollectionName::new(segment.clone())?;
            } else {
                DocumentId::from_key(segment.clone())?;
            }
            parsed.push(TriggerPathPatternSegment::Literal(segment.clone()));
        }

        Ok(Self { segments: parsed })
    }

    pub fn matches(&self, document_path: &DocumentPath) -> Option<DocumentTriggerMatch> {
        let path_segments = document_path.segments();
        if self.segments.len() != path_segments.len() {
            return None;
        }

        let mut params = BTreeMap::new();
        for (pattern, actual) in self.segments.iter().zip(path_segments.iter()) {
            match pattern {
                TriggerPathPatternSegment::Literal(value) if value == actual => {}
                TriggerPathPatternSegment::Literal(_) => return None,
                TriggerPathPatternSegment::Wildcard(name) => {
                    params.insert(name.clone(), actual.clone());
                }
            }
        }

        Some(DocumentTriggerMatch { params })
    }

    pub fn is_match(&self, document_path: &DocumentPath) -> bool {
        self.matches(document_path).is_some()
    }

    pub fn segments(&self) -> Vec<String> {
        self.segments
            .iter()
            .map(TriggerPathPatternSegment::render)
            .collect()
    }
}

impl Display for DocumentTriggerPattern {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.segments().join("/"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum TriggerPathPatternSegment {
    Literal(String),
    Wildcard(String),
}

impl TriggerPathPatternSegment {
    fn render(&self) -> String {
        match self {
            Self::Literal(value) => value.clone(),
            Self::Wildcard(name) => format!("{{{name}}}"),
        }
    }
}

/// Deterministic wildcard captures from a matching document trigger pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentTriggerMatch {
    params: BTreeMap<String, String>,
}

impl DocumentTriggerMatch {
    pub fn params(&self) -> &BTreeMap<String, String> {
        &self.params
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn into_params(self) -> BTreeMap<String, String> {
        self.params
    }
}

/// Existing storage lookup identity for one document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentLocator {
    pub table: TableName,
    pub id: DocumentId,
}

impl DocumentLocator {
    pub fn new(table: TableName, id: DocumentId) -> Self {
        Self { table, id }
    }
}

/// Full resource identity bound to a storage locator.
///
/// The raw collection/path model stays protocol-neutral and separate from the
/// existing `TableName` contract. Storage layers can choose any safe internal
/// locator strategy they need, while adapters keep lossless Firestore-style
/// ancestry here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourcePathBinding {
    pub locator: DocumentLocator,
    pub document_path: DocumentPath,
}

impl ResourcePathBinding {
    pub fn new(locator: DocumentLocator, document_path: DocumentPath) -> Self {
        Self {
            locator,
            document_path,
        }
    }

    pub fn collection_group(&self) -> &CollectionName {
        self.document_path.collection_group()
    }

    pub fn collection_path(&self) -> &CollectionPath {
        self.document_path.collection_path()
    }
}

fn validate_path_segment(value: &str, kind: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{kind} cannot be empty")));
    }
    if value.len() > 1_500 {
        return Err(Error::InvalidInput(format!(
            "{kind} cannot exceed 1500 bytes"
        )));
    }
    if value.contains('/') {
        return Err(Error::InvalidInput(format!("{kind} cannot contain `/`")));
    }
    if value.bytes().any(|byte| byte == 0) {
        return Err(Error::InvalidInput(format!(
            "{kind} cannot contain NUL bytes"
        )));
    }

    Ok(())
}

fn parse_wildcard_name(segment: &str) -> Option<&str> {
    segment.strip_prefix('{')?.strip_suffix('}')
}

fn validate_wildcard_name(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidInput(
            "trigger wildcard name cannot be empty".to_string(),
        ));
    }
    validate_path_segment(value, "trigger wildcard name")?;
    if value.contains('{') || value.contains('}') {
        return Err(Error::InvalidInput(
            "trigger wildcard name cannot contain `{` or `}`".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CollectionName, CollectionPath, DocumentLocator, DocumentPath, DocumentTriggerPattern,
        ResourcePathBinding,
    };
    use crate::{DocumentId, TableName};
    use std::collections::BTreeMap;

    #[test]
    fn collection_name_accepts_firestore_style_segments() {
        assert_eq!(
            CollectionName::new("cities.v2")
                .expect("dotted collection should parse")
                .to_string(),
            "cities.v2"
        );
        assert_eq!(
            CollectionName::new("__stats__")
                .expect("reserved-looking collection should parse")
                .to_string(),
            "__stats__"
        );
        assert_eq!(
            CollectionName::new("日本語")
                .expect("unicode collection should parse")
                .to_string(),
            "日本語"
        );
    }

    #[test]
    fn document_path_roundtrips_root_collection() {
        let path = DocumentPath::from_segments(["cities", "SF"]).expect("path should parse");

        assert_eq!(path.collection_group().as_str(), "cities");
        assert_eq!(path.document_id().as_str(), "SF");
        assert_eq!(path.to_string(), "cities/SF");
        assert!(path.parent_document_path().is_none());
    }

    #[test]
    fn document_path_roundtrips_nested_subcollections() {
        let path =
            DocumentPath::from_segments(["a", "1", "b", "2", "c", "3"]).expect("path should parse");

        assert_eq!(path.collection_group().as_str(), "c");
        assert_eq!(path.document_id().as_str(), "3");
        assert_eq!(path.collection_path().to_string(), "a/1/b/2/c");
        assert_eq!(
            path.parent_document_path()
                .expect("nested path should have a parent")
                .to_string(),
            "a/1/b/2"
        );
    }

    #[test]
    fn direct_child_collection_for_ancestor_supports_root_and_nested_documents() {
        let root = DocumentPath::from_segments(["cities", "SF"]).expect("path should parse");
        let nested = DocumentPath::from_segments(["cities", "SF", "landmarks", "GG"])
            .expect("path should parse");
        let deeply_nested =
            DocumentPath::from_segments(["cities", "SF", "landmarks", "GG", "photos", "p1"])
                .expect("path should parse");

        assert_eq!(
            root.direct_child_collection_for_ancestor(None)
                .expect("root parent should return the root collection")
                .as_str(),
            "cities"
        );
        assert_eq!(
            nested
                .direct_child_collection_for_ancestor(Some(&root))
                .expect("nested document should return its direct child collection")
                .as_str(),
            "landmarks"
        );
        assert_eq!(
            deeply_nested
                .direct_child_collection_for_ancestor(Some(&nested))
                .expect("deeper descendant should return the next child collection")
                .as_str(),
            "photos"
        );
        assert!(
            root.direct_child_collection_for_ancestor(Some(&nested))
                .is_none(),
            "ancestor checks should reject unrelated or shallower paths"
        );
    }

    #[test]
    fn collection_path_requires_collection_terminated_segments() {
        let error = CollectionPath::from_segments(["cities", "SF"])
            .expect_err("collection path cannot end with a document id");
        assert!(matches!(error, crate::Error::InvalidInput(_)));
    }

    #[test]
    fn resource_path_binding_preserves_unicode_document_ids() {
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(
                TableName::new("landmarks_store").expect("table name should parse"),
                DocumentId::from_key("internal-1").expect("internal id should parse"),
            ),
            DocumentPath::from_segments(["日本語", "東京", "城", "大阪城"])
                .expect("unicode path should parse"),
        );

        assert_eq!(binding.collection_group().as_str(), "城");
        assert_eq!(binding.document_path.document_id().as_str(), "大阪城");
        assert_eq!(binding.document_path.to_string(), "日本語/東京/城/大阪城");
    }

    #[test]
    fn document_trigger_pattern_matches_exact_paths_without_params() {
        let pattern = DocumentTriggerPattern::from_segments(["cities", "SF"])
            .expect("exact trigger pattern should parse");
        let path = DocumentPath::from_segments(["cities", "SF"]).expect("path should parse");

        let matched = pattern.matches(&path).expect("exact path should match");

        assert!(matched.params().is_empty());
        assert!(pattern.is_match(&path));
        assert_eq!(pattern.to_string(), "cities/SF");
    }

    #[test]
    fn document_trigger_pattern_captures_nested_wildcards_deterministically() {
        let pattern = DocumentTriggerPattern::from_segments([
            "users",
            "{userId}",
            "{messageCollectionId}",
            "{messageId}",
        ])
        .expect("wildcard trigger pattern should parse");
        let path = DocumentPath::from_segments(["users", "alice", "messages", "hello"])
            .expect("path should parse");

        let matched = pattern.matches(&path).expect("wildcard path should match");

        assert_eq!(matched.param("userId"), Some("alice"));
        assert_eq!(matched.param("messageCollectionId"), Some("messages"));
        assert_eq!(matched.param("messageId"), Some("hello"));
        assert_eq!(
            matched.params(),
            &BTreeMap::from([
                ("messageCollectionId".to_string(), "messages".to_string()),
                ("messageId".to_string(), "hello".to_string()),
                ("userId".to_string(), "alice".to_string()),
            ])
        );
    }

    #[test]
    fn document_trigger_pattern_rejects_collection_terminal_shapes() {
        let root_only = DocumentTriggerPattern::from_segments(["users"])
            .expect_err("collection-only trigger pattern should fail");
        let nested_collection_terminal =
            DocumentTriggerPattern::from_segments(["users", "{userId}", "messages"])
                .expect_err("collection-terminal trigger pattern should fail");

        assert!(matches!(root_only, crate::Error::InvalidInput(_)));
        assert!(matches!(
            nested_collection_terminal,
            crate::Error::InvalidInput(_)
        ));
    }

    #[test]
    fn document_trigger_pattern_rejects_duplicate_wildcards() {
        let error = DocumentTriggerPattern::from_segments(["users", "{id}", "messages", "{id}"])
            .expect_err("duplicate wildcard names should fail");

        assert!(matches!(error, crate::Error::InvalidInput(_)));
    }
}
