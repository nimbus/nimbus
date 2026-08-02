//! Durable workload-saga store port and bounded recovery vocabulary.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use nimbus_core::TenantId;

use crate::{
    WorkloadSagaError, WorkloadSagaId, WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaRevision,
};

pub const MAX_WORKLOAD_SAGA_PAGE_SIZE: u16 = 256;

pub type WorkloadSagaFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WorkloadSagaStoreError>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadSagaExpected {
    Missing,
    Revision(WorkloadSagaRevision),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadSagaCommit {
    Applied,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadSagaStoreError {
    Conflict {
        expected: WorkloadSagaExpected,
        observed: Option<WorkloadSagaRevision>,
    },
    Ambiguous,
    Corrupt,
    Unavailable,
    InvalidTransition(WorkloadSagaError),
}

impl Display for WorkloadSagaStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { expected, observed } => write!(
                formatter,
                "workload saga CAS conflict: expected {expected:?}, observed {observed:?}"
            ),
            Self::Ambiguous => formatter.write_str("workload saga commit outcome is ambiguous"),
            Self::Corrupt => formatter.write_str("workload saga record is corrupt"),
            Self::Unavailable => formatter.write_str("workload saga store is unavailable"),
            Self::InvalidTransition(error) => write!(formatter, "invalid workload saga: {error}"),
        }
    }
}

impl StdError for WorkloadSagaStoreError {}

impl From<WorkloadSagaError> for WorkloadSagaStoreError {
    fn from(value: WorkloadSagaError) -> Self {
        Self::InvalidTransition(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaRecoveryCursor {
    saga_id: WorkloadSagaId,
}

impl WorkloadSagaRecoveryCursor {
    pub fn new(saga_id: WorkloadSagaId) -> Self {
        Self { saga_id }
    }

    pub fn for_record(record: &WorkloadSagaRecord) -> Result<Self, WorkloadSagaStoreError> {
        if !record.requires_recovery() {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidTransition(
                    "recovery cursor cannot name a quiescent record",
                ),
            ));
        }
        Ok(Self::new(record.saga_id().clone()))
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
    }

    fn order_key(&self) -> &WorkloadSagaId {
        &self.saga_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaPageRequest {
    after: Option<WorkloadSagaRecoveryCursor>,
    limit: u16,
}

impl WorkloadSagaPageRequest {
    pub fn new(
        after: Option<WorkloadSagaRecoveryCursor>,
        limit: u16,
    ) -> Result<Self, WorkloadSagaStoreError> {
        if limit == 0 || limit > MAX_WORKLOAD_SAGA_PAGE_SIZE {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidCounter(
                    "workload saga recovery limit must be between 1 and 256",
                ),
            ));
        }
        Ok(Self { after, limit })
    }

    pub fn after(&self) -> Option<&WorkloadSagaRecoveryCursor> {
        self.after.as_ref()
    }

    pub fn limit(&self) -> u16 {
        self.limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaPage {
    records: Vec<WorkloadSagaRecord>,
    next_cursor: Option<WorkloadSagaRecoveryCursor>,
}

impl WorkloadSagaPage {
    pub fn new(
        request: &WorkloadSagaPageRequest,
        records: Vec<WorkloadSagaRecord>,
        has_more: bool,
    ) -> Result<Self, WorkloadSagaStoreError> {
        if records.len() > usize::from(request.limit) {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidEvidence(
                    "workload saga page exceeds its requested limit",
                ),
            ));
        }
        if has_more && records.is_empty() {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidEvidence(
                    "workload saga page cannot claim more records after an empty result",
                ),
            ));
        }

        let mut previous = request.after.clone();
        for record in &records {
            let cursor = WorkloadSagaRecoveryCursor::for_record(record)?;
            if previous
                .as_ref()
                .is_some_and(|previous| cursor.order_key() <= previous.order_key())
            {
                return Err(WorkloadSagaStoreError::InvalidTransition(
                    WorkloadSagaError::InvalidEvidence(
                        "workload saga page is duplicated, identity-unsorted, or cursor-regressing",
                    ),
                ));
            }
            previous = Some(cursor);
        }

        let next_cursor = if has_more { previous } else { None };
        Ok(Self {
            records,
            next_cursor,
        })
    }

    pub fn records(&self) -> &[WorkloadSagaRecord] {
        &self.records
    }

    pub fn next_cursor(&self) -> Option<&WorkloadSagaRecoveryCursor> {
        self.next_cursor.as_ref()
    }

    pub fn into_records(self) -> Vec<WorkloadSagaRecord> {
        self.records
    }
}

/// Immutable tenant-inventory cursor keyed by logical workload identity.
///
/// Tenant cursors are intentionally distinct from recovery cursors: tenant
/// inventory includes quiescent and terminal records that global recovery
/// omits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaTenantCursor {
    key: WorkloadSagaKey,
}

impl WorkloadSagaTenantCursor {
    pub fn new(key: WorkloadSagaKey) -> Self {
        Self { key }
    }

    pub fn for_record(record: &WorkloadSagaRecord) -> Self {
        Self::new(record.key().clone())
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn tenant_id(&self) -> &TenantId {
        self.key.tenant_id()
    }

    fn order_key(&self) -> &WorkloadSagaKey {
        &self.key
    }
}

/// Bounded request for complete workload-saga inventory within one tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaTenantPageRequest {
    after: Option<WorkloadSagaTenantCursor>,
    limit: u16,
}

impl WorkloadSagaTenantPageRequest {
    pub fn new(
        after: Option<WorkloadSagaTenantCursor>,
        limit: u16,
    ) -> Result<Self, WorkloadSagaStoreError> {
        if limit == 0 || limit > MAX_WORKLOAD_SAGA_PAGE_SIZE {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidCounter(
                    "workload saga tenant limit must be between 1 and 256",
                ),
            ));
        }
        Ok(Self { after, limit })
    }

    pub fn after(&self) -> Option<&WorkloadSagaTenantCursor> {
        self.after.as_ref()
    }

    pub fn limit(&self) -> u16 {
        self.limit
    }

    pub fn validate_for_tenant(&self, tenant_id: &TenantId) -> Result<(), WorkloadSagaStoreError> {
        if self
            .after
            .as_ref()
            .is_some_and(|cursor| cursor.tenant_id() != tenant_id)
        {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidEvidence(
                    "workload saga tenant cursor belongs to a different tenant",
                ),
            ));
        }
        Ok(())
    }
}

/// Validated, bounded workload-saga inventory page for one tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaTenantPage {
    tenant_id: TenantId,
    records: Vec<WorkloadSagaRecord>,
    next_cursor: Option<WorkloadSagaTenantCursor>,
}

impl WorkloadSagaTenantPage {
    pub fn new(
        tenant_id: &TenantId,
        request: &WorkloadSagaTenantPageRequest,
        records: Vec<WorkloadSagaRecord>,
        has_more: bool,
    ) -> Result<Self, WorkloadSagaStoreError> {
        request.validate_for_tenant(tenant_id)?;
        if records.len() > usize::from(request.limit) {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidEvidence(
                    "workload saga tenant page exceeds its requested limit",
                ),
            ));
        }
        if has_more && records.is_empty() {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidEvidence(
                    "workload saga tenant page cannot claim more records after an empty result",
                ),
            ));
        }
        if has_more && records.len() != usize::from(request.limit) {
            return Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidEvidence(
                    "workload saga tenant page claiming more records must fill its requested limit",
                ),
            ));
        }

        let mut previous = request.after.clone();
        for record in &records {
            record.validate()?;
            if record.key().tenant_id() != tenant_id {
                return Err(WorkloadSagaStoreError::InvalidTransition(
                    WorkloadSagaError::InvalidEvidence(
                        "workload saga tenant page contains a crossed-tenant record",
                    ),
                ));
            }
            let cursor = WorkloadSagaTenantCursor::for_record(record);
            if previous
                .as_ref()
                .is_some_and(|previous| cursor.order_key() <= previous.order_key())
            {
                return Err(WorkloadSagaStoreError::InvalidTransition(
                    WorkloadSagaError::InvalidEvidence(
                        "workload saga tenant page is duplicated, identity-unsorted, or cursor-regressing",
                    ),
                ));
            }
            previous = Some(cursor);
        }

        let next_cursor = if has_more { previous } else { None };
        Ok(Self {
            tenant_id: tenant_id.clone(),
            records,
            next_cursor,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn records(&self) -> &[WorkloadSagaRecord] {
        &self.records
    }

    pub fn next_cursor(&self) -> Option<&WorkloadSagaTenantCursor> {
        self.next_cursor.as_ref()
    }

    pub fn into_records(self) -> Vec<WorkloadSagaRecord> {
        self.records
    }
}

pub trait WorkloadSagaStore: Send + Sync + 'static {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>>;

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit>;

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage>;

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage>;
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
