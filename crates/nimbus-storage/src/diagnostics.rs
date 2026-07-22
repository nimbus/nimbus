use nimbus_core::{HistoricalReadErrorKind, Result, Schema, SequenceNumber};
use serde::{Deserialize, Serialize};

use crate::{
    CURRENT_STORAGE_FORMAT_VERSION, JournalProgress, LibsqlReplicaTenantStore, MySqlTenantStore,
    PostgresTenantStore, SqliteTenantStore, StorageFormatVersion, TableBackendLayout, TenantStore,
};
use crate::{RetentionGcConfig, RetentionGcWatermarks, RetentionPin};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCapabilities {
    pub backend: String,
    pub backend_layout: TableBackendLayout,
    pub capability_profile: StorageCapabilityProfile,
    pub strong_reads: bool,
    pub eventual_reads: bool,
    pub tenant_event_journal: bool,
    pub retention_floor: bool,
    pub exact_summary: bool,
    pub encryption_posture: String,
    pub feature_support: Vec<StorageFeatureSupport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageHealthDiagnostic {
    pub backend: String,
    pub backend_layout: TableBackendLayout,
    pub backend_capability_profile: StorageCapabilityProfile,
    pub event_log_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    pub retention_floor: Option<SequenceNumber>,
    pub format_version: StorageFormatVersion,
    pub document_versions: DocumentVersionStorageDiagnostic,
    pub index_versions: IndexVersionStorageDiagnostic,
    pub mvcc: MvccOperatorDiagnostic,
    pub historical_query_admission: HistoricalQueryAdmissionDiagnostic,
    pub storage_pressure: StoragePressureDiagnostic,
    pub backend_support: Vec<StorageFeatureSupport>,
    pub adapter_support: Vec<AdapterSupportDiagnostic>,
    pub retention_pins: Vec<RetentionPin>,
    pub retention_gc: RetentionGcWatermarks,
    pub encryption_posture: String,
    pub freshness_lag: u64,
    pub last_recovery_status: String,
    pub exact_summary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentVersionStorageDiagnostic {
    pub format_version: Option<StorageFormatVersion>,
    pub version_count: u64,
    pub min_sequence: Option<SequenceNumber>,
    pub max_sequence: Option<SequenceNumber>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexVersionStorageDiagnostic {
    pub format_version: Option<StorageFormatVersion>,
    pub version_count: u64,
    pub min_sequence: Option<SequenceNumber>,
    pub max_sequence: Option<SequenceNumber>,
}

/// Bounded-cardinality counters for the provider write pipeline of one loaded
/// tenant. SQL text, tenant identifiers, and statement error strings are
/// deliberately excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderWritePipelineDiagnostic {
    pub adapter: String,
    pub configured_max_in_flight: u64,
    pub batch_attempt_count: u64,
    pub journal_record_count: u64,
    pub journal_statement_count: u64,
    /// Operations admitted through the bounded provider pipeline. This is not
    /// a count of SQL statements issued internally while applying a record.
    pub provider_operation_count: u64,
    pub max_observed_in_flight: u64,
    /// Cancellations observed after a batch enters the provider pipeline,
    /// including later transaction checks. Cancellations are also included in
    /// `error_count`.
    pub cancellation_count: u64,
    /// Errors returned by admitted pipeline operations, plus cancellations
    /// observed after batch admission. Other transaction setup, validation,
    /// apply, and commit errors are reported by their owning diagnostics.
    pub error_count: u64,
    /// Wall time for the operations admitted by this adapter. PostgreSQL times
    /// its append/apply pair; MySQL times its batched journal operation, so this
    /// value must not be compared across adapters as end-to-end commit latency.
    pub elapsed_nanos: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFeature {
    LatestReads,
    HistoricalDocumentReads,
    HistoricalIndexReads,
    PointInTimeRestore,
    Changefeed,
    RetentionGc,
    OperatorDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFeatureSupportState {
    Supported,
    ExternalEvidencePending,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCapabilityProfile {
    LatestOnly,
    HistoricalReads,
    HistoricalReadsPitr,
    HistoricalReadsPitrCdc,
    EnterpriseComplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageFeatureSupport {
    pub feature: StorageFeature,
    pub state: StorageFeatureSupportState,
    pub error_kind: Option<HistoricalReadErrorKind>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterSupportDiagnostic {
    pub adapter: String,
    pub capability_profile: StorageCapabilityProfile,
    pub features: Vec<StorageFeatureSupport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MvccOperatorDiagnostic {
    pub state: StorageOperatorState,
    pub document_versions: DocumentVersionStorageDiagnostic,
    pub index_versions: IndexVersionStorageDiagnostic,
    pub version_counts: MvccVersionCountsDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageOperatorState {
    Healthy,
    Lagging,
    Compacting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MvccVersionCountsDiagnostic {
    pub table_identity_versions: u64,
    pub schema_versions: u64,
    pub index_definition_versions: u64,
    pub read_policy_versions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalQueryAdmissionRequest {
    pub requested_sequence: SequenceNumber,
    pub feature_supported: bool,
    pub format_compatible: bool,
    pub policy_allowed: bool,
}

impl HistoricalQueryAdmissionRequest {
    pub fn supported(requested_sequence: SequenceNumber) -> Self {
        Self {
            requested_sequence,
            feature_supported: true,
            format_compatible: true,
            policy_allowed: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalQueryAdmissionDiagnostic {
    pub state: HistoricalQueryAdmissionState,
    pub requested_sequence: SequenceNumber,
    pub oldest_retained_sequence: SequenceNumber,
    pub latest_sequence: SequenceNumber,
    pub error_kind: Option<HistoricalReadErrorKind>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalQueryAdmissionState {
    Admitted,
    Expired,
    Unsupported,
    FormatMismatch,
    PolicyGated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoragePressureDiagnostic {
    pub state: StoragePressureState,
    pub freshness_lag: u64,
    pub retained_version_count: u64,
    pub retained_sequence_span: Option<u64>,
    pub active_pin_count: usize,
    pub safe_prune_before: SequenceNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoragePressureState {
    Nominal,
    ReplayLagging,
    CompactionRecommended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendParityDiagnostic {
    pub state: BackendParityState,
    pub left_backend: String,
    pub right_backend: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendParityState {
    Matched,
    BackendDivergence,
}

impl StorageFeatureSupport {
    fn supported(feature: StorageFeature, reason: impl Into<String>) -> Self {
        Self {
            feature,
            state: StorageFeatureSupportState::Supported,
            error_kind: None,
            reason: reason.into(),
        }
    }

    fn unsupported(
        feature: StorageFeature,
        error_kind: HistoricalReadErrorKind,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            feature,
            state: StorageFeatureSupportState::Unsupported,
            error_kind: Some(error_kind),
            reason: reason.into(),
        }
    }
}

impl StorageHealthDiagnostic {
    pub fn diagnose_historical_query_admission(
        &self,
        request: HistoricalQueryAdmissionRequest,
    ) -> HistoricalQueryAdmissionDiagnostic {
        historical_query_admission_diagnostic(
            self.applied_head,
            self.retention_gc.document_versions.safe_prune_before,
            request,
        )
    }
}

impl BackendParityDiagnostic {
    pub fn compare(left: &StorageHealthDiagnostic, right: &StorageHealthDiagnostic) -> Self {
        let matched = left.event_log_head == right.event_log_head
            && left.applied_head == right.applied_head
            && left.format_version == right.format_version
            && left.document_versions == right.document_versions
            && left.index_versions == right.index_versions
            && left.mvcc.version_counts == right.mvcc.version_counts;
        if matched {
            Self {
                state: BackendParityState::Matched,
                left_backend: left.backend.clone(),
                right_backend: right.backend.clone(),
                reason: "operator-visible MVCC heads, formats, and version counts match"
                    .to_string(),
            }
        } else {
            Self {
                state: BackendParityState::BackendDivergence,
                left_backend: left.backend.clone(),
                right_backend: right.backend.clone(),
                reason: "operator-visible MVCC heads, formats, or version counts diverge"
                    .to_string(),
            }
        }
    }
}

fn capabilities(
    backend: &str,
    backend_layout: TableBackendLayout,
    eventual_reads: bool,
    encryption_posture: &str,
) -> StorageCapabilities {
    let feature_support = shared_journal_backend_feature_support();
    let capability_profile = capability_profile_for_features(&feature_support);
    StorageCapabilities {
        backend: backend.to_string(),
        backend_layout,
        capability_profile,
        strong_reads: true,
        eventual_reads,
        tenant_event_journal: true,
        retention_floor: true,
        exact_summary: true,
        encryption_posture: encryption_posture.to_string(),
        feature_support,
    }
}

struct StorageHealthDiagnosticInput {
    capabilities: StorageCapabilities,
    progress: JournalProgress,
    retention_floor: Option<SequenceNumber>,
    document_versions: DocumentVersionStorageDiagnostic,
    index_versions: IndexVersionStorageDiagnostic,
    version_counts: MvccVersionCountsDiagnostic,
    retention_pins: Vec<RetentionPin>,
    retention_gc: RetentionGcWatermarks,
}

fn diagnostic(input: StorageHealthDiagnosticInput) -> StorageHealthDiagnostic {
    let StorageHealthDiagnosticInput {
        capabilities,
        progress,
        retention_floor,
        document_versions,
        index_versions,
        version_counts,
        retention_pins,
        retention_gc,
    } = input;
    let freshness_lag = progress
        .durable_head
        .0
        .saturating_sub(progress.applied_head.0);
    let storage_pressure = storage_pressure_diagnostic(
        freshness_lag,
        &document_versions,
        &index_versions,
        &retention_gc,
    );
    let state = operator_state_for_pressure(storage_pressure.state);
    let historical_query_admission = historical_query_admission_diagnostic(
        progress.applied_head,
        retention_gc.document_versions.safe_prune_before,
        HistoricalQueryAdmissionRequest::supported(progress.applied_head),
    );
    let backend_support = capabilities.feature_support.clone();
    StorageHealthDiagnostic {
        backend: capabilities.backend,
        backend_layout: capabilities.backend_layout,
        backend_capability_profile: capabilities.capability_profile,
        event_log_head: progress.durable_head,
        applied_head: progress.applied_head,
        retention_floor,
        format_version: CURRENT_STORAGE_FORMAT_VERSION,
        document_versions: document_versions.clone(),
        index_versions: index_versions.clone(),
        mvcc: MvccOperatorDiagnostic {
            state,
            document_versions,
            index_versions,
            version_counts,
        },
        historical_query_admission,
        storage_pressure,
        backend_support,
        adapter_support: adapter_support_matrix(),
        retention_pins,
        retention_gc,
        encryption_posture: capabilities.encryption_posture,
        freshness_lag,
        last_recovery_status: if progress.durable_head == progress.applied_head {
            "caught_up".to_string()
        } else {
            "pending_replay".to_string()
        },
        exact_summary: capabilities.exact_summary,
    }
}

fn shared_journal_backend_feature_support() -> Vec<StorageFeatureSupport> {
    let mut supports = vec![
        StorageFeatureSupport::supported(
            StorageFeature::LatestReads,
            "latest-row reads stay on the backend fast path",
        ),
        StorageFeatureSupport::supported(
            StorageFeature::RetentionGc,
            "typed retention watermarks and GC summaries are available",
        ),
        StorageFeatureSupport::supported(
            StorageFeature::OperatorDiagnostics,
            "storage health diagnostics are available",
        ),
    ];
    for feature in [
        StorageFeature::HistoricalDocumentReads,
        StorageFeature::HistoricalIndexReads,
        StorageFeature::PointInTimeRestore,
        StorageFeature::Changefeed,
    ] {
        supports.push(StorageFeatureSupport::supported(
            feature,
            "implemented on the shared tenant-event journal and covered by deterministic plus live provider verification",
        ));
    }
    supports
}

fn adapter_support_matrix() -> Vec<AdapterSupportDiagnostic> {
    let unsupported_extension =
        "SEQ storage primitive exists, but this adapter has no public equivalent yet; fail closed";
    [
        "convex",
        "firebase",
        "cloud_functions",
        "dynamodb",
        "mongodb",
    ]
    .into_iter()
    .map(|adapter| {
        let features = vec![
            StorageFeatureSupport::supported(
                StorageFeature::LatestReads,
                "adapter latest-row behavior stays on the shared storage path",
            ),
            StorageFeatureSupport::unsupported(
                StorageFeature::HistoricalDocumentReads,
                HistoricalReadErrorKind::UnsupportedAdapter,
                unsupported_extension,
            ),
            StorageFeatureSupport::unsupported(
                StorageFeature::PointInTimeRestore,
                HistoricalReadErrorKind::UnsupportedAdapter,
                unsupported_extension,
            ),
            StorageFeatureSupport::unsupported(
                StorageFeature::Changefeed,
                HistoricalReadErrorKind::UnsupportedAdapter,
                unsupported_extension,
            ),
            StorageFeatureSupport::supported(
                StorageFeature::OperatorDiagnostics,
                "support state is visible to operators",
            ),
        ];
        AdapterSupportDiagnostic {
            adapter: adapter.to_string(),
            capability_profile: capability_profile_for_features(&features),
            features,
        }
    })
    .chain(std::iter::once({
        let features = vec![
            StorageFeatureSupport::supported(
                StorageFeature::LatestReads,
                "native APIs are the canonical Nimbus surface",
            ),
            StorageFeatureSupport::unsupported(
                StorageFeature::HistoricalDocumentReads,
                HistoricalReadErrorKind::UnsupportedAdapter,
                "storage primitive exists, but no public native HTTP/WebSocket route is documented yet; fail closed",
            ),
            StorageFeatureSupport::unsupported(
                StorageFeature::PointInTimeRestore,
                HistoricalReadErrorKind::UnsupportedAdapter,
                "storage primitive exists, but no public native HTTP/WebSocket route is documented yet; fail closed",
            ),
            StorageFeatureSupport::unsupported(
                StorageFeature::Changefeed,
                HistoricalReadErrorKind::UnsupportedAdapter,
                "storage primitive exists, but no public native HTTP/WebSocket route is documented yet; fail closed",
            ),
            StorageFeatureSupport::supported(
                StorageFeature::OperatorDiagnostics,
                "native extension can expose storage health diagnostics",
            ),
        ];
        AdapterSupportDiagnostic {
            adapter: "native_http_websocket".to_string(),
            capability_profile: capability_profile_for_features(&features),
            features,
        }
    }))
    .collect()
}

fn capability_profile_for_features(features: &[StorageFeatureSupport]) -> StorageCapabilityProfile {
    let supports = |feature| {
        features.iter().any(|support| {
            support.feature == feature && support.state == StorageFeatureSupportState::Supported
        })
    };
    let historical_reads = supports(StorageFeature::HistoricalDocumentReads)
        && supports(StorageFeature::HistoricalIndexReads);
    let pitr = supports(StorageFeature::PointInTimeRestore);
    let cdc = supports(StorageFeature::Changefeed);
    let enterprise_complete = historical_reads
        && pitr
        && cdc
        && supports(StorageFeature::RetentionGc)
        && supports(StorageFeature::OperatorDiagnostics);

    if enterprise_complete {
        StorageCapabilityProfile::EnterpriseComplete
    } else if historical_reads && pitr && cdc {
        StorageCapabilityProfile::HistoricalReadsPitrCdc
    } else if historical_reads && pitr {
        StorageCapabilityProfile::HistoricalReadsPitr
    } else if historical_reads {
        StorageCapabilityProfile::HistoricalReads
    } else {
        StorageCapabilityProfile::LatestOnly
    }
}

fn mvcc_version_counts(
    schema: &Schema,
    table_identity_versions: u64,
) -> MvccVersionCountsDiagnostic {
    MvccVersionCountsDiagnostic {
        table_identity_versions,
        schema_versions: schema.tables.len() as u64,
        index_definition_versions: schema
            .tables
            .values()
            .map(|table| table.indexes.len() as u64)
            .sum(),
        read_policy_versions: schema
            .tables
            .values()
            .filter(|table| table.access_policy.is_some())
            .count() as u64,
    }
}

fn storage_pressure_diagnostic(
    freshness_lag: u64,
    document_versions: &DocumentVersionStorageDiagnostic,
    index_versions: &IndexVersionStorageDiagnostic,
    retention_gc: &RetentionGcWatermarks,
) -> StoragePressureDiagnostic {
    let retained_version_count = document_versions
        .version_count
        .saturating_add(index_versions.version_count);
    let min_sequence = [document_versions.min_sequence, index_versions.min_sequence]
        .into_iter()
        .flatten()
        .min_by_key(|sequence| sequence.0);
    let max_sequence = [document_versions.max_sequence, index_versions.max_sequence]
        .into_iter()
        .flatten()
        .max_by_key(|sequence| sequence.0);
    let retained_sequence_span = min_sequence
        .zip(max_sequence)
        .map(|(min, max)| max.0.saturating_sub(min.0).saturating_add(1));
    let safe_prune_before = retention_gc
        .document_versions
        .safe_prune_before
        .max(retention_gc.index_versions.safe_prune_before);
    let compactable = min_sequence
        .map(|min| min.0 < safe_prune_before.0)
        .unwrap_or(false);
    let state = if freshness_lag > 0 {
        StoragePressureState::ReplayLagging
    } else if compactable {
        StoragePressureState::CompactionRecommended
    } else {
        StoragePressureState::Nominal
    };
    StoragePressureDiagnostic {
        state,
        freshness_lag,
        retained_version_count,
        retained_sequence_span,
        active_pin_count: retention_gc.document_versions.active_pin_count,
        safe_prune_before,
    }
}

fn operator_state_for_pressure(state: StoragePressureState) -> StorageOperatorState {
    match state {
        StoragePressureState::Nominal => StorageOperatorState::Healthy,
        StoragePressureState::ReplayLagging => StorageOperatorState::Lagging,
        StoragePressureState::CompactionRecommended => StorageOperatorState::Compacting,
    }
}

fn historical_query_admission_diagnostic(
    latest_sequence: SequenceNumber,
    oldest_retained_sequence: SequenceNumber,
    request: HistoricalQueryAdmissionRequest,
) -> HistoricalQueryAdmissionDiagnostic {
    let (state, error_kind, message) = if !request.feature_supported {
        (
            HistoricalQueryAdmissionState::Unsupported,
            Some(HistoricalReadErrorKind::UnsupportedBackend),
            "historical query feature is not supported by this backend or surface".to_string(),
        )
    } else if !request.format_compatible {
        (
            HistoricalQueryAdmissionState::FormatMismatch,
            Some(HistoricalReadErrorKind::FormatMismatch),
            "historical query cannot run against the current storage format".to_string(),
        )
    } else if !request.policy_allowed {
        (
            HistoricalQueryAdmissionState::PolicyGated,
            Some(HistoricalReadErrorKind::PolicySnapshotMissing),
            "historical query is blocked by read-policy snapshot admission".to_string(),
        )
    } else if request.requested_sequence.0 < oldest_retained_sequence.0 {
        (
            HistoricalQueryAdmissionState::Expired,
            Some(HistoricalReadErrorKind::RetentionExpired),
            "requested sequence is older than the retained MVCC window".to_string(),
        )
    } else {
        (
            HistoricalQueryAdmissionState::Admitted,
            None,
            "historical query is within the retained MVCC window".to_string(),
        )
    };
    HistoricalQueryAdmissionDiagnostic {
        state,
        requested_sequence: request.requested_sequence,
        oldest_retained_sequence,
        latest_sequence,
        error_kind,
        message,
    }
}

macro_rules! impl_storage_health_diagnostic {
    ($store:ty) => {
        impl $store {
            pub fn storage_health_diagnostic(&self) -> Result<StorageHealthDiagnostic> {
                self.storage_health_diagnostic_with_retention_config(RetentionGcConfig::default())
            }

            pub fn storage_health_diagnostic_with_retention_config(
                &self,
                retention_config: RetentionGcConfig,
            ) -> Result<StorageHealthDiagnostic> {
                let progress = self.journal_progress()?;
                let schema = self.load_schema()?;
                let table_identity_versions = self.table_identity_diagnostics()?.len() as u64;
                let document_versions = self.document_version_storage_diagnostic()?;
                let index_versions = self.index_version_storage_diagnostic()?;
                let retention_gc = self
                    .retention_floor
                    .gc_watermarks(progress.applied_head, retention_config);
                Ok(diagnostic(StorageHealthDiagnosticInput {
                    capabilities: self.storage_capabilities(),
                    progress,
                    retention_floor: self.retention_floor.lowest_pinned_sequence(),
                    document_versions,
                    index_versions,
                    version_counts: mvcc_version_counts(&schema, table_identity_versions),
                    retention_pins: self.retention_floor.snapshot(),
                    retention_gc,
                }))
            }
        }
    };
}

impl TenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "redb",
            TableBackendLayout::RedbKeyspaceByTableId,
            false,
            "configured_per_store",
        )
    }
}

impl SqliteTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "sqlite",
            TableBackendLayout::SharedDocumentsByTableId,
            false,
            if self.is_encrypted() {
                "sqlcipher"
            } else {
                "not_configured"
            },
        )
    }
}

impl PostgresTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "postgres",
            TableBackendLayout::SharedDocumentsByTableId,
            false,
            "server_managed",
        )
    }
}

impl MySqlTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "mysql",
            TableBackendLayout::SharedDocumentsByTableId,
            false,
            "server_managed",
        )
    }
}

impl LibsqlReplicaTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "libsql",
            TableBackendLayout::LibsqlReplicaSharedDocumentsByTableId,
            true,
            "replica_cache_optional",
        )
    }
}

impl_storage_health_diagnostic!(TenantStore);
impl_storage_health_diagnostic!(SqliteTenantStore);
impl_storage_health_diagnostic!(PostgresTenantStore);
impl_storage_health_diagnostic!(MySqlTenantStore);
impl_storage_health_diagnostic!(LibsqlReplicaTenantStore);

#[cfg(test)]
mod tests {
    use nimbus_core::{
        Document, FieldSchema, FieldType, IndexDefinition, IndexState, TableAccessPolicy,
        TableName, TableSchema,
    };
    use serde_json::json;

    use super::*;

    #[test]
    fn shared_journal_backend_feature_support_is_enterprise_complete() {
        let support = shared_journal_backend_feature_support();

        for feature in [
            StorageFeature::LatestReads,
            StorageFeature::RetentionGc,
            StorageFeature::OperatorDiagnostics,
            StorageFeature::HistoricalDocumentReads,
            StorageFeature::HistoricalIndexReads,
            StorageFeature::PointInTimeRestore,
            StorageFeature::Changefeed,
        ] {
            assert!(
                support.iter().any(|entry| entry.feature == feature
                    && entry.state == StorageFeatureSupportState::Supported),
                "{feature:?} should be supported by shared-journal backends"
            );
        }
        assert_eq!(
            capability_profile_for_features(&support),
            StorageCapabilityProfile::EnterpriseComplete
        );
    }

    #[test]
    fn storage_health_diagnostic_reports_backend_layout_and_heads() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let health = store
            .storage_health_diagnostic()
            .expect("diagnostic should load");
        assert_eq!(
            health.backend_layout,
            TableBackendLayout::RedbKeyspaceByTableId
        );
        assert_eq!(
            health.backend_capability_profile,
            StorageCapabilityProfile::EnterpriseComplete
        );
        assert_eq!(health.event_log_head, SequenceNumber(0));
        assert_eq!(health.applied_head, SequenceNumber(0));
        assert_eq!(health.format_version, CURRENT_STORAGE_FORMAT_VERSION);
        assert_eq!(health.document_versions.version_count, 0);
        assert_eq!(health.document_versions.min_sequence, None);
        assert_eq!(health.document_versions.max_sequence, None);
        assert_eq!(health.index_versions.version_count, 0);
        assert_eq!(health.mvcc.state, StorageOperatorState::Healthy);
        assert_eq!(health.storage_pressure.state, StoragePressureState::Nominal);
        assert!(health.backend_support.iter().any(|support| support.feature
            == StorageFeature::HistoricalDocumentReads
            && support.state == StorageFeatureSupportState::Supported));
        assert!(health.adapter_support.iter().any(|adapter| {
            adapter.adapter == "firebase"
                && adapter.capability_profile == StorageCapabilityProfile::LatestOnly
                && adapter.features.iter().any(|support| {
                    support.feature == StorageFeature::HistoricalDocumentReads
                        && support.state == StorageFeatureSupportState::Unsupported
                        && support.error_kind == Some(HistoricalReadErrorKind::UnsupportedAdapter)
                })
        }));
        assert!(health.adapter_support.iter().any(|adapter| {
            adapter.adapter == "native_http_websocket"
                && adapter.capability_profile == StorageCapabilityProfile::LatestOnly
                && adapter.features.iter().any(|support| {
                    support.feature == StorageFeature::PointInTimeRestore
                        && support.state == StorageFeatureSupportState::Unsupported
                        && support.error_kind == Some(HistoricalReadErrorKind::UnsupportedAdapter)
                })
        }));
        assert!(
            health
                .backend_support
                .iter()
                .all(|support| support.state != StorageFeatureSupportState::ExternalEvidencePending),
            "SEQ14 closeout diagnostics must not report stale external evidence pending states"
        );
    }

    #[test]
    fn storage_operator_diagnostics_cover_healthy_lagging_compacting_and_backend_divergence() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let schema = diagnostic_schema();
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let first = diagnostic_document(&schema.table, "first", 1);
        let insert = store
            .insert_with_indexes(&first, &schema.indexes)
            .expect("insert should commit");
        let updated_fields = serde_json::Map::from_iter([
            ("title".to_string(), json!("first-updated")),
            ("rank".to_string(), json!(2)),
        ]);
        let update = store
            .update_with_indexes(&schema.table, &first.id, &updated_fields, &schema.indexes)
            .expect("update should commit");
        let delete = store
            .delete(&schema.table, &first.id)
            .expect("delete should commit");

        let health = store
            .storage_health_diagnostic_with_retention_config(
                RetentionGcConfig::new(1).expect("retention config should parse"),
            )
            .expect("diagnostic should load");
        assert_eq!(health.event_log_head, delete.sequence);
        assert_eq!(health.document_versions.min_sequence, Some(insert.sequence));
        assert_eq!(health.document_versions.max_sequence, Some(delete.sequence));
        assert!(health.index_versions.version_count >= 2);
        assert_eq!(
            health.mvcc.version_counts.table_identity_versions, 1,
            "schema replacement should create one stable table identity"
        );
        assert_eq!(health.mvcc.version_counts.schema_versions, 1);
        assert_eq!(health.mvcc.version_counts.index_definition_versions, 1);
        assert_eq!(health.mvcc.version_counts.read_policy_versions, 1);
        assert_eq!(
            health.storage_pressure.state,
            StoragePressureState::CompactionRecommended
        );
        assert_eq!(health.mvcc.state, StorageOperatorState::Compacting);

        let lagging_pressure = storage_pressure_diagnostic(
            2,
            &health.document_versions,
            &health.index_versions,
            &health.retention_gc,
        );
        assert_eq!(lagging_pressure.state, StoragePressureState::ReplayLagging);
        assert_eq!(
            operator_state_for_pressure(lagging_pressure.state),
            StorageOperatorState::Lagging
        );

        let matched = BackendParityDiagnostic::compare(&health, &health);
        assert_eq!(matched.state, BackendParityState::Matched);
        let mut divergent = health.clone();
        divergent.document_versions.version_count =
            divergent.document_versions.version_count.saturating_add(1);
        divergent.mvcc.document_versions.version_count = divergent
            .mvcc
            .document_versions
            .version_count
            .saturating_add(1);
        let divergence = BackendParityDiagnostic::compare(&health, &divergent);
        assert_eq!(divergence.state, BackendParityState::BackendDivergence);
        assert_eq!(update.sequence.0, insert.sequence.0 + 1);
    }

    #[test]
    fn historical_query_admission_diagnostics_cover_expired_unsupported_format_and_policy_gates() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let table = TableName::new("admission_tasks").expect("table should parse");
        let first = diagnostic_document(&table, "first", 1);
        store.insert(&first).expect("insert should commit");
        let second = diagnostic_document(&table, "second", 2);
        let second_insert = store.insert(&second).expect("second insert should commit");
        let health = store
            .storage_health_diagnostic_with_retention_config(
                RetentionGcConfig::new(1).expect("retention config should parse"),
            )
            .expect("diagnostic should load");

        let expired = health.diagnose_historical_query_admission(
            HistoricalQueryAdmissionRequest::supported(SequenceNumber(0)),
        );
        assert_eq!(expired.state, HistoricalQueryAdmissionState::Expired);
        assert_eq!(
            expired.error_kind,
            Some(HistoricalReadErrorKind::RetentionExpired)
        );

        let unsupported =
            health.diagnose_historical_query_admission(HistoricalQueryAdmissionRequest {
                requested_sequence: second_insert.sequence,
                feature_supported: false,
                format_compatible: true,
                policy_allowed: true,
            });
        assert_eq!(
            unsupported.state,
            HistoricalQueryAdmissionState::Unsupported
        );
        assert_eq!(
            unsupported.error_kind,
            Some(HistoricalReadErrorKind::UnsupportedBackend)
        );

        let format_mismatch =
            health.diagnose_historical_query_admission(HistoricalQueryAdmissionRequest {
                requested_sequence: second_insert.sequence,
                feature_supported: true,
                format_compatible: false,
                policy_allowed: true,
            });
        assert_eq!(
            format_mismatch.state,
            HistoricalQueryAdmissionState::FormatMismatch
        );
        assert_eq!(
            format_mismatch.error_kind,
            Some(HistoricalReadErrorKind::FormatMismatch)
        );

        let policy_gated =
            health.diagnose_historical_query_admission(HistoricalQueryAdmissionRequest {
                requested_sequence: second_insert.sequence,
                feature_supported: true,
                format_compatible: true,
                policy_allowed: false,
            });
        assert_eq!(
            policy_gated.state,
            HistoricalQueryAdmissionState::PolicyGated
        );
        assert_eq!(
            policy_gated.error_kind,
            Some(HistoricalReadErrorKind::PolicySnapshotMissing)
        );
    }

    fn diagnostic_schema() -> TableSchema {
        TableSchema {
            table: TableName::new("diagnostic_tasks").expect("table should parse"),
            fields: vec![FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            }],
            indexes: vec![IndexDefinition {
                id: nimbus_core::IndexId::new(),
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
                state: IndexState::Enabled,
            }],
            access_policy: Some(TableAccessPolicy::default()),
        }
    }

    fn diagnostic_document(table: &TableName, title: &str, rank: u64) -> Document {
        Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!(title)),
                ("rank".to_string(), json!(rank)),
            ]),
        )
    }
}
