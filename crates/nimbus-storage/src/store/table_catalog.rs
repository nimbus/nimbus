use nimbus_core::{Error, Result, TableId, TableName, TableState};
use redb::{ReadableTable, TableError};
use serde::{Deserialize, Serialize};

use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, TableIdentitySnapshotEntry, deleting_table_namespace,
    hidden_table_namespace,
};

use super::{TABLE_CATALOG, map_redb_error};

fn catalog_key(namespace: &str, table: &TableName) -> String {
    format!("{namespace}\0{}", table.as_str())
}

fn default_catalog_key(table: &TableName) -> String {
    catalog_key(DEFAULT_TABLE_NAMESPACE, table)
}

fn parse_catalog_key(key: &str) -> Result<(String, TableName)> {
    let Some((namespace, table_name)) = key.split_once('\0') else {
        return Err(nimbus_core::Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            format!("malformed table catalog key: {key:?}"),
        ));
    };
    Ok((namespace.to_string(), TableName::new(table_name)?))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TableCatalogValue {
    table_id: TableId,
    #[serde(default)]
    state: TableState,
}

impl TableCatalogValue {
    fn active(table_id: TableId) -> Self {
        Self {
            table_id,
            state: TableState::Active,
        }
    }
}

fn encode_catalog_value(value: &TableCatalogValue) -> Result<String> {
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn decode_catalog_value(value: &str) -> Result<TableCatalogValue> {
    serde_json::from_str(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn active_table_id(table: &TableName, value: TableCatalogValue) -> Result<Option<TableId>> {
    match value.state {
        TableState::Active => Ok(Some(value.table_id)),
        TableState::Hidden | TableState::Deleting => Err(Error::Conflict(format!(
            "logical table {} is in {} lifecycle state",
            table, value.state
        ))),
    }
}

fn find_catalog_entry_by_table_id_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table_id: &TableId,
) -> Result<Option<(String, TableName, TableCatalogValue)>> {
    let catalog = match write_txn.open_table(TABLE_CATALOG) {
        Ok(catalog) => catalog,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };

    for item in catalog.iter().map_err(map_redb_error)? {
        let (key, value) = item.map_err(map_redb_error)?;
        let value = decode_catalog_value(value.value())?;
        if &value.table_id == table_id {
            let (namespace, table) = parse_catalog_key(key.value())?;
            return Ok(Some((namespace, table, value)));
        }
    }
    Ok(None)
}

pub(crate) fn resolve_table_id_in_read_txn(
    read_txn: &redb::ReadTransaction,
    table: &TableName,
) -> Result<Option<TableId>> {
    let catalog = match read_txn.open_table(TABLE_CATALOG) {
        Ok(catalog) => catalog,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    let key = default_catalog_key(table);
    let Some(value) = catalog.get(key.as_str()).map_err(map_redb_error)? else {
        return Ok(None);
    };
    active_table_id(table, decode_catalog_value(value.value())?)
}

pub(crate) fn resolve_table_id_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
) -> Result<Option<TableId>> {
    let catalog = match write_txn.open_table(TABLE_CATALOG) {
        Ok(catalog) => catalog,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    let key = default_catalog_key(table);
    let Some(value) = catalog.get(key.as_str()).map_err(map_redb_error)? else {
        return Ok(None);
    };
    active_table_id(table, decode_catalog_value(value.value())?)
}

pub(crate) fn export_table_identities_in_read_txn(
    read_txn: &redb::ReadTransaction,
) -> Result<Vec<TableIdentitySnapshotEntry>> {
    let catalog = match read_txn.open_table(TABLE_CATALOG) {
        Ok(catalog) => catalog,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let mut entries = Vec::new();
    for item in catalog.iter().map_err(map_redb_error)? {
        let (key, value) = item.map_err(map_redb_error)?;
        let (namespace, table) = parse_catalog_key(key.value())?;
        let value = decode_catalog_value(value.value())?;
        entries.push(TableIdentitySnapshotEntry {
            namespace,
            table,
            table_id: value.table_id,
            state: value.state,
        });
    }
    entries.sort_by(|left, right| {
        (&left.namespace, &left.table, &left.table_id).cmp(&(
            &right.namespace,
            &right.table,
            &right.table_id,
        ))
    });
    Ok(entries)
}

pub(crate) fn resolve_or_create_table_id_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
) -> Result<TableId> {
    if let Some(table_id) = resolve_table_id_in_write_txn(write_txn, table)? {
        return Ok(table_id);
    }

    let table_id = TableId::new();
    let key = default_catalog_key(table);
    let value = encode_catalog_value(&TableCatalogValue::active(table_id.clone()))?;
    let mut catalog = write_txn
        .open_table(TABLE_CATALOG)
        .map_err(map_redb_error)?;
    catalog
        .insert(key.as_str(), value.as_str())
        .map_err(map_redb_error)?;
    Ok(table_id)
}

pub(crate) fn ensure_table_id_in_write_txn(
    write_txn: &redb::WriteTransaction,
    identity: &TableIdentitySnapshotEntry,
) -> Result<()> {
    let key = catalog_key(&identity.namespace, &identity.table);
    let mut catalog = write_txn
        .open_table(TABLE_CATALOG)
        .map_err(map_redb_error)?;
    if let Some(existing) = catalog.get(key.as_str()).map_err(map_redb_error)? {
        let existing = decode_catalog_value(existing.value())?;
        if existing.table_id == identity.table_id && existing.state == identity.state {
            return Ok(());
        }
        return Err(nimbus_core::Error::Conflict(format!(
            "logical table {} in namespace {} is already assigned table id {} in {} state, journal references {} in {} state",
            identity.table,
            identity.namespace,
            existing.table_id,
            existing.state,
            identity.table_id,
            identity.state
        )));
    }

    for item in catalog.iter().map_err(map_redb_error)? {
        let (existing_key, existing_value) = item.map_err(map_redb_error)?;
        let existing = decode_catalog_value(existing_value.value())?;
        if existing.table_id == identity.table_id && existing_key.value() != key {
            return Err(nimbus_core::Error::Conflict(format!(
                "table id {} is already assigned to catalog key {:?}, cannot assign it to {} in namespace {}",
                identity.table_id,
                existing_key.value(),
                identity.table,
                identity.namespace
            )));
        }
    }

    let value = encode_catalog_value(&TableCatalogValue {
        table_id: identity.table_id.clone(),
        state: identity.state,
    })?;
    catalog
        .insert(key.as_str(), value.as_str())
        .map_err(map_redb_error)?;
    Ok(())
}

pub(crate) fn ensure_default_table_id_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
    table_id: &TableId,
) -> Result<()> {
    let key = default_catalog_key(table);
    let mut catalog = write_txn
        .open_table(TABLE_CATALOG)
        .map_err(map_redb_error)?;

    let existing = catalog
        .get(key.as_str())
        .map_err(map_redb_error)?
        .map(|value| decode_catalog_value(value.value()))
        .transpose()?;
    let hidden_key = catalog_key(&hidden_table_namespace(table_id), table);
    let staged_hidden = match catalog
        .get(hidden_key.as_str())
        .map_err(map_redb_error)?
        .map(|value| decode_catalog_value(value.value()))
        .transpose()?
    {
        Some(hidden) if hidden.table_id == *table_id && hidden.state == TableState::Hidden => true,
        Some(hidden) => {
            return Err(Error::Conflict(format!(
                "hidden identity slot for logical table {} and table id {} contains {} in {} state",
                table, table_id, hidden.table_id, hidden.state
            )));
        }
        None => false,
    };
    match existing {
        Some(existing)
            if existing.table_id == *table_id && existing.state == TableState::Active =>
        {
            if staged_hidden {
                return Err(Error::Conflict(format!(
                    "logical table {} already has active table id {} and a duplicate hidden slot",
                    table, table_id
                )));
            }
            return Ok(());
        }
        Some(existing) if existing.table_id == *table_id => {
            return Err(Error::Conflict(format!(
                "logical table {} is assigned table id {} in {} lifecycle state",
                table, table_id, existing.state
            )));
        }
        Some(existing) if existing.state == TableState::Active => {
            ensure_table_id_unassigned_in_catalog(&catalog, table_id, Some(hidden_key.as_str()))?;
            let deleting_key = catalog_key(&deleting_table_namespace(&existing.table_id), table);
            let deleting = {
                catalog
                    .get(deleting_key.as_str())
                    .map_err(map_redb_error)?
                    .map(|value| decode_catalog_value(value.value()))
                    .transpose()?
            };
            match deleting {
                Some(deleting)
                    if deleting.table_id == existing.table_id
                        && deleting.state == TableState::Deleting => {}
                Some(deleting) => {
                    return Err(Error::Conflict(format!(
                        "deleting identity slot for logical table {} already contains table id {} in {} state",
                        table, deleting.table_id, deleting.state
                    )));
                }
                None => {
                    let value = encode_catalog_value(&TableCatalogValue {
                        table_id: existing.table_id,
                        state: TableState::Deleting,
                    })?;
                    catalog
                        .insert(deleting_key.as_str(), value.as_str())
                        .map_err(map_redb_error)?;
                }
            }
            if staged_hidden {
                catalog
                    .remove(hidden_key.as_str())
                    .map_err(map_redb_error)?;
            }
            let value = encode_catalog_value(&TableCatalogValue::active(table_id.clone()))?;
            catalog
                .insert(key.as_str(), value.as_str())
                .map_err(map_redb_error)?;
            Ok(())
        }
        Some(existing) => Err(Error::Conflict(format!(
            "logical table {} is already assigned table id {} in {} lifecycle state, journal references {}",
            table, existing.table_id, existing.state, table_id
        ))),
        None => {
            ensure_table_id_unassigned_in_catalog(&catalog, table_id, Some(hidden_key.as_str()))?;
            if staged_hidden {
                catalog
                    .remove(hidden_key.as_str())
                    .map_err(map_redb_error)?;
            }
            let value = encode_catalog_value(&TableCatalogValue::active(table_id.clone()))?;
            catalog
                .insert(key.as_str(), value.as_str())
                .map_err(map_redb_error)?;
            Ok(())
        }
    }
}

fn ensure_table_id_unassigned_in_catalog(
    catalog: &redb::Table<'_, &str, &str>,
    table_id: &TableId,
    allowed_key: Option<&str>,
) -> Result<()> {
    for item in catalog.iter().map_err(map_redb_error)? {
        let (existing_key, existing_value) = item.map_err(map_redb_error)?;
        if Some(existing_key.value()) == allowed_key {
            continue;
        }
        let existing = decode_catalog_value(existing_value.value())?;
        if existing.table_id == *table_id {
            return Err(Error::Conflict(format!(
                "table id {} is already assigned to catalog key {:?}",
                table_id,
                existing_key.value()
            )));
        }
    }
    Ok(())
}

pub(crate) fn stage_hidden_table_identity_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
    table_id: &TableId,
) -> Result<()> {
    if let Some((namespace, existing_table, existing)) =
        find_catalog_entry_by_table_id_in_write_txn(write_txn, table_id)?
    {
        return Err(Error::Conflict(format!(
            "table id {} is already assigned to logical table {} in namespace {} with {} state",
            existing.table_id, existing_table, namespace, existing.state
        )));
    }

    let namespace = hidden_table_namespace(table_id);
    let key = catalog_key(&namespace, table);
    let value = encode_catalog_value(&TableCatalogValue {
        table_id: table_id.clone(),
        state: TableState::Hidden,
    })?;
    let mut catalog = write_txn
        .open_table(TABLE_CATALOG)
        .map_err(map_redb_error)?;
    if catalog.get(key.as_str()).map_err(map_redb_error)?.is_some() {
        return Err(Error::Conflict(format!(
            "hidden table identity already exists for logical table {} and table id {}",
            table, table_id
        )));
    }
    catalog
        .insert(key.as_str(), value.as_str())
        .map_err(map_redb_error)?;
    Ok(())
}

pub(crate) fn mark_default_table_deleting_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
) -> Result<Option<TableId>> {
    let default_key = default_catalog_key(table);
    let mut catalog = write_txn
        .open_table(TABLE_CATALOG)
        .map_err(map_redb_error)?;
    let existing = {
        let Some(existing) = catalog.get(default_key.as_str()).map_err(map_redb_error)? else {
            return Ok(None);
        };
        decode_catalog_value(existing.value())?
    };
    if existing.state != TableState::Active {
        return Err(Error::Conflict(format!(
            "logical table {} is already in {} lifecycle state",
            table, existing.state
        )));
    }

    let table_id = existing.table_id;
    let deleting_namespace = deleting_table_namespace(&table_id);
    let deleting_key = catalog_key(&deleting_namespace, table);
    if catalog
        .get(deleting_key.as_str())
        .map_err(map_redb_error)?
        .is_some()
    {
        return Err(Error::Conflict(format!(
            "deleting table identity already exists for logical table {} and table id {}",
            table, table_id
        )));
    }

    catalog
        .remove(default_key.as_str())
        .map_err(map_redb_error)?;
    let value = encode_catalog_value(&TableCatalogValue {
        table_id: table_id.clone(),
        state: TableState::Deleting,
    })?;
    catalog
        .insert(deleting_key.as_str(), value.as_str())
        .map_err(map_redb_error)?;
    Ok(Some(table_id))
}

pub(crate) fn activate_hidden_table_identity_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
    table_id: &TableId,
) -> Result<Option<TableId>> {
    let hidden_namespace = hidden_table_namespace(table_id);
    let hidden_key = catalog_key(&hidden_namespace, table);
    let default_key = default_catalog_key(table);
    let mut catalog = write_txn
        .open_table(TABLE_CATALOG)
        .map_err(map_redb_error)?;
    let hidden = {
        let Some(hidden) = catalog.get(hidden_key.as_str()).map_err(map_redb_error)? else {
            return Err(Error::InvalidInput(format!(
                "hidden table identity {} for logical table {} does not exist",
                table_id, table
            )));
        };
        decode_catalog_value(hidden.value())?
    };
    if &hidden.table_id != table_id || hidden.state != TableState::Hidden {
        return Err(Error::Conflict(format!(
            "hidden table identity {} for logical table {} is cataloged as {} in {} state",
            table_id, table, hidden.table_id, hidden.state
        )));
    }

    let existing_active = catalog
        .get(default_key.as_str())
        .map_err(map_redb_error)?
        .map(|value| decode_catalog_value(value.value()))
        .transpose()?;
    let old_table_id = match existing_active {
        Some(existing) if existing.state == TableState::Active => Some(existing.table_id),
        Some(existing) => {
            return Err(Error::Conflict(format!(
                "logical table {} is already in {} lifecycle state",
                table, existing.state
            )));
        }
        None => None,
    };

    if let Some(old_table_id) = old_table_id.as_ref() {
        let deleting_key = catalog_key(&deleting_table_namespace(old_table_id), table);
        if catalog
            .get(deleting_key.as_str())
            .map_err(map_redb_error)?
            .is_some()
        {
            return Err(Error::Conflict(format!(
                "deleting table identity already exists for logical table {} and table id {}",
                table, old_table_id
            )));
        }
    }

    catalog
        .remove(hidden_key.as_str())
        .map_err(map_redb_error)?;
    if let Some(old_table_id) = old_table_id.as_ref() {
        let deleting_key = catalog_key(&deleting_table_namespace(old_table_id), table);
        let deleting_value = encode_catalog_value(&TableCatalogValue {
            table_id: old_table_id.clone(),
            state: TableState::Deleting,
        })?;
        catalog
            .remove(default_key.as_str())
            .map_err(map_redb_error)?;
        catalog
            .insert(deleting_key.as_str(), deleting_value.as_str())
            .map_err(map_redb_error)?;
    }

    let active_value = encode_catalog_value(&TableCatalogValue {
        table_id: table_id.clone(),
        state: TableState::Active,
    })?;
    catalog
        .insert(default_key.as_str(), active_value.as_str())
        .map_err(map_redb_error)?;
    Ok(old_table_id)
}

pub(crate) fn hard_delete_deleting_table_identity_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table_id: &TableId,
) -> Result<Option<TableName>> {
    let Some((namespace, table, value)) =
        find_catalog_entry_by_table_id_in_write_txn(write_txn, table_id)?
    else {
        return Ok(None);
    };
    if value.state != TableState::Deleting {
        return Err(Error::Conflict(format!(
            "table id {} for logical table {} is in {} lifecycle state, not deleting",
            table_id, table, value.state
        )));
    }

    let key = catalog_key(&namespace, &table);
    let mut catalog = write_txn
        .open_table(TABLE_CATALOG)
        .map_err(map_redb_error)?;
    catalog.remove(key.as_str()).map_err(map_redb_error)?;
    Ok(Some(table))
}
