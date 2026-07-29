use nimbus_core::{
    AccessAction, ArrayPopSide, AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome,
    AtomicWriteResult, BitwiseOperation, Document, Error, FieldTransform, FieldTransformOperation,
    NumericValue, Result, SpecialDouble, StoredValue, Timestamp, TypedFieldMap, TypedScalarValue,
    WriteKey, WritePrecondition, WriteSetMode,
};

use super::super::mutations::enforce_mutation_authorization;
use super::MutationExecutionUnit;

struct PendingAtomicWriteResult {
    update_time: Option<nimbus_core::Timestamp>,
    transform_results: Vec<StoredValue>,
    server_timestamp_results: Vec<usize>,
}

struct PreparedAtomicWriteBatch {
    results: Vec<PendingAtomicWriteResult>,
}

struct SetWriteInput {
    key: WriteKey,
    document: serde_json::Map<String, serde_json::Value>,
    typed_fields: TypedFieldMap,
    mode: WriteSetMode,
    precondition: WritePrecondition,
    transforms: Vec<FieldTransform>,
}

struct PatchWriteInput {
    key: WriteKey,
    field_patch: serde_json::Map<String, serde_json::Value>,
    typed_fields: TypedFieldMap,
    mask: Vec<String>,
    precondition: WritePrecondition,
    transforms: Vec<FieldTransform>,
}

impl MutationExecutionUnit {
    pub fn stage_atomic_write_batch(
        &self,
        batch: AtomicWriteBatch,
    ) -> Result<AtomicWriteBatchOutcome> {
        let prepared = self.prepare_atomic_write_batch(batch)?;
        Ok(self.atomic_write_batch_outcome(None, Timestamp(0), prepared.results))
    }

    pub fn execute_atomic_write_batch(
        &self,
        batch: AtomicWriteBatch,
    ) -> Result<AtomicWriteBatchOutcome> {
        let prepared = self.prepare_atomic_write_batch(batch)?;

        let commit = self.commit()?;
        let commit_time = commit
            .as_ref()
            .map(|commit| commit.timestamp)
            .unwrap_or(Timestamp(0));
        Ok(self.atomic_write_batch_outcome(commit, commit_time, prepared.results))
    }

    fn apply_atomic_write(
        &self,
        write: AtomicWrite,
        write_time: Timestamp,
    ) -> Result<PendingAtomicWriteResult> {
        match write {
            AtomicWrite::Set {
                key,
                document,
                typed_fields,
                mode,
                precondition,
                transforms,
            } => self.apply_set_write(
                SetWriteInput {
                    key,
                    document,
                    typed_fields,
                    mode,
                    precondition,
                    transforms,
                },
                write_time,
            ),
            AtomicWrite::Patch {
                key,
                field_patch,
                typed_fields,
                mask,
                precondition,
                transforms,
            } => self.apply_patch_write(
                PatchWriteInput {
                    key,
                    field_patch,
                    typed_fields,
                    mask,
                    precondition,
                    transforms,
                },
                write_time,
            ),
            AtomicWrite::Delete {
                key,
                precondition,
                missing_ok,
            } => self.apply_delete_write(key, precondition, missing_ok),
            AtomicWrite::Verify { key, precondition } => self.apply_verify_write(key, precondition),
            AtomicWrite::Transform {
                key,
                transforms,
                precondition,
            } => self.apply_transform_write(key, transforms, precondition, write_time),
        }
    }

    fn prepare_atomic_write_batch(
        &self,
        batch: AtomicWriteBatch,
    ) -> Result<PreparedAtomicWriteBatch> {
        if batch.writes.is_empty() {
            return Err(Error::InvalidInput(
                "atomic write batch must contain at least one write".to_string(),
            ));
        }

        let timestamp = Timestamp(0);
        let mut pending_results = Vec::with_capacity(batch.writes.len());
        for write in batch.writes {
            pending_results.push(self.apply_atomic_write(write, timestamp)?);
        }
        Ok(PreparedAtomicWriteBatch {
            results: pending_results,
        })
    }

    fn atomic_write_batch_outcome(
        &self,
        commit: Option<nimbus_core::CommitEntry>,
        commit_time: Timestamp,
        pending_results: Vec<PendingAtomicWriteResult>,
    ) -> AtomicWriteBatchOutcome {
        let write_results = pending_results
            .into_iter()
            .map(|mut result| {
                for index in result.server_timestamp_results {
                    result.transform_results[index] = StoredValue::TypedScalar {
                        value: TypedScalarValue::Timestamp { value: commit_time },
                    };
                }
                AtomicWriteResult {
                    update_time: result.update_time.map(|_| commit_time),
                    transform_results: result.transform_results,
                }
            })
            .collect();

        AtomicWriteBatchOutcome {
            commit,
            commit_time,
            write_results,
        }
    }

    fn apply_set_write(
        &self,
        input: SetWriteInput,
        write_time: Timestamp,
    ) -> Result<PendingAtomicWriteResult> {
        let SetWriteInput {
            key,
            document,
            typed_fields,
            mode,
            precondition,
            transforms,
        } = input;
        precondition.validate()?;

        let (replace_document, overwritten_fields) = match &mode {
            WriteSetMode::Create | WriteSetMode::Overwrite => (true, Vec::new()),
            WriteSetMode::MergeAll => (false, document.keys().cloned().collect()),
            WriteSetMode::MergeFields(mask) => (false, mask.clone()),
        };
        let server_timestamp_results = server_timestamp_result_indexes(&transforms);

        let locator = key.locator().clone();
        let table = locator.table.clone();
        let existing = self.load_batch_document(&key)?;
        self.ensure_write_precondition(&locator, existing.as_ref(), &precondition)?;
        let table_schema = self.schema_snapshot.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();

        let mut current = match mode {
            WriteSetMode::Create => {
                if existing.is_some() {
                    return Err(Error::AlreadyExists(format!(
                        "document already exists: {}",
                        locator.id
                    )));
                }
                let mut created =
                    Document::with_id(locator.id.clone(), table.clone(), serde_json::Map::new());
                apply_patch_mask(&mut created, &document, &typed_fields, &[]);
                created
            }
            WriteSetMode::Overwrite => overwrite_document(
                &locator,
                table.clone(),
                existing.as_ref(),
                document,
                typed_fields,
                write_time,
            ),
            WriteSetMode::MergeAll => merge_document(
                &locator,
                table.clone(),
                existing.as_ref(),
                document,
                typed_fields,
                None,
                write_time,
            ),
            WriteSetMode::MergeFields(mask) => merge_document(
                &locator,
                table.clone(),
                existing.as_ref(),
                document,
                typed_fields,
                Some(mask),
                write_time,
            ),
        };
        let transform_results = apply_field_transforms_at(&mut current, &transforms, write_time)?;

        if let Some(table_schema) = table_schema.as_ref() {
            table_schema.validate(&current.fields)?;
        }
        enforce_mutation_authorization(
            table_schema.as_ref(),
            if existing.is_some() {
                AccessAction::Update
            } else {
                AccessAction::Create
            },
            &self.principal,
            Some(&current),
            existing.as_ref(),
        )?;
        preserve_document_lifecycle_times(existing.as_ref(), &mut current, write_time);

        self.stage_write(
            table,
            locator.id.clone(),
            existing,
            Some(current),
            indexes,
            key.resource_path_binding().cloned(),
        )?;
        self.update_deferred_server_timestamp_fields(
            &locator,
            replace_document,
            &overwritten_fields,
            &transforms,
        )?;

        Ok(PendingAtomicWriteResult {
            update_time: Some(write_time),
            transform_results,
            server_timestamp_results,
        })
    }

    fn apply_patch_write(
        &self,
        input: PatchWriteInput,
        write_time: Timestamp,
    ) -> Result<PendingAtomicWriteResult> {
        let PatchWriteInput {
            key,
            field_patch,
            typed_fields,
            mask,
            precondition,
            transforms,
        } = input;
        precondition.validate()?;

        let overwritten_fields = if mask.is_empty() {
            field_patch.keys().cloned().collect::<Vec<_>>()
        } else {
            mask.clone()
        };
        let server_timestamp_results = server_timestamp_result_indexes(&transforms);

        let locator = key.locator().clone();
        let table = locator.table.clone();
        let existing = self.load_batch_document(&key)?;
        self.ensure_write_precondition(&locator, existing.as_ref(), &precondition)?;
        let table_schema = self.schema_snapshot.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();

        let mut current = existing.clone().unwrap_or_else(|| {
            Document::with_id(locator.id.clone(), table.clone(), serde_json::Map::new())
        });
        apply_patch_mask(&mut current, &field_patch, &typed_fields, &mask);
        let transform_results = apply_field_transforms_at(&mut current, &transforms, write_time)?;
        if let Some(table_schema) = table_schema.as_ref() {
            table_schema.validate(&current.fields)?;
        }
        enforce_mutation_authorization(
            table_schema.as_ref(),
            if existing.is_some() {
                AccessAction::Update
            } else {
                AccessAction::Create
            },
            &self.principal,
            Some(&current),
            existing.as_ref(),
        )?;
        preserve_document_lifecycle_times(existing.as_ref(), &mut current, write_time);

        self.stage_write(
            table,
            locator.id.clone(),
            existing,
            Some(current),
            indexes,
            key.resource_path_binding().cloned(),
        )?;
        self.update_deferred_server_timestamp_fields(
            &locator,
            false,
            &overwritten_fields,
            &transforms,
        )?;

        Ok(PendingAtomicWriteResult {
            update_time: Some(write_time),
            transform_results,
            server_timestamp_results,
        })
    }

    fn apply_delete_write(
        &self,
        key: WriteKey,
        precondition: WritePrecondition,
        missing_ok: bool,
    ) -> Result<PendingAtomicWriteResult> {
        precondition.validate()?;

        let locator = key.locator().clone();
        let table = locator.table.clone();
        let existing = self.load_batch_document(&key)?;
        if existing.is_none() && precondition.is_empty() && missing_ok {
            return Ok(PendingAtomicWriteResult {
                update_time: None,
                transform_results: Vec::new(),
                server_timestamp_results: Vec::new(),
            });
        }
        self.ensure_write_precondition(&locator, existing.as_ref(), &precondition)?;

        let Some(existing) = existing else {
            return Err(Error::DocumentNotFound(locator.id));
        };

        let table_schema = self.schema_snapshot.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Delete,
            &self.principal,
            None,
            Some(&existing),
        )?;
        self.stage_write(
            table,
            locator.id.clone(),
            Some(existing),
            None,
            indexes,
            None,
        )?;

        Ok(PendingAtomicWriteResult {
            update_time: None,
            transform_results: Vec::new(),
            server_timestamp_results: Vec::new(),
        })
    }

    fn apply_verify_write(
        &self,
        key: WriteKey,
        precondition: WritePrecondition,
    ) -> Result<PendingAtomicWriteResult> {
        precondition.validate()?;
        if precondition.is_empty() {
            return Err(Error::InvalidInput(
                "verify writes must include a precondition".to_string(),
            ));
        }

        let locator = key.locator().clone();
        let existing = self.load_batch_document(&key)?;
        let table_schema = self.schema_snapshot.get_table(&locator.table).cloned();
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Read,
            &self.principal,
            existing.as_ref(),
            existing.as_ref(),
        )?;
        self.ensure_write_precondition(&locator, existing.as_ref(), &precondition)?;

        Ok(PendingAtomicWriteResult {
            update_time: None,
            transform_results: Vec::new(),
            server_timestamp_results: Vec::new(),
        })
    }

    fn apply_transform_write(
        &self,
        key: WriteKey,
        transforms: Vec<FieldTransform>,
        precondition: WritePrecondition,
        write_time: Timestamp,
    ) -> Result<PendingAtomicWriteResult> {
        precondition.validate()?;
        if transforms.is_empty() {
            return Err(Error::InvalidInput(
                "transform writes must include at least one field transform".to_string(),
            ));
        }
        let server_timestamp_results = server_timestamp_result_indexes(&transforms);

        let locator = key.locator().clone();
        let table = locator.table.clone();
        let existing = self.load_batch_document(&key)?;
        self.ensure_write_precondition(&locator, existing.as_ref(), &precondition)?;
        let table_schema = self.schema_snapshot.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();

        let mut current = existing.clone().unwrap_or_else(|| {
            Document::with_id(locator.id.clone(), table.clone(), serde_json::Map::new())
        });
        let transform_results = apply_field_transforms_at(&mut current, &transforms, write_time)?;
        if let Some(table_schema) = table_schema.as_ref() {
            table_schema.validate(&current.fields)?;
        }
        enforce_mutation_authorization(
            table_schema.as_ref(),
            if existing.is_some() {
                AccessAction::Update
            } else {
                AccessAction::Create
            },
            &self.principal,
            Some(&current),
            existing.as_ref(),
        )?;
        preserve_document_lifecycle_times(existing.as_ref(), &mut current, write_time);

        self.stage_write(
            table,
            locator.id.clone(),
            existing,
            Some(current),
            indexes,
            key.resource_path_binding().cloned(),
        )?;
        self.update_deferred_server_timestamp_fields(&locator, false, &[], &transforms)?;

        Ok(PendingAtomicWriteResult {
            update_time: Some(write_time),
            transform_results,
            server_timestamp_results,
        })
    }

    fn update_deferred_server_timestamp_fields(
        &self,
        locator: &nimbus_core::DocumentLocator,
        replace_document: bool,
        overwritten_fields: &[String],
        transforms: &[FieldTransform],
    ) -> Result<()> {
        let mut state = self.active_state()?;
        let key = (locator.table.clone(), locator.id.clone());
        let fields = state
            .deferred_server_timestamp_fields
            .entry(key)
            .or_default();
        if replace_document {
            fields.clear();
        }
        for field in overwritten_fields {
            if let Some(top_level) = field.split('.').next() {
                fields.remove(top_level);
            }
        }
        for transform in transforms {
            let field = top_level_transform_field_name(&transform.field)?.to_string();
            fields.remove(&field);
            if matches!(
                &transform.transform,
                FieldTransformOperation::ServerTimestamp
            ) {
                fields.insert(field);
            }
        }
        if fields.is_empty() {
            state
                .deferred_server_timestamp_fields
                .remove(&(locator.table.clone(), locator.id.clone()));
        }
        Ok(())
    }

    fn load_batch_document(&self, key: &WriteKey) -> Result<Option<Document>> {
        let locator = key.locator();
        let document = self.current_document(&locator.table, &locator.id)?;
        let table_id = self.snapshot.table_id(&locator.table)?;
        match table_id.as_ref() {
            Some(table_id) => self.active_state()?.read_dependencies.record_document(
                &locator.table,
                table_id,
                locator.id.clone(),
            ),
            None => self
                .active_state()?
                .read_dependencies
                .record_missing_table(&locator.table),
        }
        Ok(document)
    }

    fn ensure_write_precondition(
        &self,
        locator: &nimbus_core::DocumentLocator,
        existing: Option<&Document>,
        precondition: &WritePrecondition,
    ) -> Result<()> {
        if let Some(update_time) = precondition.update_time {
            let Some(existing) = existing else {
                return Err(Error::DocumentNotFound(locator.id.clone()));
            };
            if existing.update_time != update_time {
                return Err(Error::PreconditionFailed(format!(
                    "document update_time precondition failed for {}: expected {}, found {}",
                    locator.id, update_time.0, existing.update_time.0
                )));
            }
        }

        match precondition.exists {
            Some(true) if existing.is_none() => Err(Error::DocumentNotFound(locator.id.clone())),
            Some(false) if existing.is_some() => Err(Error::AlreadyExists(format!(
                "document already exists: {}",
                locator.id
            ))),
            Some(_) | None => Ok(()),
        }
    }
}

fn overwrite_document(
    locator: &nimbus_core::DocumentLocator,
    table: nimbus_core::TableName,
    existing: Option<&Document>,
    fields: serde_json::Map<String, serde_json::Value>,
    typed_fields: TypedFieldMap,
    update_time: Timestamp,
) -> Document {
    let mut document = Document::with_id(locator.id.clone(), table, serde_json::Map::new());
    apply_patch_mask(&mut document, &fields, &typed_fields, &[]);
    preserve_document_lifecycle_times(existing, &mut document, update_time);
    document
}

fn merge_document(
    locator: &nimbus_core::DocumentLocator,
    table: nimbus_core::TableName,
    existing: Option<&Document>,
    patch: serde_json::Map<String, serde_json::Value>,
    typed_fields: TypedFieldMap,
    mask: Option<Vec<String>>,
    update_time: Timestamp,
) -> Document {
    let mut document = existing
        .cloned()
        .unwrap_or_else(|| Document::with_id(locator.id.clone(), table, serde_json::Map::new()));
    apply_patch_mask(
        &mut document,
        &patch,
        &typed_fields,
        mask.as_deref().unwrap_or(&[]),
    );
    preserve_document_lifecycle_times(existing, &mut document, update_time);
    document
}

fn apply_patch_mask(
    document: &mut Document,
    patch: &serde_json::Map<String, serde_json::Value>,
    typed_patch: &TypedFieldMap,
    mask: &[String],
) {
    if mask.is_empty() {
        for (field, value) in patch {
            document.set_field(field.clone(), value.clone());
            let stored_value = typed_patch
                .get(field)
                .cloned()
                .unwrap_or_else(|| StoredValue::from_json_tree(value.clone()));
            set_document_typed_field_path(document, &[field.as_str()], stored_value);
        }
        return;
    }

    for field in mask {
        let segments = split_field_path_segments(field);
        match patch_value_at_path(patch, &segments) {
            Some(value) => {
                set_document_field_path(document, &segments, value.clone());
                let stored_value = typed_patch_value_at_path(typed_patch, &segments)
                    .cloned()
                    .unwrap_or_else(|| StoredValue::from_json_tree(value.clone()));
                set_document_typed_field_path(document, &segments, stored_value);
            }
            None => {
                remove_document_field_path(document, &segments);
            }
        }
    }
}

fn typed_patch_value_at_path<'a>(
    typed_patch: &'a TypedFieldMap,
    segments: &[&str],
) -> Option<&'a StoredValue> {
    let (first, rest) = segments.split_first()?;
    let mut current = typed_patch.get(*first)?;
    for segment in rest {
        let StoredValue::Map { entries } = current else {
            return None;
        };
        current = entries.get(*segment)?;
    }
    Some(current)
}

fn split_field_path_segments(field_path: &str) -> Vec<&str> {
    field_path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn patch_value_at_path<'a>(
    patch: &'a serde_json::Map<String, serde_json::Value>,
    segments: &[&str],
) -> Option<&'a serde_json::Value> {
    let (first, rest) = segments.split_first()?;
    let mut current = patch.get(*first)?;
    for segment in rest {
        current = current.as_object()?.get(*segment)?;
    }
    Some(current)
}

fn set_document_field_path(document: &mut Document, segments: &[&str], value: serde_json::Value) {
    if let [field] = segments {
        document.set_field((*field).to_string(), value);
        return;
    }

    let root = segments[0].to_string();
    if !matches!(
        document.fields.get(&root),
        Some(serde_json::Value::Object(_))
    ) {
        document.typed_fields.remove(&root);
    }
    set_value_at_path(&mut document.fields, segments, value);
}

fn set_document_typed_field_path(document: &mut Document, segments: &[&str], value: StoredValue) {
    let [root, rest @ ..] = segments else {
        return;
    };
    if rest.is_empty() {
        if value.contains_typed_metadata() {
            document.typed_fields.insert((*root).to_string(), value);
        } else {
            document.typed_fields.remove(*root);
        }
        return;
    }

    let projected_root = document
        .fields
        .get(*root)
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let mut stored_root = document
        .typed_fields
        .remove(*root)
        .unwrap_or_else(|| StoredValue::from_json_tree(projected_root.clone()));
    if !matches!(stored_root, StoredValue::Map { .. }) {
        stored_root = StoredValue::from_json_tree(projected_root);
    }
    set_stored_value_at_path(&mut stored_root, rest, value);
    if stored_root.contains_typed_metadata() {
        document
            .typed_fields
            .insert((*root).to_string(), stored_root);
    }
}

fn set_stored_value_at_path(current: &mut StoredValue, segments: &[&str], value: StoredValue) {
    let (first, rest) = segments
        .split_first()
        .expect("typed field paths should include at least one segment");
    if !matches!(current, StoredValue::Map { .. }) {
        *current = StoredValue::Map {
            entries: std::collections::BTreeMap::new(),
        };
    }
    let StoredValue::Map { entries } = current else {
        unreachable!("stored value was normalized to a map")
    };
    if rest.is_empty() {
        entries.insert((*first).to_string(), value);
        return;
    }
    let child = entries
        .entry((*first).to_string())
        .or_insert_with(|| StoredValue::Map {
            entries: std::collections::BTreeMap::new(),
        });
    set_stored_value_at_path(child, rest, value);
}

fn set_value_at_path(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    segments: &[&str],
    value: serde_json::Value,
) {
    let (first, rest) = segments
        .split_first()
        .expect("field paths should include at least one segment");
    if rest.is_empty() {
        fields.insert((*first).to_string(), value);
        return;
    }

    let entry = fields
        .entry((*first).to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = serde_json::Value::Object(serde_json::Map::new());
    }
    let nested = entry
        .as_object_mut()
        .expect("nested patch paths should materialize JSON objects");
    set_value_at_path(nested, rest, value);
}

fn remove_document_field_path(document: &mut Document, segments: &[&str]) {
    if let [field] = segments {
        document.remove_field(field);
        return;
    }

    remove_value_at_path(&mut document.fields, segments);
    remove_document_typed_field_path(document, segments);
}

fn remove_document_typed_field_path(document: &mut Document, segments: &[&str]) {
    let [root, rest @ ..] = segments else {
        return;
    };
    if rest.is_empty() {
        document.typed_fields.remove(*root);
        return;
    }
    let Some(mut stored_root) = document.typed_fields.remove(*root) else {
        return;
    };
    remove_stored_value_at_path(&mut stored_root, rest);
    if stored_root.contains_typed_metadata() {
        document
            .typed_fields
            .insert((*root).to_string(), stored_root);
    }
}

fn remove_stored_value_at_path(current: &mut StoredValue, segments: &[&str]) {
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    let StoredValue::Map { entries } = current else {
        return;
    };
    if rest.is_empty() {
        entries.remove(*first);
        return;
    }
    if let Some(child) = entries.get_mut(*first) {
        remove_stored_value_at_path(child, rest);
    }
}

fn remove_value_at_path(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    segments: &[&str],
) -> bool {
    let (first, rest) = segments
        .split_first()
        .expect("field paths should include at least one segment");
    if rest.is_empty() {
        fields.remove(*first);
        return fields.is_empty();
    }

    let should_prune = match fields.get_mut(*first) {
        Some(serde_json::Value::Object(map)) => remove_value_at_path(map, rest),
        Some(_) => {
            fields.remove(*first);
            false
        }
        None => false,
    };
    if should_prune {
        fields.remove(*first);
    }
    fields.is_empty()
}

fn preserve_document_lifecycle_times(
    existing: Option<&Document>,
    current: &mut Document,
    update_time: Timestamp,
) {
    if let Some(existing) = existing {
        current.creation_time = existing.creation_time;
        current.update_time =
            if existing.fields == current.fields && existing.typed_fields == current.typed_fields {
                existing.update_time
            } else {
                update_time
            };
        return;
    }
    current.update_time = update_time;
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FiniteNumericTransformValue {
    Integer(i64),
    Double(f64),
}

impl FiniteNumericTransformValue {
    fn from_operand(value: &NumericValue, context: &str) -> Result<Self> {
        match value {
            NumericValue::Integer { value } => Ok(Self::Integer(*value)),
            NumericValue::Double { value } if value.is_finite() => Ok(Self::Double(*value)),
            NumericValue::Double { .. } | NumericValue::SpecialDouble { .. } => {
                Err(Error::InvalidInput(format!(
                    "{context} must be a Firestore int64 or finite double"
                )))
            }
        }
    }

    fn from_document(value: &serde_json::Value) -> Option<Self> {
        if let Some(value) = value.as_i64() {
            return Some(Self::Integer(value));
        }
        if let Some(value) = value.as_u64() {
            return i64::try_from(value).ok().map(Self::Integer);
        }
        value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Self::Double)
    }

    fn as_f64(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Double(value) => value,
        }
    }

    fn into_value(self) -> Result<serde_json::Value> {
        Ok(match self {
            Self::Integer(value) => serde_json::Value::Number(serde_json::Number::from(value)),
            Self::Double(value) => {
                serde_json::Value::Number(serde_json::Number::from_f64(value).ok_or_else(|| {
                    Error::InvalidInput(
                        "numeric transform produced a non-finite double".to_string(),
                    )
                })?)
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ComparableNumericValue {
    Integer(i64),
    Double(f64),
    SpecialDouble(SpecialDouble),
}

impl ComparableNumericValue {
    fn from_operand(value: &NumericValue, context: &str) -> Result<Self> {
        match value {
            NumericValue::Integer { value } => Ok(Self::Integer(*value)),
            NumericValue::Double { value } if value.is_finite() => Ok(Self::Double(*value)),
            NumericValue::Double { .. } => Err(Error::InvalidInput(format!(
                "{context} must be a Firestore int64, finite double, or special double sentinel"
            ))),
            NumericValue::SpecialDouble { value } => Ok(Self::SpecialDouble(*value)),
        }
    }

    fn from_document(document: &Document, field_name: &str) -> Option<Self> {
        match document.typed_field(field_name) {
            Some(TypedScalarValue::SpecialDouble { value }) => Some(Self::SpecialDouble(*value)),
            Some(TypedScalarValue::Timestamp { .. }) => None,
            Some(_) => None,
            None => document
                .get_field(field_name)
                .and_then(FiniteNumericTransformValue::from_document)
                .map(Into::into),
        }
    }

    fn into_stored_value(self) -> StoredValue {
        match self {
            Self::Integer(value) => StoredValue::Json {
                value: serde_json::Value::Number(serde_json::Number::from(value)),
            },
            Self::Double(value) => StoredValue::Json {
                value: serde_json::Value::Number(
                    serde_json::Number::from_f64(value).expect("finite doubles should serialize"),
                ),
            },
            Self::SpecialDouble(value) => StoredValue::TypedScalar {
                value: TypedScalarValue::SpecialDouble { value },
            },
        }
    }

    fn write_to_document(self, document: &mut Document, field_name: &str) {
        match self {
            Self::Integer(value) => document.set_field(
                field_name.to_string(),
                serde_json::Value::Number(serde_json::Number::from(value)),
            ),
            Self::Double(value) => document.set_field(
                field_name.to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(value).expect("finite doubles should serialize"),
                ),
            ),
            Self::SpecialDouble(value) => document.set_typed_field(
                field_name.to_string(),
                TypedScalarValue::SpecialDouble { value },
            ),
        }
    }

    fn equivalent(self, other: Self) -> bool {
        match (self, other) {
            (Self::SpecialDouble(left), Self::SpecialDouble(right)) => left == right,
            (left, right) => {
                let left = left.numeric_cmp_value();
                let right = right.numeric_cmp_value();
                if left == 0.0 && right == 0.0 {
                    true
                } else {
                    left == right
                }
            }
        }
    }

    fn numeric_cmp_value(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Double(value) => value,
            Self::SpecialDouble(SpecialDouble::NegativeZero) => -0.0,
            Self::SpecialDouble(SpecialDouble::Nan) => f64::NAN,
            Self::SpecialDouble(SpecialDouble::PositiveInfinity) => f64::INFINITY,
            Self::SpecialDouble(SpecialDouble::NegativeInfinity) => f64::NEG_INFINITY,
        }
    }
}

impl From<FiniteNumericTransformValue> for ComparableNumericValue {
    fn from(value: FiniteNumericTransformValue) -> Self {
        match value {
            FiniteNumericTransformValue::Integer(value) => Self::Integer(value),
            FiniteNumericTransformValue::Double(value) => Self::Double(value),
        }
    }
}

fn apply_field_transforms_at(
    document: &mut Document,
    transforms: &[FieldTransform],
    transform_time: Timestamp,
) -> Result<Vec<StoredValue>> {
    let mut results = Vec::with_capacity(transforms.len());
    for transform in transforms {
        let field_name = top_level_transform_field_name(&transform.field)?;
        let result =
            apply_field_transform(document, field_name, &transform.transform, transform_time)?;
        results.push(result);
    }
    Ok(results)
}

fn server_timestamp_result_indexes(transforms: &[FieldTransform]) -> Vec<usize> {
    transforms
        .iter()
        .enumerate()
        .filter_map(|(index, transform)| {
            matches!(
                &transform.transform,
                FieldTransformOperation::ServerTimestamp
            )
            .then_some(index)
        })
        .collect()
}

fn apply_field_transform(
    document: &mut Document,
    field_name: &str,
    transform: &FieldTransformOperation,
    transform_time: Timestamp,
) -> Result<StoredValue> {
    match transform {
        FieldTransformOperation::ServerTimestamp => {
            let value = TypedScalarValue::Timestamp {
                value: transform_time,
            };
            document.set_typed_field(field_name.to_string(), value.clone());
            Ok(StoredValue::TypedScalar { value })
        }
        FieldTransformOperation::Increment { operand } => {
            let next = transform_increment(document.get_field(field_name), operand)?;
            document.set_field(field_name.to_string(), next.clone());
            Ok(StoredValue::Json { value: next })
        }
        FieldTransformOperation::Multiply { operand } => {
            let next = transform_multiply(document.get_field(field_name), operand)?;
            document.set_field(field_name.to_string(), next.clone());
            Ok(StoredValue::Json { value: next })
        }
        FieldTransformOperation::Maximum { operand } => {
            let next = transform_extreme(document, field_name, operand, ExtremeKind::Maximum)?;
            next.write_to_document(document, field_name);
            Ok(next.into_stored_value())
        }
        FieldTransformOperation::Minimum { operand } => {
            let next = transform_extreme(document, field_name, operand, ExtremeKind::Minimum)?;
            next.write_to_document(document, field_name);
            Ok(next.into_stored_value())
        }
        FieldTransformOperation::AppendMissingElements { values } => {
            let mut next_values = current_array_elements(document, field_name);
            for value in values {
                let value = value.canonical();
                if !next_values
                    .iter()
                    .any(|existing| firestore_transform_values_equivalent(existing, &value))
                {
                    next_values.push(value);
                }
            }
            write_array_elements(document, field_name, next_values);
            Ok(StoredValue::Json {
                value: serde_json::Value::Null,
            })
        }
        FieldTransformOperation::AppendElements { values } => {
            let mut next_values = current_array_elements(document, field_name);
            next_values.extend(values.iter().map(StoredValue::canonical));
            write_array_elements(document, field_name, next_values);
            Ok(StoredValue::Json {
                value: serde_json::Value::Null,
            })
        }
        FieldTransformOperation::PopArray { side } => {
            let mut next_values = current_array_elements(document, field_name);
            if !next_values.is_empty() {
                match side {
                    ArrayPopSide::First => {
                        next_values.remove(0);
                    }
                    ArrayPopSide::Last => {
                        next_values.pop();
                    }
                }
            }
            write_array_elements(document, field_name, next_values);
            Ok(StoredValue::Json {
                value: serde_json::Value::Null,
            })
        }
        FieldTransformOperation::RemoveAllFromArray { values } => {
            let removals = values
                .iter()
                .map(StoredValue::canonical)
                .collect::<Vec<_>>();
            let next_values = current_array_elements(document, field_name)
                .into_iter()
                .filter(|existing| {
                    !removals
                        .iter()
                        .any(|value| firestore_transform_values_equivalent(existing, value))
                })
                .collect();
            write_array_elements(document, field_name, next_values);
            Ok(StoredValue::Json {
                value: serde_json::Value::Null,
            })
        }
        FieldTransformOperation::Bitwise { operation, operand } => {
            let current = transform_integer_value(document.get_field(field_name)).unwrap_or(0);
            let next = match operation {
                BitwiseOperation::And => current & operand,
                BitwiseOperation::Or => current | operand,
                BitwiseOperation::Xor => current ^ operand,
            };
            let value = serde_json::Value::Number(serde_json::Number::from(next));
            document.set_field(field_name.to_string(), value.clone());
            Ok(StoredValue::Json { value })
        }
    }
}

fn top_level_transform_field_name(field_path: &str) -> Result<&str> {
    if field_path.is_empty() {
        return Err(Error::InvalidInput(
            "field transform `fieldPath` cannot be empty".to_string(),
        ));
    }
    if field_path.contains('.') || field_path.contains('`') || field_path.contains('\\') {
        return Err(Error::InvalidInput(
            "nested or quoted field paths in field transforms are not supported yet".to_string(),
        ));
    }
    Ok(field_path)
}

fn transform_increment(
    current: Option<&serde_json::Value>,
    operand: &NumericValue,
) -> Result<serde_json::Value> {
    let operand =
        FiniteNumericTransformValue::from_operand(operand, "increment transform operand")?;
    match current.and_then(FiniteNumericTransformValue::from_document) {
        Some(current) => match (current, operand) {
            (
                FiniteNumericTransformValue::Integer(current),
                FiniteNumericTransformValue::Integer(operand),
            ) => FiniteNumericTransformValue::Integer(current.saturating_add(operand)).into_value(),
            (current, operand) => {
                FiniteNumericTransformValue::Double(current.as_f64() + operand.as_f64())
                    .into_value()
            }
        },
        None => operand.into_value(),
    }
}

fn transform_multiply(
    current: Option<&serde_json::Value>,
    operand: &NumericValue,
) -> Result<serde_json::Value> {
    let operand = FiniteNumericTransformValue::from_operand(operand, "multiply transform operand")?;
    match current.and_then(FiniteNumericTransformValue::from_document) {
        Some(current) => match (current, operand) {
            (
                FiniteNumericTransformValue::Integer(current),
                FiniteNumericTransformValue::Integer(operand),
            ) => FiniteNumericTransformValue::Integer(current.saturating_mul(operand)).into_value(),
            (current, operand) => {
                FiniteNumericTransformValue::Double(current.as_f64() * operand.as_f64())
                    .into_value()
            }
        },
        None => FiniteNumericTransformValue::Integer(0).into_value(),
    }
}

fn transform_integer_value(current: Option<&serde_json::Value>) -> Option<i64> {
    let value = current?;
    if let Some(value) = value.as_i64() {
        return Some(value);
    }
    value.as_u64().and_then(|value| i64::try_from(value).ok())
}

#[derive(Debug, Clone, Copy)]
enum ExtremeKind {
    Maximum,
    Minimum,
}

fn transform_extreme(
    document: &Document,
    field_name: &str,
    operand: &NumericValue,
    kind: ExtremeKind,
) -> Result<ComparableNumericValue> {
    let operand = ComparableNumericValue::from_operand(
        operand,
        match kind {
            ExtremeKind::Maximum => "maximum transform operand",
            ExtremeKind::Minimum => "minimum transform operand",
        },
    )?;
    let Some(current) = ComparableNumericValue::from_document(document, field_name) else {
        return Ok(operand);
    };

    if current.equivalent(operand) {
        return Ok(current);
    }

    if matches!(
        current,
        ComparableNumericValue::SpecialDouble(SpecialDouble::Nan)
    ) || matches!(
        operand,
        ComparableNumericValue::SpecialDouble(SpecialDouble::Nan)
    ) {
        return Ok(ComparableNumericValue::SpecialDouble(SpecialDouble::Nan));
    }

    let use_operand = match kind {
        ExtremeKind::Maximum => current.numeric_cmp_value() < operand.numeric_cmp_value(),
        ExtremeKind::Minimum => current.numeric_cmp_value() > operand.numeric_cmp_value(),
    };
    Ok(if use_operand { operand } else { current })
}

/// Read one array field as canonical stored elements, preferring the typed tree
/// so array members that carry adapter scalars (Firestore timestamps, bytes,
/// references, geo points) survive a transform instead of decaying to their
/// plain JSON projection.
fn current_array_elements(document: &Document, field_name: &str) -> Vec<StoredValue> {
    match document.typed_value(field_name) {
        Some(StoredValue::List { items }) => {
            return items.iter().map(StoredValue::canonical).collect();
        }
        // A field whose typed value is not a list has no array to transform;
        // Firestore array transforms overwrite such a field with a fresh
        // array. `Json`-spelled arrays cannot appear here: every write site
        // strips metadata-free root entries from the typed sidecar
        // (`set_document_typed_field_path`, `write_array_elements`, adapter
        // lowering), `typed_value` is an exact root-key lookup, and nested
        // field paths are rejected by transform validation upstream — pinned
        // by `atomic_write_batch_set_normalizes_metadata_free_typed_twins`.
        Some(_) => return Vec::new(),
        None => {}
    }
    match document.get_field(field_name) {
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .map(|value| StoredValue::Json {
                value: value.clone(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Write one array field back, keeping typed metadata only when some element
/// still needs it so metadata-free arrays stay plain JSON in storage.
fn write_array_elements(document: &mut Document, field_name: &str, values: Vec<StoredValue>) {
    let stored = StoredValue::List { items: values };
    if stored.contains_typed_metadata() {
        document.set_typed_field(field_name.to_string(), stored);
    } else {
        document.set_field(field_name.to_string(), stored.projected_json());
    }
}

/// Compare two array-transform values the way Firestore compares them.
///
/// Structural equality is not enough at any depth: Firestore treats an int64 and
/// a double of the same magnitude as the same value, so `3` and `3.0` must
/// dedupe together whether they sit at the top of an element or several levels
/// inside one. Both sides are expected to be canonical (see
/// `StoredValue::canonical`), which means every metadata-free subtree is spelled
/// `Json`; a `Map` or `List` node therefore still carries typed metadata
/// somewhere and can never equal a plain `Json` node.
fn firestore_transform_values_equivalent(left: &StoredValue, right: &StoredValue) -> bool {
    match (left, right) {
        (StoredValue::Json { value: left }, StoredValue::Json { value: right }) => {
            json_transform_values_equivalent(left, right)
        }
        (StoredValue::TypedScalar { value: left }, StoredValue::TypedScalar { value: right }) => {
            left == right
        }
        (StoredValue::Map { entries: left }, StoredValue::Map { entries: right }) => {
            left.len() == right.len()
                && left.iter().all(|(field, left)| {
                    right
                        .get(field)
                        .is_some_and(|right| firestore_transform_values_equivalent(left, right))
                })
        }
        (StoredValue::List { items: left }, StoredValue::List { items: right }) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| firestore_transform_values_equivalent(left, right))
        }
        _ => false,
    }
}

/// Numeric-aware structural comparison of two plain JSON subtrees.
fn json_transform_values_equivalent(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    if let (Some(left), Some(right)) = (
        FiniteNumericTransformValue::from_document(left),
        FiniteNumericTransformValue::from_document(right),
    ) {
        return numeric_transform_values_equivalent(left, right);
    }
    match (left, right) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(field, left)| {
                    right
                        .get(field)
                        .is_some_and(|right| json_transform_values_equivalent(left, right))
                })
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| json_transform_values_equivalent(left, right))
        }
        (left, right) => left == right,
    }
}

fn numeric_transform_values_equivalent(
    left: FiniteNumericTransformValue,
    right: FiniteNumericTransformValue,
) -> bool {
    match (left, right) {
        (
            FiniteNumericTransformValue::Integer(left),
            FiniteNumericTransformValue::Integer(right),
        ) => left == right,
        (left, right) => {
            if left.as_f64() == 0.0 && right.as_f64() == 0.0 {
                true
            } else {
                left.as_f64() == right.as_f64()
            }
        }
    }
}
