use std::cmp::Ordering;

use neovex_core::{Document, Result, TableName};
use redb::{ReadTransaction, TableError};
use serde_json::Value;

use crate::keys::{document_key, prefix_end};
use crate::store::{DOCUMENTS, INDEXES, TenantReadSnapshot, TenantStore, map_redb_error};

use super::bounds::composite_range_scan_bounds;
use super::encoding::{encode_index_tuple, encode_index_value};
use super::keyspace::{
    doc_id_from_index_key, encoded_value_from_index_key, index_prefix, index_value_prefix,
};

fn decode_document(bytes: &[u8]) -> Result<Document> {
    Document::from_msgpack(bytes)
        .map_err(|error| neovex_core::Error::Serialization(error.to_string()))
}

fn scan_documents_for_index_key_bounds_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    match_prefix: &[u8],
    start_key: &[u8],
    end_key: Option<&[u8]>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let index_table = match read_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let documents_table = match read_txn.open_table(DOCUMENTS) {
        Ok(documents_table) => documents_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let mut documents = Vec::new();
    if let Some(end_key) = end_key {
        for item in index_table
            .range(start_key..end_key)
            .map_err(map_redb_error)?
        {
            check_cancel()?;
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(match_prefix) {
                break;
            }
            let doc_id = doc_id_from_index_key(key.value());
            let doc_key = document_key(table, &doc_id);
            if let Some(value) = documents_table
                .get(doc_key.as_slice())
                .map_err(map_redb_error)?
            {
                documents.push(decode_document(value.value())?);
            }
        }
    } else {
        for item in index_table.range(start_key..).map_err(map_redb_error)? {
            check_cancel()?;
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(match_prefix) {
                break;
            }
            let doc_id = doc_id_from_index_key(key.value());
            let doc_key = document_key(table, &doc_id);
            if let Some(value) = documents_table
                .get(doc_key.as_slice())
                .map_err(map_redb_error)?
            {
                documents.push(decode_document(value.value())?);
            }
        }
    }
    Ok(documents)
}

fn scan_documents_for_index_key_bounds(
    store: &TenantStore,
    table: &TableName,
    match_prefix: &[u8],
    start_key: &[u8],
    end_key: Option<&[u8]>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let read_txn = store.db.begin_read().map_err(map_redb_error)?;
    scan_documents_for_index_key_bounds_in_read_txn(
        &read_txn,
        table,
        match_prefix,
        start_key,
        end_key,
        check_cancel,
    )
}

fn index_scan_eq_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    value: &Value,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let index_table = match read_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let documents_table = match read_txn.open_table(DOCUMENTS) {
        Ok(documents_table) => documents_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let encoded = encode_index_value(value)?;
    let prefix = index_value_prefix(table, index_name, &encoded);
    let mut documents = Vec::new();
    match prefix_end(&prefix) {
        Some(end) => {
            for item in index_table
                .range(prefix.as_slice()..end.as_slice())
                .map_err(map_redb_error)?
            {
                check_cancel()?;
                let (key, _) = item.map_err(map_redb_error)?;
                let doc_id = doc_id_from_index_key(key.value());
                let doc_key = document_key(table, &doc_id);
                if let Some(value) = documents_table
                    .get(doc_key.as_slice())
                    .map_err(map_redb_error)?
                {
                    documents.push(decode_document(value.value())?);
                }
            }
        }
        None => {
            for item in index_table
                .range(prefix.as_slice()..)
                .map_err(map_redb_error)?
            {
                check_cancel()?;
                let (key, _) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                let doc_id = doc_id_from_index_key(key.value());
                let doc_key = document_key(table, &doc_id);
                if let Some(value) = documents_table
                    .get(doc_key.as_slice())
                    .map_err(map_redb_error)?
                {
                    documents.push(decode_document(value.value())?);
                }
            }
        }
    }
    Ok(documents)
}

#[allow(clippy::too_many_arguments)]
fn index_scan_range_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let index_table = match read_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let documents_table = match read_txn.open_table(DOCUMENTS) {
        Ok(documents_table) => documents_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let prefix = index_prefix(table, index_name);
    let prefix_len = prefix.len();
    let start = start.map(encode_index_value).transpose()?;
    let end = end.map(encode_index_value).transpose()?;

    let mut documents = Vec::new();
    for item in index_table
        .range(prefix.as_slice()..)
        .map_err(map_redb_error)?
    {
        check_cancel()?;
        let (key, _) = item.map_err(map_redb_error)?;
        if !key.value().starts_with(&prefix) {
            break;
        }
        let encoded_value = encoded_value_from_index_key(key.value(), prefix_len);
        if let Some(start) = start.as_ref() {
            match encoded_value.cmp(start.as_slice()) {
                Ordering::Less => continue,
                Ordering::Equal if !start_inclusive => continue,
                Ordering::Equal | Ordering::Greater => {}
            }
        }
        if let Some(end) = end.as_ref() {
            match encoded_value.cmp(end.as_slice()) {
                Ordering::Greater => continue,
                Ordering::Equal if !end_inclusive => continue,
                Ordering::Equal | Ordering::Less => {}
            }
        }

        let doc_id = doc_id_from_index_key(key.value());
        let doc_key = document_key(table, &doc_id);
        if let Some(value) = documents_table
            .get(doc_key.as_slice())
            .map_err(map_redb_error)?
        {
            documents.push(decode_document(value.value())?);
        }
    }
    Ok(documents)
}

impl TenantStore {
    /// Returns documents whose indexed field equals the provided value.
    pub fn index_scan_eq(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
    ) -> Result<Vec<Document>> {
        self.index_scan_eq_cancellable(table, index_name, value, &mut || Ok(()))
    }

    /// Returns documents whose indexed field equals the provided value, checking for cancellation between rows.
    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_eq_in_read_txn(&read_txn, table, index_name, value, check_cancel)
    }

    /// Returns documents whose indexed field falls within the provided range.
    pub fn index_scan_range(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Vec<Document>> {
        self.index_scan_range_cancellable(
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            &mut || Ok(()),
        )
    }

    /// Returns documents whose indexed field falls within the provided range, checking for cancellation between rows.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_range_in_read_txn(
            &read_txn,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    /// Returns documents whose indexed tuple matches the provided exact leading prefix.
    pub fn index_scan_prefix(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(table, index_name, prefix_values, &mut || Ok(()))
    }

    /// Returns documents whose indexed tuple matches the provided exact leading prefix, checking for cancellation between rows.
    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let encoded_prefix = encode_index_tuple(prefix_values)?;
        let match_prefix = index_value_prefix(table, index_name, &encoded_prefix);
        let end_key = prefix_end(&match_prefix);
        scan_documents_for_index_key_bounds(
            self,
            table,
            &match_prefix,
            &match_prefix,
            end_key.as_deref(),
            check_cancel,
        )
    }

    /// Returns documents whose composite index matches an exact leading prefix and one range on the next field.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Vec<Document>> {
        self.index_scan_composite_range_cancellable(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            &mut || Ok(()),
        )
    }

    /// Returns documents whose composite index matches an exact leading prefix and one range on the next field, checking for cancellation between rows.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let (match_prefix, start_key, end_key) = composite_range_scan_bounds(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
        )?;
        if start_key.is_empty() {
            return Ok(Vec::new());
        }

        scan_documents_for_index_key_bounds(
            self,
            table,
            &match_prefix,
            &start_key,
            end_key.as_deref(),
            check_cancel,
        )
    }
}

impl TenantReadSnapshot {
    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        index_scan_eq_in_read_txn(&self.read_txn, table, index_name, value, check_cancel)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        index_scan_range_in_read_txn(
            &self.read_txn,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let encoded_prefix = encode_index_tuple(prefix_values)?;
        let match_prefix = index_value_prefix(table, index_name, &encoded_prefix);
        let end_key = prefix_end(&match_prefix);
        scan_documents_for_index_key_bounds_in_read_txn(
            &self.read_txn,
            table,
            &match_prefix,
            &match_prefix,
            end_key.as_deref(),
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let (match_prefix, start_key, end_key) = composite_range_scan_bounds(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
        )?;
        if start_key.is_empty() {
            return Ok(Vec::new());
        }

        scan_documents_for_index_key_bounds_in_read_txn(
            &self.read_txn,
            table,
            &match_prefix,
            &start_key,
            end_key.as_deref(),
            check_cancel,
        )
    }
}
