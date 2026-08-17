//! Portable durable tenant-retirement identity, progress, and store port.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::pin::Pin;
use std::str::FromStr;

use nimbus_core::TenantId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::{
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceKind, WorkloadProvisionSourceResourceVersion,
};

pub const TENANT_RETIREMENT_FORMAT_VERSION: u32 = 1;
pub const MAX_TENANT_RETIREMENT_PAGE_SIZE: u16 = 256;

fn parse_decimal(value: &str, label: &'static str) -> Result<u64, TenantRetirementError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || value.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return Err(TenantRetirementError::InvalidCounter(label));
    }
    value
        .parse()
        .map_err(|_| TenantRetirementError::InvalidCounter(label))
}

macro_rules! decimal_counter {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn as_u64(self) -> u64 {
                self.0
            }

            pub const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                parse_decimal(&value, $label)
                    .map(Self)
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

decimal_counter!(
    TenantRetirementRevision,
    "tenant retirement revision must be canonical unsigned decimal text"
);
decimal_counter!(
    TenantWorkloadMutationEpoch,
    "tenant workload mutation epoch must be canonical unsigned decimal text"
);

/// Stable identity for one exact Engine tenant incarnation retirement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TenantRetirementId(String);

impl TenantRetirementId {
    const PREFIX: &'static str = "trt_";

    pub fn for_incarnation(tenant_id: &TenantId, incarnation: NonZeroU64) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"nimbus.workloads.tenant-retirement.v1\0");
        hasher.update(tenant_id.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(incarnation.get().to_string().as_bytes());
        Self(format!("{}{:x}", Self::PREFIX, hasher.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for TenantRetirementId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for TenantRetirementId {
    type Err = TenantRetirementError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digest =
            value
                .strip_prefix(Self::PREFIX)
                .ok_or(TenantRetirementError::InvalidIdentity(
                    "tenant retirement id has an invalid domain prefix",
                ))?;
        if digest.len() != 64
            || digest
                .bytes()
                .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
        {
            return Err(TenantRetirementError::InvalidIdentity(
                "tenant retirement id must contain one lowercase SHA-256 digest",
            ));
        }
        Ok(Self(value.to_owned()))
    }
}

impl Serialize for TenantRetirementId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TenantRetirementId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Exact source-owner facts frozen before tenant retirement effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantRetirementSource {
    identity: WorkloadProvisionSourceIdentity,
    source_generation: WorkloadProvisionSourceGeneration,
    resource_version: WorkloadProvisionSourceResourceVersion,
    has_observation: bool,
}

impl TenantRetirementSource {
    pub fn new(
        identity: WorkloadProvisionSourceIdentity,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: WorkloadProvisionSourceResourceVersion,
        has_observation: bool,
    ) -> Self {
        Self {
            identity,
            source_generation,
            resource_version,
            has_observation,
        }
    }

    pub fn identity(&self) -> &WorkloadProvisionSourceIdentity {
        &self.identity
    }

    pub const fn source_generation(&self) -> WorkloadProvisionSourceGeneration {
        self.source_generation
    }

    pub fn resource_version(&self) -> &WorkloadProvisionSourceResourceVersion {
        &self.resource_version
    }

    pub const fn has_observation(&self) -> bool {
        self.has_observation
    }

    fn order_key(&self) -> (u8, &str, Option<&str>) {
        let kind = match self.identity.kind() {
            WorkloadProvisionSourceKind::StandaloneSandbox => 0,
            WorkloadProvisionSourceKind::SandboxBackedService => 1,
        };
        (kind, self.identity.stable_name(), self.identity.profile())
    }
}

/// Durable progress. Every transition is idempotently recoverable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantRetirementPhase {
    IntentCommitted,
    ChildrenRecorded,
    SourcesFinalized,
    EngineDeleted,
    Recorded,
}

impl TenantRetirementPhase {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Recorded)
    }

    const fn next(self) -> Option<Self> {
        match self {
            Self::IntentCommitted => Some(Self::ChildrenRecorded),
            Self::ChildrenRecorded => Some(Self::SourcesFinalized),
            Self::SourcesFinalized => Some(Self::EngineDeleted),
            Self::EngineDeleted => Some(Self::Recorded),
            Self::Recorded => None,
        }
    }
}

/// Portable durable intent for one tenant-wide workload retirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantRetirementRecord {
    format_version: u32,
    retirement_id: TenantRetirementId,
    tenant_id: TenantId,
    #[serde(with = "canonical_nonzero_u64")]
    tenant_incarnation: NonZeroU64,
    revision: TenantRetirementRevision,
    phase: TenantRetirementPhase,
    sources: Vec<TenantRetirementSource>,
}

impl TenantRetirementRecord {
    pub fn new(
        tenant_id: TenantId,
        tenant_incarnation: NonZeroU64,
        mut sources: Vec<TenantRetirementSource>,
    ) -> Result<Self, TenantRetirementError> {
        sources.sort_by(|left, right| left.order_key().cmp(&right.order_key()));
        let record = Self {
            format_version: TENANT_RETIREMENT_FORMAT_VERSION,
            retirement_id: TenantRetirementId::for_incarnation(&tenant_id, tenant_incarnation),
            tenant_id,
            tenant_incarnation,
            revision: TenantRetirementRevision::new(0),
            phase: TenantRetirementPhase::IntentCommitted,
            sources,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), TenantRetirementError> {
        if self.format_version != TENANT_RETIREMENT_FORMAT_VERSION {
            return Err(TenantRetirementError::InvalidRecord(
                "tenant retirement format version is unsupported",
            ));
        }
        if self.retirement_id
            != TenantRetirementId::for_incarnation(&self.tenant_id, self.tenant_incarnation)
        {
            return Err(TenantRetirementError::InvalidRecord(
                "tenant retirement identity is crossed with tenant incarnation",
            ));
        }
        if self.revision.as_u64() != self.phase.revision() {
            return Err(TenantRetirementError::InvalidRecord(
                "tenant retirement revision is crossed with progress phase",
            ));
        }
        let mut previous: Option<(u8, &str, Option<&str>)> = None;
        let mut workload_names = std::collections::BTreeSet::new();
        for source in &self.sources {
            let key = source.order_key();
            if previous.is_some_and(|previous| key <= previous)
                || !workload_names.insert(source.identity.stable_name())
            {
                return Err(TenantRetirementError::InvalidRecord(
                    "tenant retirement sources are duplicated or not canonically ordered",
                ));
            }
            previous = Some(key);
        }
        Ok(())
    }

    pub fn advance(&self, target: TenantRetirementPhase) -> Result<Self, TenantRetirementError> {
        self.validate()?;
        if self.phase.next() != Some(target) {
            return Err(TenantRetirementError::InvalidTransition(
                "tenant retirement progress must advance exactly one phase",
            ));
        }
        let revision = self
            .revision
            .checked_next()
            .ok_or(TenantRetirementError::RevisionOverflow)?;
        let mut next = self.clone();
        next.revision = revision;
        next.phase = target;
        Ok(next)
    }

    pub fn retirement_id(&self) -> &TenantRetirementId {
        &self.retirement_id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub const fn tenant_incarnation(&self) -> NonZeroU64 {
        self.tenant_incarnation
    }

    pub const fn revision(&self) -> TenantRetirementRevision {
        self.revision
    }

    pub const fn phase(&self) -> TenantRetirementPhase {
        self.phase
    }

    pub fn sources(&self) -> &[TenantRetirementSource] {
        &self.sources
    }
}

impl TenantRetirementPhase {
    const fn revision(self) -> u64 {
        match self {
            Self::IntentCommitted => 0,
            Self::ChildrenRecorded => 1,
            Self::SourcesFinalized => 2,
            Self::EngineDeleted => 3,
            Self::Recorded => 4,
        }
    }
}

mod canonical_nonzero_u64 {
    use std::num::NonZeroU64;

    use serde::{Deserialize, Deserializer, Serializer};

    use super::parse_decimal;

    pub(super) fn serialize<S>(value: &NonZeroU64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.get().to_string())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<NonZeroU64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let value = parse_decimal(
            &value,
            "tenant incarnation must be canonical nonzero unsigned decimal text",
        )
        .map_err(serde::de::Error::custom)?;
        NonZeroU64::new(value).ok_or_else(|| {
            serde::de::Error::custom(
                "tenant incarnation must be canonical nonzero unsigned decimal text",
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantRetirementError {
    InvalidIdentity(&'static str),
    InvalidCounter(&'static str),
    InvalidRecord(&'static str),
    InvalidTransition(&'static str),
    RevisionOverflow,
}

impl Display for TenantRetirementError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(message)
            | Self::InvalidCounter(message)
            | Self::InvalidRecord(message)
            | Self::InvalidTransition(message) => formatter.write_str(message),
            Self::RevisionOverflow => formatter.write_str("tenant retirement revision overflow"),
        }
    }
}

impl StdError for TenantRetirementError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantRetirementExpected {
    Missing,
    Revision(TenantRetirementRevision),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantRetirementCommit {
    Applied,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantRetirementStoreError {
    Conflict {
        expected: TenantRetirementExpected,
        observed: Option<TenantRetirementRevision>,
    },
    Ambiguous,
    Corrupt,
    Unavailable,
    Invalid(TenantRetirementError),
}

impl Display for TenantRetirementStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { expected, observed } => write!(
                formatter,
                "tenant retirement CAS conflict: expected {expected:?}, observed {observed:?}"
            ),
            Self::Ambiguous => formatter.write_str("tenant retirement outcome is ambiguous"),
            Self::Corrupt => formatter.write_str("tenant retirement store is corrupt"),
            Self::Unavailable => formatter.write_str("tenant retirement store is unavailable"),
            Self::Invalid(error) => write!(formatter, "invalid tenant retirement: {error}"),
        }
    }
}

impl StdError for TenantRetirementStoreError {}

impl From<TenantRetirementError> for TenantRetirementStoreError {
    fn from(value: TenantRetirementError) -> Self {
        Self::Invalid(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantRetirementCursor(TenantRetirementId);

impl TenantRetirementCursor {
    pub fn for_record(record: &TenantRetirementRecord) -> Result<Self, TenantRetirementStoreError> {
        record.validate()?;
        Ok(Self(record.retirement_id().clone()))
    }

    pub fn retirement_id(&self) -> &TenantRetirementId {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantRetirementPageRequest {
    after: Option<TenantRetirementCursor>,
    limit: u16,
}

impl TenantRetirementPageRequest {
    pub fn new(
        after: Option<TenantRetirementCursor>,
        limit: u16,
    ) -> Result<Self, TenantRetirementStoreError> {
        if limit == 0 || limit > MAX_TENANT_RETIREMENT_PAGE_SIZE {
            return Err(TenantRetirementStoreError::Invalid(
                TenantRetirementError::InvalidCounter(
                    "tenant retirement page limit must be between 1 and 256",
                ),
            ));
        }
        Ok(Self { after, limit })
    }

    pub fn after(&self) -> Option<&TenantRetirementCursor> {
        self.after.as_ref()
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantRetirementPage {
    records: Vec<TenantRetirementRecord>,
    next_cursor: Option<TenantRetirementCursor>,
}

impl TenantRetirementPage {
    pub fn active(
        request: &TenantRetirementPageRequest,
        records: Vec<TenantRetirementRecord>,
        has_more: bool,
    ) -> Result<Self, TenantRetirementStoreError> {
        if records.iter().any(|record| record.phase().is_terminal()) {
            return Err(TenantRetirementStoreError::Corrupt);
        }
        Self::retained(request, records, has_more)
    }

    pub fn retained(
        request: &TenantRetirementPageRequest,
        records: Vec<TenantRetirementRecord>,
        has_more: bool,
    ) -> Result<Self, TenantRetirementStoreError> {
        if records.len() > usize::from(request.limit) || has_more && records.is_empty() {
            return Err(TenantRetirementStoreError::Corrupt);
        }
        let mut previous = request.after.clone();
        for record in &records {
            record.validate()?;
            let cursor = TenantRetirementCursor::for_record(record)?;
            if previous
                .as_ref()
                .is_some_and(|previous| cursor.retirement_id() <= previous.retirement_id())
            {
                return Err(TenantRetirementStoreError::Corrupt);
            }
            previous = Some(cursor);
        }
        Ok(Self {
            records,
            next_cursor: if has_more { previous } else { None },
        })
    }

    pub fn records(&self) -> &[TenantRetirementRecord] {
        &self.records
    }

    pub fn next_cursor(&self) -> Option<&TenantRetirementCursor> {
        self.next_cursor.as_ref()
    }
}

pub type TenantRetirementFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TenantRetirementStoreError>> + Send + 'a>>;

/// Portable persistence boundary for tenant retirement and stable saga scans.
pub trait TenantRetirementStore: Send + Sync + 'static {
    fn load_retirement<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> TenantRetirementFuture<'a, Option<TenantRetirementRecord>>;

    fn compare_and_swap_retirement<'a>(
        &'a self,
        expected: TenantRetirementExpected,
        next: TenantRetirementRecord,
    ) -> TenantRetirementFuture<'a, TenantRetirementCommit>;

    fn delete_retirement<'a>(
        &'a self,
        expected: TenantRetirementRecord,
    ) -> TenantRetirementFuture<'a, TenantRetirementCommit>;

    fn list_active_retirements<'a>(
        &'a self,
        request: TenantRetirementPageRequest,
    ) -> TenantRetirementFuture<'a, TenantRetirementPage>;

    /// Lists every retained record, including terminal records awaiting exact
    /// barrier release and deletion after a process cut.
    fn list_retirements<'a>(
        &'a self,
        request: TenantRetirementPageRequest,
    ) -> TenantRetirementFuture<'a, TenantRetirementPage>;

    fn load_workload_mutation_epoch<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> TenantRetirementFuture<'a, TenantWorkloadMutationEpoch>;
}

#[cfg(test)]
#[path = "tenant_retirement/tests.rs"]
mod tests;
