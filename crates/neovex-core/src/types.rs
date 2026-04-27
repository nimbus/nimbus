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
