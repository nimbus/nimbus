use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use ulid::Ulid;

use crate::{Error, Result};

/// Unique identifier for a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantId(String);

impl TenantId {
    /// Creates a new tenant id wrapper.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }

    /// Returns the tenant id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for TenantId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TenantId {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for TenantId {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        validate_logical_name(&value, "tenant id")?;
        Ok(Self(value))
    }
}

impl From<TenantId> for String {
    fn from(value: TenantId) -> Self {
        value.0
    }
}

/// Unique identifier for a logical table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TableName(String);

impl TableName {
    /// Creates a new table name wrapper.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }

    /// Returns the table name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for TableName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TableName {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for TableName {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        validate_logical_name(&value, "table name")?;
        Ok(Self(value))
    }
}

impl From<TableName> for String {
    fn from(value: TableName) -> Self {
        value.0
    }
}

/// Stable identifier for a logical table instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TableId(String);

impl TableId {
    /// Generates a new stable table identifier.
    pub fn new() -> Self {
        Self(Ulid::new().to_string())
    }

    /// Returns the table id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TableId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for TableId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TableId {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new_from_string(s.to_string())
    }
}

impl TryFrom<String> for TableId {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::new_from_string(value)
    }
}

impl From<TableId> for String {
    fn from(value: TableId) -> Self {
        value.0
    }
}

impl TableId {
    fn new_from_string(value: String) -> Result<Self> {
        validate_logical_name(&value, "table id")?;
        Ok(Self(value))
    }
}

/// Stable identifier for a logical index instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IndexId(String);

impl IndexId {
    /// Generates a new stable index identifier.
    pub fn new() -> Self {
        Self(Ulid::new().to_string())
    }

    /// Returns the index id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for IndexId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for IndexId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for IndexId {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new_from_string(s.to_string())
    }
}

impl TryFrom<String> for IndexId {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::new_from_string(value)
    }
}

impl From<IndexId> for String {
    fn from(value: IndexId) -> Self {
        value.0
    }
}

impl IndexId {
    fn new_from_string(value: String) -> Result<Self> {
        validate_logical_name(&value, "index id")?;
        Ok(Self(value))
    }
}

/// Lifecycle state for a logical table identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableState {
    Active,
    Hidden,
    Deleting,
}

impl TableState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Hidden => "hidden",
            Self::Deleting => "deleting",
        }
    }
}

impl Default for TableState {
    fn default() -> Self {
        Self::Active
    }
}

impl Display for TableState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TableState {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "hidden" => Ok(Self::Hidden),
            "deleting" => Ok(Self::Deleting),
            _ => Err(Error::InvalidInput(format!(
                "unknown table lifecycle state: {s}"
            ))),
        }
    }
}

/// Protocol-neutral document identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DocumentId(String);

impl DocumentId {
    /// Generates a new document identifier.
    pub fn new() -> Self {
        Self(Ulid::new().to_string())
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Creates a document identifier from a caller-provided key.
    pub fn from_key(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }
}

impl Default for DocumentId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for DocumentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for DocumentId {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::from_key(s)
    }
}

impl TryFrom<String> for DocumentId {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        validate_document_key(&value)?;
        Ok(Self(value))
    }
}

impl From<DocumentId> for String {
    fn from(value: DocumentId) -> Self {
        value.0
    }
}

/// A document id resolved against the table context claimed by a protocol.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResolvedDocumentId {
    table: TableName,
    document_id: DocumentId,
}

impl ResolvedDocumentId {
    const TABLE_SCOPED_SEPARATOR: char = ':';

    pub fn new(table: TableName, document_id: DocumentId) -> Self {
        Self { table, document_id }
    }

    pub fn table(&self) -> &TableName {
        &self.table
    }

    pub fn document_id(&self) -> &DocumentId {
        &self.document_id
    }

    pub fn into_document_id(self) -> DocumentId {
        self.document_id
    }

    /// Encodes a protocol-facing id that carries its developer-visible table.
    ///
    /// This is intentionally separate from storage layout: backends still store
    /// raw `DocumentId` values keyed by durable `TableId`.
    pub fn encode_table_scoped(table: &TableName, document_id: &DocumentId) -> Result<DocumentId> {
        DocumentId::from_key(format!(
            "{}{}{}",
            table.as_str(),
            Self::TABLE_SCOPED_SEPARATOR,
            document_id.as_str()
        ))
    }

    pub fn resolve_table_scoped(
        expected_table: &TableName,
        document_id: DocumentId,
    ) -> Result<Self> {
        let Some((encoded_table, raw_document_id)) = document_id
            .as_str()
            .split_once(Self::TABLE_SCOPED_SEPARATOR)
        else {
            return Err(Error::InvalidInput(format!(
                "document id for table {} must be table-scoped",
                expected_table
            )));
        };
        let encoded_table = TableName::new(encoded_table.to_string())?;
        if &encoded_table != expected_table {
            return Err(Error::InvalidInput(format!(
                "document id belongs to table {}, not {}",
                encoded_table, expected_table
            )));
        }
        Ok(Self {
            table: encoded_table,
            document_id: DocumentId::from_key(raw_document_id.to_string())?,
        })
    }
}

/// Commit log sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct SequenceNumber(pub u64);

impl Display for SequenceNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Milliseconds since Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Returns the current wall-clock timestamp in milliseconds since epoch.
    pub fn now() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch");
        Self(duration.as_millis() as u64)
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub(crate) fn validate_logical_name(value: &str, kind: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{kind} cannot be empty")));
    }
    if value.len() > 128 {
        return Err(Error::InvalidInput(format!(
            "{kind} cannot exceed 128 characters"
        )));
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Ok(());
    }

    Err(Error::InvalidInput(format!(
        "{kind} may only contain ASCII letters, numbers, `_`, and `-`"
    )))
}

pub(crate) fn validate_document_key(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(
            "document key cannot be empty".to_string(),
        ));
    }
    if value.len() > 1_500 {
        return Err(Error::InvalidInput(
            "document key cannot exceed 1500 bytes".to_string(),
        ));
    }
    if value.contains('/') {
        return Err(Error::InvalidInput(
            "document key cannot contain `/`".to_string(),
        ));
    }
    if value.bytes().any(|byte| byte == 0) {
        return Err(Error::InvalidInput(
            "document key cannot contain NUL bytes".to_string(),
        ));
    }

    Ok(())
}
