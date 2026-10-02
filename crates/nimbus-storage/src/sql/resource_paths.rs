//! Dialect-shared resource path bindings.
//!
//! A binding maps a document locator to a document path. The binding policy
//! (one locator per path, idempotent upsert), the blob codecs, and the lookup
//! statements are the same for PostgreSQL and MySQL, so they live here once.
//! The providers differ in how they key the table:
//!
//! - **PostgreSQL** indexes the raw locator, path, and collection group keys.
//! - **MySQL** cannot index keys of unbounded length, so it indexes a SHA-256
//!   hash of each key and stores the raw key beside it. A lookup by hash then
//!   checks the stored key to detect a collision.
//!
//! [`SqlResourcePathSession`] carries that keying difference.

use std::borrow::Cow;
use std::future::Future;

use nimbus_core::{
    CollectionName, DocumentLocator, DocumentPath, Error, ResourcePathBinding, Result,
};

use crate::keys::{document_path_key, resource_locator_key};
use crate::sql::dialect::{
    Dialect, SqlRow, SqlSession, SqlSessionStore, SqlSessionTransaction, SqlValue,
};
use crate::sql::store_core::SqlStoreCore;
use crate::sql::write_core::SqlWriteBackend;

const BINDINGS_TABLE: &str = "resource_path_bindings";

/// The encoded columns of one binding row.
pub(crate) struct ResourcePathRow<'a> {
    pub(crate) locator_key: &'a [u8],
    pub(crate) document_path_key: &'a [u8],
    pub(crate) collection_group: &'a str,
    pub(crate) binding_blob: &'a [u8],
    pub(crate) locator_blob: &'a [u8],
}

/// Session hook for the provider-owned keying of `resource_path_bindings`.
pub(crate) trait SqlResourcePathSession: SqlSession {
    /// Indexed column that finds a row by locator key.
    const LOCATOR_LOOKUP_COLUMN: &'static str;
    /// Indexed column that finds a row by document path key.
    const PATH_LOOKUP_COLUMN: &'static str;

    /// Value stored in a lookup column for `key`.
    fn lookup_key(key: &[u8]) -> Cow<'_, [u8]>;

    /// Insert or replace the row for `row.locator_key`.
    fn upsert_binding_row(
        &mut self,
        tenant_schema: &str,
        row: ResourcePathRow<'_>,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Rows whose first column is the binding blob of a binding that may
    /// belong to `collection_group`. The caller filters and orders them.
    fn collection_group_binding_rows(
        &mut self,
        tenant_schema: &str,
        collection_group: &CollectionName,
    ) -> impl Future<Output = Result<Vec<Self::Row>>> + Send;
}

/// Store-level resource path operations.
///
/// As elsewhere in [`crate::sql`], a default method here shares a name with the
/// inherent method that [`sql_resource_path_facade`] generates. Inherent
/// methods win method-call resolution, so the facade is not recursive.
pub(crate) trait SqlResourcePathStore: SqlStoreCore + SqlSessionStore {
    fn upsert_resource_path_binding(&self, binding: &ResourcePathBinding) -> Result<()> {
        let binding = binding.clone();
        self.execute_write(move |transaction| transaction.upsert_resource_path_binding(&binding))?;
        Ok(())
    }

    fn remove_resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        let locator = locator.clone();
        Ok(self
            .execute_write(move |transaction| transaction.remove_resource_path_binding(&locator))?
            .value)
    }

    fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let locator_key = resource_locator_key(locator);
        self.block_on_read(async move {
            load_binding_by_locator_key(&mut session.await?, &tenant_schema, &locator_key).await
        })
    }

    fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let path_key = document_path_key(document_path);
        self.block_on_read(async move {
            load_locator_by_path_key(&mut session.await?, &tenant_schema, &path_key).await
        })
    }

    fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let collection_group = collection_group.clone();
        self.block_on_read(async move {
            load_collection_group_bindings(&mut session.await?, &tenant_schema, &collection_group)
                .await
        })
    }
}

/// Load the binding of `locator` inside the open write transaction.
pub(crate) fn sql_resource_path_binding<T: SqlWriteBackend + SqlSessionTransaction>(
    transaction: &mut T,
    locator: &DocumentLocator,
) -> Result<Option<ResourcePathBinding>> {
    transaction.check_cancel()?;
    let locator_key = resource_locator_key(locator);
    let (mut session, tenant_schema, runtime_handle) = transaction.session_parts()?;
    T::block_on_session(&runtime_handle, async move {
        load_binding_by_locator_key(&mut session, &tenant_schema, &locator_key).await
    })
}

/// Bind `binding` inside the open write transaction.
pub(crate) fn sql_upsert_resource_path_binding<T: SqlWriteBackend + SqlSessionTransaction>(
    transaction: &mut T,
    binding: &ResourcePathBinding,
) -> Result<()> {
    transaction.check_cancel()?;
    let (mut session, tenant_schema, runtime_handle) = transaction.session_parts()?;
    T::block_on_session(&runtime_handle, async move {
        upsert_resource_path_binding_in_session(&mut session, &tenant_schema, binding).await
    })
}

/// Unbind `locator` inside the open write transaction and return the binding
/// it had.
pub(crate) fn sql_remove_resource_path_binding<T: SqlWriteBackend + SqlSessionTransaction>(
    transaction: &mut T,
    locator: &DocumentLocator,
) -> Result<Option<ResourcePathBinding>> {
    transaction.check_cancel()?;
    let locator_key = resource_locator_key(locator);
    let (mut session, tenant_schema, runtime_handle) = transaction.session_parts()?;
    T::block_on_session(&runtime_handle, async move {
        let existing =
            load_binding_by_locator_key(&mut session, &tenant_schema, &locator_key).await?;
        if existing.is_some() {
            delete_binding_row(&mut session, &tenant_schema, &locator_key).await?;
        }
        Ok(existing)
    })
}

pub(crate) async fn upsert_resource_path_binding_in_session<S: SqlResourcePathSession>(
    session: &mut S,
    tenant_schema: &str,
    binding: &ResourcePathBinding,
) -> Result<()> {
    let locator_key = resource_locator_key(&binding.locator);
    let path_key = document_path_key(&binding.document_path);
    let existing_locator = load_locator_by_path_key(session, tenant_schema, &path_key).await?;
    if existing_locator
        .as_ref()
        .is_some_and(|locator| locator != &binding.locator)
    {
        return Err(Error::AlreadyExists(format!(
            "document path already bound: {}",
            binding.document_path
        )));
    }
    if load_binding_by_locator_key(session, tenant_schema, &locator_key)
        .await?
        .as_ref()
        == Some(binding)
    {
        return Ok(());
    }

    let binding_blob = encode_blob(binding)?;
    let locator_blob = encode_blob(&binding.locator)?;
    session
        .upsert_binding_row(
            tenant_schema,
            ResourcePathRow {
                locator_key: &locator_key,
                document_path_key: &path_key,
                collection_group: binding.collection_group().as_str(),
                binding_blob: &binding_blob,
                locator_blob: &locator_blob,
            },
        )
        .await
}

pub(crate) async fn remove_resource_path_binding_in_session<S: SqlResourcePathSession>(
    session: &mut S,
    tenant_schema: &str,
    locator: &DocumentLocator,
) -> Result<()> {
    delete_binding_row(session, tenant_schema, &resource_locator_key(locator)).await
}

/// Every binding of the tenant, ordered by document path key.
pub(crate) async fn load_resource_path_bindings_from_session<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
) -> Result<Vec<ResourcePathBinding>> {
    let query = format!(
        "SELECT binding_blob FROM {}",
        S::table(tenant_schema, BINDINGS_TABLE)
    );
    let rows = session.fetch_all(&query, &[]).await?;
    decode_bindings_by_path(rows, |_| true)
}

async fn delete_binding_row<S: SqlResourcePathSession>(
    session: &mut S,
    tenant_schema: &str,
    locator_key: &[u8],
) -> Result<()> {
    let query = format!(
        "DELETE FROM {} WHERE {} = ?",
        S::table(tenant_schema, BINDINGS_TABLE),
        S::LOCATOR_LOOKUP_COLUMN
    );
    session
        .execute_sql(&query, &[SqlValue::Bytes(&S::lookup_key(locator_key))])
        .await?;
    Ok(())
}

async fn load_binding_by_locator_key<S: SqlResourcePathSession>(
    session: &mut S,
    tenant_schema: &str,
    locator_key: &[u8],
) -> Result<Option<ResourcePathBinding>> {
    let query = format!(
        "SELECT locator_key, binding_blob FROM {} WHERE {} = ?",
        S::table(tenant_schema, BINDINGS_TABLE),
        S::LOCATOR_LOOKUP_COLUMN
    );
    let Some(row) = session
        .fetch_optional(&query, &[SqlValue::Bytes(&S::lookup_key(locator_key))])
        .await?
    else {
        return Ok(None);
    };
    if row.bytes(0)? != locator_key {
        return Err(Error::Internal(format!(
            "{} resource locator hash collision while loading path binding",
            S::Dialect::NAME
        )));
    }
    decode_blob(&row.bytes(1)?).map(Some)
}

async fn load_locator_by_path_key<S: SqlResourcePathSession>(
    session: &mut S,
    tenant_schema: &str,
    path_key: &[u8],
) -> Result<Option<DocumentLocator>> {
    let query = format!(
        "SELECT document_path_key, locator_blob FROM {} WHERE {} = ?",
        S::table(tenant_schema, BINDINGS_TABLE),
        S::PATH_LOOKUP_COLUMN
    );
    let Some(row) = session
        .fetch_optional(&query, &[SqlValue::Bytes(&S::lookup_key(path_key))])
        .await?
    else {
        return Ok(None);
    };
    if row.bytes(0)? != path_key {
        return Err(Error::Internal(format!(
            "{} document path hash collision while loading resource locator",
            S::Dialect::NAME
        )));
    }
    decode_blob(&row.bytes(1)?).map(Some)
}

async fn load_collection_group_bindings<S: SqlResourcePathSession>(
    session: &mut S,
    tenant_schema: &str,
    collection_group: &CollectionName,
) -> Result<Vec<ResourcePathBinding>> {
    let rows = session
        .collection_group_binding_rows(tenant_schema, collection_group)
        .await?;
    decode_bindings_by_path(rows, |binding| {
        binding.collection_group() == collection_group
    })
}

fn decode_bindings_by_path<R: SqlRow>(
    rows: Vec<R>,
    keep: impl Fn(&ResourcePathBinding) -> bool,
) -> Result<Vec<ResourcePathBinding>> {
    let mut bindings = Vec::with_capacity(rows.len());
    for row in rows {
        let binding: ResourcePathBinding = decode_blob(&row.bytes(0)?)?;
        if keep(&binding) {
            bindings.push(binding);
        }
    }
    bindings.sort_by_key(|binding| document_path_key(&binding.document_path));
    Ok(bindings)
}

fn encode_blob<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    rmp_serde::to_vec(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn decode_blob<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))
}

/// Re-exposes the [`SqlResourcePathStore`] entry points and the transaction
/// functions above as inherent methods, keeping the public API of each store
/// and write transaction unchanged.
macro_rules! sql_resource_path_facade {
    ($store:ty, $transaction:ty) => {
        impl $store {
            pub fn upsert_resource_path_binding(
                &self,
                binding: &nimbus_core::ResourcePathBinding,
            ) -> nimbus_core::Result<()> {
                <Self as crate::sql::resource_paths::SqlResourcePathStore>::upsert_resource_path_binding(self, binding)
            }

            pub fn remove_resource_path_binding(
                &self,
                locator: &nimbus_core::DocumentLocator,
            ) -> nimbus_core::Result<Option<nimbus_core::ResourcePathBinding>> {
                <Self as crate::sql::resource_paths::SqlResourcePathStore>::remove_resource_path_binding(self, locator)
            }

            pub fn resource_path_binding(
                &self,
                locator: &nimbus_core::DocumentLocator,
            ) -> nimbus_core::Result<Option<nimbus_core::ResourcePathBinding>> {
                <Self as crate::sql::resource_paths::SqlResourcePathStore>::resource_path_binding(self, locator)
            }

            pub fn locator_for_document_path(
                &self,
                document_path: &nimbus_core::DocumentPath,
            ) -> nimbus_core::Result<Option<nimbus_core::DocumentLocator>> {
                <Self as crate::sql::resource_paths::SqlResourcePathStore>::locator_for_document_path(self, document_path)
            }

            pub fn scan_collection_group_bindings(
                &self,
                collection_group: &nimbus_core::CollectionName,
            ) -> nimbus_core::Result<Vec<nimbus_core::ResourcePathBinding>> {
                <Self as crate::sql::resource_paths::SqlResourcePathStore>::scan_collection_group_bindings(self, collection_group)
            }
        }

        impl $transaction {
            pub fn resource_path_binding(
                &mut self,
                locator: &nimbus_core::DocumentLocator,
            ) -> nimbus_core::Result<Option<nimbus_core::ResourcePathBinding>> {
                crate::sql::resource_paths::sql_resource_path_binding(self, locator)
            }

            pub fn upsert_resource_path_binding(
                &mut self,
                binding: &nimbus_core::ResourcePathBinding,
            ) -> nimbus_core::Result<()> {
                crate::sql::resource_paths::sql_upsert_resource_path_binding(self, binding)
            }

            pub fn remove_resource_path_binding(
                &mut self,
                locator: &nimbus_core::DocumentLocator,
            ) -> nimbus_core::Result<Option<nimbus_core::ResourcePathBinding>> {
                crate::sql::resource_paths::sql_remove_resource_path_binding(self, locator)
            }
        }
    };
}

pub(crate) use sql_resource_path_facade;
