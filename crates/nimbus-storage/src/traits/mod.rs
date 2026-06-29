//! Focused storage capability traits.
//!
//! These traits are the MBA2 capability split over the current concrete store
//! families. They do not replace the async executor seam in `async_storage`;
//! that seam still owns blocking work and cancellation. The traits here make
//! backend support explicit so future providers can implement only the
//! capability families they actually support.
#![allow(async_fn_in_trait)]

mod kv;

use nimbus_core::{
    CommitEntry, Document, DocumentId, Filter, Result, SequenceNumber, TableName,
    TenantEventRecord, TenantId, Timestamp,
};
use nimbus_crypto::LocalKeyProvider;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::async_storage::{
    EmbeddedPersistenceProvider, EmbeddedRedbProvider, EmbeddedSqliteProvider,
    OpenedEmbeddedRedbTenant, OpenedEmbeddedSqliteTenant, UsageStorage,
};
use crate::changefeed::{ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage};
use crate::libsql::OpenedLibsqlReplicaTenant;
use crate::mysql::OpenedMySqlTenant;
use crate::postgres::OpenedPostgresTenant;
use crate::store::{DurableJournalBootstrap, DurableJournalPage, JournalProgress};
use crate::{
    IndexRangeBound, LibsqlReplicaProvider, LibsqlReplicaTenantStore, MySqlProvider,
    MySqlTenantStore, PostgresProvider, PostgresTenantStore, RedbUsageStorage, SqliteTenantStore,
    TenantStore,
};

pub use kv::{
    KvBatchOp, KvBatchOutcome, KvEntry, KvMutation, KvPut, KvScanPage, KvStorageEngine,
    KvSweepOutcome, TenantKvStore,
};

/// Reserved table where object manifests are stored.
///
/// The byte plane (`nimbus-blob`) never depends on storage; this table is the
/// named metadata plane consumed by the S3 surface and object filesystem binder.
pub const OBJECT_MANIFEST_TABLE: &str = "_nimbus_objects";

const OBJECT_FIELD_KEY: &str = "key";
const OBJECT_FIELD_SIZE: &str = "size";
const OBJECT_FIELD_CONTENT_TYPE: &str = "content_type";
const OBJECT_FIELD_USER_METADATA: &str = "user_metadata";
const OBJECT_FIELD_ETAG: &str = "etag";
const OBJECT_FIELD_BLOB_LAYOUT: &str = "blob_layout";

/// A blob reference inside an object manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectChunkRef {
    pub blob_hash: String,
    pub offset: u64,
    pub len: u64,
}

/// Object byte layout recorded in the metadata plane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectBlobLayout {
    Whole { blob_hash: String },
    Chunked { chunks: Vec<ObjectChunkRef> },
}

/// Protocol-neutral object manifest.
///
/// `key` is the S3/developer-visible object key. It is not used directly as a
/// `DocumentId`, because object keys may contain `/`; storage uses a stable
/// derived document id and stores the original key as data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectManifest {
    pub key: String,
    pub size: u64,
    pub blob_layout: ObjectBlobLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default)]
    pub user_metadata: Map<String, Value>,
    pub etag: String,
}

impl ObjectManifest {
    pub fn whole(
        key: impl Into<String>,
        size: u64,
        blob_hash: impl Into<String>,
        content_type: Option<String>,
        user_metadata: Map<String, Value>,
        etag: impl Into<String>,
    ) -> Result<Self> {
        let manifest = Self {
            key: key.into(),
            size,
            blob_layout: ObjectBlobLayout::Whole {
                blob_hash: blob_hash.into(),
            },
            content_type,
            user_metadata,
            etag: etag.into(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        validate_object_key(&self.key)?;
        if self.etag.is_empty() {
            return Err(nimbus_core::Error::InvalidInput(
                "object manifest etag cannot be empty".to_string(),
            ));
        }
        match &self.blob_layout {
            ObjectBlobLayout::Whole { blob_hash } if blob_hash.is_empty() => {
                Err(nimbus_core::Error::InvalidInput(
                    "object manifest whole blob hash cannot be empty".to_string(),
                ))
            }
            ObjectBlobLayout::Chunked { chunks } if chunks.is_empty() => {
                Err(nimbus_core::Error::InvalidInput(
                    "object manifest chunked layout cannot be empty".to_string(),
                ))
            }
            ObjectBlobLayout::Chunked { chunks } => {
                for chunk in chunks {
                    if chunk.blob_hash.is_empty() {
                        return Err(nimbus_core::Error::InvalidInput(
                            "object manifest chunk blob hash cannot be empty".to_string(),
                        ));
                    }
                    if chunk.len == 0 {
                        return Err(nimbus_core::Error::InvalidInput(
                            "object manifest chunk length cannot be zero".to_string(),
                        ));
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn document_id(&self) -> Result<DocumentId> {
        object_document_id(&self.key)
    }

    fn to_document(&self) -> Result<Document> {
        self.validate()?;
        let mut fields = Map::new();
        fields.insert(
            OBJECT_FIELD_KEY.to_string(),
            Value::String(self.key.clone()),
        );
        fields.insert(OBJECT_FIELD_SIZE.to_string(), json!(self.size));
        fields.insert(
            OBJECT_FIELD_CONTENT_TYPE.to_string(),
            self.content_type
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        fields.insert(
            OBJECT_FIELD_USER_METADATA.to_string(),
            Value::Object(self.user_metadata.clone()),
        );
        fields.insert(
            OBJECT_FIELD_ETAG.to_string(),
            Value::String(self.etag.clone()),
        );
        fields.insert(
            OBJECT_FIELD_BLOB_LAYOUT.to_string(),
            serde_json::to_value(&self.blob_layout).map_err(|err| {
                nimbus_core::Error::Serialization(format!("encode object blob layout: {err}"))
            })?,
        );
        Ok(Document::with_id(
            self.document_id()?,
            object_manifest_table()?,
            fields,
        ))
    }

    fn from_document(document: &Document) -> Result<Self> {
        let key = required_string(document, OBJECT_FIELD_KEY)?;
        let size = required_u64(document, OBJECT_FIELD_SIZE)?;
        let content_type = optional_string(document, OBJECT_FIELD_CONTENT_TYPE)?;
        let user_metadata = match document.fields.get(OBJECT_FIELD_USER_METADATA) {
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return Err(nimbus_core::Error::Serialization(
                    "object manifest user_metadata must be an object".to_string(),
                ));
            }
            None => Map::new(),
        };
        let etag = required_string(document, OBJECT_FIELD_ETAG)?;
        let layout_value = document
            .fields
            .get(OBJECT_FIELD_BLOB_LAYOUT)
            .ok_or_else(|| {
                nimbus_core::Error::Serialization("object manifest missing blob_layout".to_string())
            })?
            .clone();
        let blob_layout: ObjectBlobLayout =
            serde_json::from_value(layout_value).map_err(|err| {
                nimbus_core::Error::Serialization(format!("decode object blob layout: {err}"))
            })?;
        let manifest = Self {
            key,
            size,
            blob_layout,
            content_type,
            user_metadata,
            etag,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

/// Metadata-plane capability for named object manifests.
pub trait ObjectMetaStore {
    fn put_object_manifest(&self, manifest: &ObjectManifest) -> Result<CommitEntry>;
    fn get_object_manifest(&self, key: &str) -> Result<Option<ObjectManifest>>;
    fn delete_object_manifest(&self, key: &str) -> Result<Option<(CommitEntry, ObjectManifest)>>;
    fn list_object_manifests(&self, prefix: &str, limit: usize) -> Result<Vec<ObjectManifest>>;
}

/// Tenant lifecycle and discovery for provider families that can own tenants.
pub trait TenantLifecycle {
    type OpenedTenant;

    async fn list_tenants(&self) -> Result<Vec<TenantId>>;
    async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool>;
    async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant>;
    async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>>;
    async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()>;
}

/// Point document reads by table and document ID.
pub trait TenantPointRead {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>>;
}

/// Point document writes that commit through the backend's durable write path.
pub trait TenantPointWrite {
    fn insert_document(&self, document: &Document) -> Result<CommitEntry>;

    fn update_document_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &Map<String, Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static;

    fn delete_document_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static;
}

/// Table and index range reads used by the query planner.
pub trait TenantRangeScan {
    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>;

    fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;
}

/// Durable journal access used for recovery, subscriptions, and replication.
pub trait DurableJournal {
    fn journal_progress(&self) -> Result<JournalProgress>;
    fn read_durable_journal_from(&self, sequence: SequenceNumber)
    -> Result<Vec<TenantEventRecord>>;
    fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage>;
    fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap>;

    fn export_changefeed_bootstrap(&self) -> Result<ChangefeedBootstrap> {
        ChangefeedBootstrap::from_durable_bootstrap(self.export_durable_journal_bootstrap()?)
    }

    fn stream_changefeed(&self, cursor: &ChangefeedCursor, limit: usize) -> Result<ChangefeedPage> {
        cursor.rotate_handle(cursor.handle.clone())?;
        let page = self
            .stream_durable_journal(cursor.after, limit)
            .map_err(crate::changefeed::map_changefeed_journal_error)?;
        ChangefeedPage::from_durable_page(cursor.handle.clone(), page)
    }
}

/// Scheduler inspection capability for stores that own scheduled work.
pub trait SchedulerStore {
    fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool>;
    fn has_scheduled_work(&self) -> Result<bool>;
    fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>>;
}

fn validate_object_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(nimbus_core::Error::InvalidInput(
            "object key cannot be empty".to_string(),
        ));
    }
    if key.len() > 1_024 {
        return Err(nimbus_core::Error::InvalidInput(
            "object key cannot exceed 1024 bytes".to_string(),
        ));
    }
    if key.bytes().any(|byte| byte == 0) {
        return Err(nimbus_core::Error::InvalidInput(
            "object key cannot contain NUL bytes".to_string(),
        ));
    }
    Ok(())
}

fn object_manifest_table() -> Result<TableName> {
    TableName::new(OBJECT_MANIFEST_TABLE)
}

fn object_document_id(key: &str) -> Result<DocumentId> {
    validate_object_key(key)?;
    let digest = Sha256::digest(key.as_bytes());
    DocumentId::from_key(format!("object_{}", hex::encode(digest)))
}

fn required_string(document: &Document, field: &str) -> Result<String> {
    match document.fields.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(nimbus_core::Error::Serialization(format!(
            "object manifest field {field} must be a string"
        ))),
        None => Err(nimbus_core::Error::Serialization(format!(
            "object manifest missing field {field}"
        ))),
    }
}

fn optional_string(document: &Document, field: &str) -> Result<Option<String>> {
    match document.fields.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(nimbus_core::Error::Serialization(format!(
            "object manifest field {field} must be a string or null"
        ))),
    }
}

fn required_u64(document: &Document, field: &str) -> Result<u64> {
    match document.fields.get(field).and_then(Value::as_u64) {
        Some(value) => Ok(value),
        None => Err(nimbus_core::Error::Serialization(format!(
            "object manifest field {field} must be an unsigned integer"
        ))),
    }
}

fn put_object_manifest_for_store<S>(store: &S, manifest: &ObjectManifest) -> Result<CommitEntry>
where
    S: TenantPointRead + TenantPointWrite,
{
    let document = manifest.to_document()?;
    if store.get(&document.table, &document.id)?.is_some() {
        store.update_document_validated(&document.table, &document.id, &document.fields, |_, _| {
            Ok(())
        })
    } else {
        store.insert_document(&document)
    }
}

fn get_object_manifest_for_store<S>(store: &S, key: &str) -> Result<Option<ObjectManifest>>
where
    S: TenantPointRead,
{
    let table = object_manifest_table()?;
    let id = object_document_id(key)?;
    store
        .get(&table, &id)?
        .as_ref()
        .map(ObjectManifest::from_document)
        .transpose()
}

fn delete_object_manifest_for_store<S>(
    store: &S,
    key: &str,
) -> Result<Option<(CommitEntry, ObjectManifest)>>
where
    S: TenantPointRead + TenantPointWrite,
{
    let table = object_manifest_table()?;
    let id = object_document_id(key)?;
    let Some(existing) = store.get(&table, &id)? else {
        return Ok(None);
    };
    let manifest = ObjectManifest::from_document(&existing)?;
    let (commit, _) = store.delete_document_validated(&table, &id, |_| Ok(()))?;
    Ok(Some((commit, manifest)))
}

fn list_object_manifests_for_store<S>(
    store: &S,
    prefix: &str,
    limit: usize,
) -> Result<Vec<ObjectManifest>>
where
    S: TenantRangeScan,
{
    let table = object_manifest_table()?;
    let mut check_cancel = || Ok(());
    let mut manifests = store
        .scan_table_matching_with_filters_cancellable(&table, &[], &mut check_cancel, |document| {
            match document.fields.get(OBJECT_FIELD_KEY) {
                Some(Value::String(key)) => Ok(key.starts_with(prefix)),
                _ => Ok(false),
            }
        })?
        .iter()
        .map(ObjectManifest::from_document)
        .collect::<Result<Vec<_>>>()?;
    manifests.sort_by(|left, right| left.key.cmp(&right.key));
    if manifests.len() > limit {
        manifests.truncate(limit);
    }
    Ok(manifests)
}

/// Control-plane usage storage.
pub trait ControlPlaneUsage: UsageStorage {}

/// Local database key-provider capability.
pub trait KeyProviderSurface: LocalKeyProvider {}

/// Composite convenience trait for tenant data stores that support the core
/// engine read, write, journal, and scheduler capabilities.
pub trait StorageEngine:
    TenantPointRead
    + TenantPointWrite
    + TenantRangeScan
    + DurableJournal
    + SchedulerStore
    + ObjectMetaStore
{
}

impl TenantLifecycle for EmbeddedRedbProvider {
    type OpenedTenant = OpenedEmbeddedRedbTenant;

    async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        <Self as EmbeddedPersistenceProvider>::list_tenants(self).await
    }

    async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        Self::tenant_exists(self, tenant_id).await
    }

    async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        Self::create_tenant(self, tenant_id).await
    }

    async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        Self::open_existing_tenant(self, tenant_id).await
    }

    async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        Self::delete_tenant(self, tenant_id).await
    }
}

impl TenantLifecycle for EmbeddedSqliteProvider {
    type OpenedTenant = OpenedEmbeddedSqliteTenant;

    async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        Self::list_tenants(self).await
    }

    async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        Self::tenant_exists(self, tenant_id).await
    }

    async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        Self::create_tenant(self, tenant_id).await
    }

    async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        Self::open_existing_tenant(self, tenant_id).await
    }

    async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        Self::delete_tenant(self, tenant_id).await
    }
}

macro_rules! impl_provider_lifecycle {
    ($provider:ty, $opened:ty) => {
        impl TenantLifecycle for $provider {
            type OpenedTenant = $opened;

            async fn list_tenants(&self) -> Result<Vec<TenantId>> {
                <$provider>::list_tenants(self).await
            }

            async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
                <$provider>::tenant_exists(self, tenant_id).await
            }

            async fn create_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
                <$provider>::create_opened_tenant(self, tenant_id).await
            }

            async fn open_existing_tenant(
                &self,
                tenant_id: &TenantId,
            ) -> Result<Option<Self::OpenedTenant>> {
                <$provider>::open_existing_opened_tenant(self, tenant_id).await
            }

            async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
                <$provider>::delete_tenant(self, tenant_id).await
            }
        }
    };
}

impl_provider_lifecycle!(PostgresProvider, OpenedPostgresTenant);
impl_provider_lifecycle!(MySqlProvider, OpenedMySqlTenant);
impl_provider_lifecycle!(LibsqlReplicaProvider, OpenedLibsqlReplicaTenant);

macro_rules! impl_point_read {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TenantPointRead for $ty {
                fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
                    <$ty>::get(self, table, id)
                }
            }
        )+
    };
}

impl_point_read!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_point_write {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TenantPointWrite for $ty {
                fn insert_document(&self, document: &Document) -> Result<CommitEntry> {
                    <$ty>::insert(self, document)
                }

                fn update_document_validated<F>(
                    &self,
                    table: &TableName,
                    id: &DocumentId,
                    patch: &Map<String, Value>,
                    validate: F,
                ) -> Result<CommitEntry>
                where
                    F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
                {
                    <$ty>::update_validated(self, table, id, patch, validate)
                }

                fn delete_document_validated<F>(
                    &self,
                    table: &TableName,
                    id: &DocumentId,
                    validate: F,
                ) -> Result<(CommitEntry, Document)>
                where
                    F: FnOnce(&Document) -> Result<()> + Send + 'static,
                {
                    <$ty>::delete_validated_returning_document(self, table, id, validate)
                }
            }
        )+
    };
}

impl_point_write!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_range_scan {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TenantRangeScan for $ty {
                fn scan_table_matching_with_filters_cancellable<F>(
                    &self,
                    table: &TableName,
                    filters: &[Filter],
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                    include_document: F,
                ) -> Result<Vec<Document>>
                where
                    F: FnMut(&Document) -> Result<bool>,
                {
                    <$ty>::scan_table_matching_with_filters_cancellable(
                        self,
                        table,
                        filters,
                        check_cancel,
                        include_document,
                    )
                }

                fn scan_table_id_prefix_cancellable(
                    &self,
                    table: &TableName,
                    id_prefix: &str,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::scan_table_id_prefix_cancellable(self, table, id_prefix, check_cancel)
                }

                fn scan_table_id_starting_at_cancellable(
                    &self,
                    table: &TableName,
                    start_id: &str,
                    limit: usize,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::scan_table_id_starting_at_cancellable(
                        self,
                        table,
                        start_id,
                        limit,
                        check_cancel,
                    )
                }

                fn index_scan_eq_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    value: &Value,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
                }

                fn index_scan_prefix_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    prefix_values: &[Value],
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_prefix_cancellable(
                        self,
                        table,
                        index_name,
                        prefix_values,
                        check_cancel,
                    )
                }

                fn index_scan_range_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    start: IndexRangeBound<'_>,
                    end: IndexRangeBound<'_>,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_range_cancellable(
                        self,
                        table,
                        index_name,
                        start,
                        end,
                        check_cancel,
                    )
                }

                fn index_scan_composite_range_cancellable(
                    &self,
                    table: &TableName,
                    index_name: &str,
                    exact_prefix: &[Value],
                    start: IndexRangeBound<'_>,
                    end: IndexRangeBound<'_>,
                    check_cancel: &mut dyn FnMut() -> Result<()>,
                ) -> Result<Vec<Document>> {
                    <$ty>::index_scan_composite_range_cancellable(
                        self,
                        table,
                        index_name,
                        exact_prefix,
                        start,
                        end,
                        check_cancel,
                    )
                }
            }
        )+
    };
}

impl_range_scan!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_durable_journal {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl DurableJournal for $ty {
                fn journal_progress(&self) -> Result<JournalProgress> {
                    <$ty>::journal_progress(self)
                }

                fn read_durable_journal_from(
                    &self,
                    sequence: SequenceNumber,
                ) -> Result<Vec<TenantEventRecord>> {
                    <$ty>::read_durable_journal_from(self, sequence)
                }

                fn stream_durable_journal(
                    &self,
                    after: SequenceNumber,
                    limit: usize,
                ) -> Result<DurableJournalPage> {
                    <$ty>::stream_durable_journal(self, after, limit)
                }

                fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
                    <$ty>::export_durable_journal_bootstrap(self)
                }
            }
        )+
    };
}

impl_durable_journal!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_scheduler_store {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl SchedulerStore for $ty {
                fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
                    <$ty>::scheduled_execution_exists(self, execution_id)
                }

                fn has_scheduled_work(&self) -> Result<bool> {
                    <$ty>::has_scheduled_work(self)
                }

                fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
                    <$ty>::next_scheduled_work_at(self)
                }
            }
        )+
    };
}

impl_scheduler_store!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

macro_rules! impl_object_meta_store {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl ObjectMetaStore for $ty {
                fn put_object_manifest(&self, manifest: &ObjectManifest) -> Result<CommitEntry> {
                    put_object_manifest_for_store(self, manifest)
                }

                fn get_object_manifest(&self, key: &str) -> Result<Option<ObjectManifest>> {
                    get_object_manifest_for_store(self, key)
                }

                fn delete_object_manifest(
                    &self,
                    key: &str,
                ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
                    delete_object_manifest_for_store(self, key)
                }

                fn list_object_manifests(
                    &self,
                    prefix: &str,
                    limit: usize,
                ) -> Result<Vec<ObjectManifest>> {
                    list_object_manifests_for_store(self, prefix, limit)
                }
            }
        )+
    };
}

impl_object_meta_store!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);

impl ControlPlaneUsage for RedbUsageStorage {}

impl KeyProviderSurface for nimbus_crypto::MasterKeyFileProvider {}
impl KeyProviderSurface for nimbus_crypto::KeyDirectoryProvider {}
#[cfg(feature = "aws-kms")]
impl KeyProviderSurface for nimbus_crypto::AwsKmsKeyProvider {}

/// StorageEngine includes ObjectMetaStore so object manifests use the same stores.
impl<T> StorageEngine for T where
    T: TenantPointRead
        + TenantPointWrite
        + TenantRangeScan
        + DurableJournal
        + SchedulerStore
        + ObjectMetaStore
{
}
