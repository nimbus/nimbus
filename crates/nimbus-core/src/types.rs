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

/// Opaque identifier for a workload that owns an egress enforcement point (PEP).
///
/// The node-scoped `EgressEngine` (in `nimbus-proxy`) keys its per-workload PEP
/// registry on this id. It lives in `nimbus-core` on purpose: the engine must
/// reference only `nimbus-core`/`nimbus-proxy` types — never
/// `nimbus-sandbox::SandboxId` — so `nimbus-proxy` never gains a dependency on
/// `nimbus-sandbox` (the cycle the egress-engine plan forbids). The sandbox
/// layer builds a `WorkloadId` from its `SandboxId`.
///
/// Deliberately permissive — any non-empty string — because `SandboxId` imposes
/// no character or length constraints, so a stricter rule (like
/// `validate_logical_name`) could reject a valid id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkloadId(String);

impl WorkloadId {
    /// Creates a new workload id wrapper. Rejects only the empty string.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }

    /// Returns the workload id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for WorkloadId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for WorkloadId {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for WorkloadId {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(Error::InvalidInput(
                "workload id cannot be empty".to_string(),
            ));
        }
        Ok(Self(value))
    }
}

impl From<WorkloadId> for String {
    fn from(value: WorkloadId) -> Self {
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
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TableState {
    #[default]
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
    /// Construct a timestamp from Unix-epoch milliseconds.
    pub const fn from_unix_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Return this timestamp as Unix-epoch milliseconds.
    pub const fn as_unix_millis(self) -> u64 {
        self.0
    }

    /// Return whole Unix-epoch seconds, flooring any fractional second.
    pub const fn as_unix_secs_floor(self) -> u64 {
        self.0 / 1_000
    }

    /// Add an elapsed duration when the result and millisecond conversion fit.
    pub fn checked_add_duration(self, duration: std::time::Duration) -> Option<Self> {
        let millis = u64::try_from(duration.as_millis()).ok()?;
        self.0.checked_add(millis).map(Self)
    }

    /// Add an elapsed duration, saturating at the largest representable epoch.
    pub fn saturating_add_duration(self, duration: std::time::Duration) -> Self {
        let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(millis))
    }

    /// Subtract an elapsed duration when it does not cross the Unix epoch.
    pub fn checked_sub_duration(self, duration: std::time::Duration) -> Option<Self> {
        let millis = u64::try_from(duration.as_millis()).ok()?;
        self.0.checked_sub(millis).map(Self)
    }

    /// Subtract an elapsed duration, saturating at the Unix epoch.
    pub fn saturating_sub_duration(self, duration: std::time::Duration) -> Self {
        let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        Self(self.0.saturating_sub(millis))
    }

    /// Elapsed duration since `earlier`, or `None` when ordering is reversed.
    pub fn checked_duration_since(self, earlier: Self) -> Option<std::time::Duration> {
        self.0
            .checked_sub(earlier.0)
            .map(std::time::Duration::from_millis)
    }

    /// Elapsed duration since `earlier`, saturating at zero when reversed.
    pub fn saturating_duration_since(self, earlier: Self) -> std::time::Duration {
        std::time::Duration::from_millis(self.0.saturating_sub(earlier.0))
    }

    /// Returns the current wall-clock timestamp in milliseconds since epoch.
    pub(crate) fn now() -> Self {
        Self::saturating_from_system_time(std::time::SystemTime::now())
    }

    /// Convert a system time, returning `None` for pre-epoch observations or
    /// values whose millisecond representation exceeds `u64`.
    pub fn checked_from_system_time(time: std::time::SystemTime) -> Option<Self> {
        time.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            .map(Self)
    }

    /// Convert a system time, saturating pre-epoch observations at zero and
    /// excessively distant observations at the largest representable epoch.
    pub fn saturating_from_system_time(time: std::time::SystemTime) -> Self {
        match time.duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => Self(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)),
            Err(_) => Self(0),
        }
    }

    /// Convert to the standard-library system-time representation when it fits.
    pub fn checked_system_time(self) -> Option<std::time::SystemTime> {
        std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(self.0))
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn timestamp_system_time_conversion_handles_pre_epoch_explicitly() {
        assert_eq!(
            Timestamp::checked_from_system_time(UNIX_EPOCH - Duration::from_millis(1)),
            None
        );
        assert_eq!(
            Timestamp::saturating_from_system_time(UNIX_EPOCH - Duration::from_millis(1)),
            Timestamp(0)
        );
    }

    #[test]
    fn timestamp_from_system_time_preserves_epoch_millis() {
        assert_eq!(
            Timestamp::checked_from_system_time(UNIX_EPOCH + Duration::from_millis(1_234)),
            Some(Timestamp(1_234))
        );
        assert_eq!(
            Timestamp(1_234).checked_system_time(),
            Some(UNIX_EPOCH + Duration::from_millis(1_234))
        );
    }

    #[test]
    fn timestamp_duration_arithmetic_covers_exact_edge_and_overflow() {
        assert_eq!(
            Timestamp(10).checked_add_duration(Duration::from_millis(5)),
            Some(Timestamp(15))
        );
        assert_eq!(
            Timestamp(u64::MAX).checked_add_duration(Duration::from_millis(1)),
            None
        );
        assert_eq!(
            Timestamp(u64::MAX).saturating_add_duration(Duration::from_millis(1)),
            Timestamp(u64::MAX)
        );
        assert_eq!(
            Timestamp(5).checked_sub_duration(Duration::from_millis(5)),
            Some(Timestamp(0))
        );
        assert_eq!(
            Timestamp(4).checked_sub_duration(Duration::from_millis(5)),
            None
        );
        assert_eq!(
            Timestamp(4).saturating_sub_duration(Duration::from_millis(5)),
            Timestamp(0)
        );
        assert_eq!(
            Timestamp(5).checked_duration_since(Timestamp(5)),
            Some(Duration::ZERO)
        );
        assert_eq!(
            Timestamp(4).saturating_duration_since(Timestamp(5)),
            Duration::ZERO
        );
    }

    #[test]
    fn timestamp_epoch_seconds_floor_and_serialization_remain_stable() {
        assert_eq!(Timestamp::from_unix_millis(1_999).as_unix_secs_floor(), 1);
        assert_eq!(
            serde_json::to_string(&Timestamp(1_234)).expect("timestamp should serialize"),
            "1234"
        );
        assert_eq!(
            serde_json::from_str::<Timestamp>("1234").expect("timestamp should deserialize"),
            Timestamp(1_234)
        );
    }

    #[test]
    fn workload_id_accepts_arbitrary_sandbox_id_shapes() {
        // SandboxId imposes no character/length constraints, so WorkloadId must
        // accept values a logical-name rule would reject (dots, slashes, colons,
        // long strings). This is the whole point of the permissive newtype.
        let long = "a".repeat(200);
        for raw in ["sbx-01", "pod.default/abc:123", long.as_str()] {
            let id = WorkloadId::new(raw).expect("permissive workload id should accept it");
            assert_eq!(id.as_str(), raw);
            assert_eq!(id.to_string(), raw);
        }
    }

    #[test]
    fn workload_id_rejects_empty() {
        use std::str::FromStr;
        assert!(WorkloadId::new("").is_err());
        assert!(WorkloadId::from_str("").is_err());
    }

    #[test]
    fn workload_id_round_trips_through_string_and_serde() {
        let id = WorkloadId::new("workload-xyz").unwrap();
        let owned: String = id.clone().into();
        assert_eq!(owned, "workload-xyz");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"workload-xyz\"");
        assert_eq!(serde_json::from_str::<WorkloadId>(&json).unwrap(), id);
    }

    #[test]
    fn workload_id_usable_as_hashmap_key() {
        use std::collections::HashMap;
        let mut map: HashMap<WorkloadId, u32> = HashMap::new();
        map.insert(WorkloadId::new("a").unwrap(), 1);
        map.insert(WorkloadId::new("b").unwrap(), 2);
        assert_eq!(map.get(&WorkloadId::new("a").unwrap()), Some(&1));
        assert_eq!(map.get(&WorkloadId::new("missing").unwrap()), None);
    }
}
