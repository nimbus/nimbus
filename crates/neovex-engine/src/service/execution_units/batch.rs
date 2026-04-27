use neovex_core::{
    AccessAction, AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome, AtomicWriteResult,
    Document, Error, FieldTransform, FieldTransformOperation, NumericValue, Result, SpecialDouble,
    StoredValue, Timestamp, TypedScalarValue, WriteKey, WritePrecondition, WriteSetMode,
};

use super::super::mutations::enforce_mutation_authorization;
use super::MutationExecutionUnit;

struct PendingAtomicWriteResult {
    update_time: Option<neovex_core::Timestamp>,
    transform_results: Vec<StoredValue>,
}

impl MutationExecutionUnit {
    pub fn stage_atomic_write_batch(
        &self,
        batch: AtomicWriteBatch,
    ) -> Result<AtomicWriteBatchOutcome> {
        let pending_results = self.prepare_atomic_write_batch(batch)?;
        Ok(self.atomic_write_batch_outcome(None, self.service.now(), pending_results))
    }

    pub fn execute_atomic_write_batch(
        &self,
        batch: AtomicWriteBatch,
    ) -> Result<AtomicWriteBatchOutcome> {
        let pending_results = self.prepare_atomic_write_batch(batch)?;

        let commit = self.commit()?;
        let commit_time = commit
            .as_ref()
            .map(|commit| commit.timestamp)
            .unwrap_or_else(|| self.service.now());
        Ok(self.atomic_write_batch_outcome(commit, commit_time, pending_results))
    }

    fn apply_atomic_write(&self, write: AtomicWrite) -> Result<PendingAtomicWriteResult> {
        match write {
            AtomicWrite::Set {
                key,
                document,
                mode,
                precondition,
                transforms,
            } => self.apply_set_write(key, document, mode, precondition, transforms),
            AtomicWrite::Patch {
                key,
                field_patch,
                mask,
                precondition,
                transforms,
            } => self.apply_patch_write(key, field_patch, mask, precondition, transforms),
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
            } => self.apply_transform_write(key, transforms, precondition),
        }
    }

    fn prepare_atomic_write_batch(
        &self,
        batch: AtomicWriteBatch,
    ) -> Result<Vec<PendingAtomicWriteResult>> {
        if batch.writes.is_empty() {
            return Err(Error::InvalidInput(
                "atomic write batch must contain at least one write".to_string(),
            ));
        }

        let mut pending_results = Vec::with_capacity(batch.writes.len());
        for write in batch.writes {
            pending_results.push(self.apply_atomic_write(write)?);
        }
        Ok(pending_results)
    }

    fn atomic_write_batch_outcome(
        &self,
        commit: Option<neovex_core::CommitEntry>,
        commit_time: Timestamp,
        pending_results: Vec<PendingAtomicWriteResult>,
    ) -> AtomicWriteBatchOutcome {
        let write_results = pending_results
            .into_iter()
            .map(|result| AtomicWriteResult {
                update_time: result.update_time.map(|_| commit_time),
                transform_results: result.transform_results,
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
        key: WriteKey,
        document: serde_json::Map<String, serde_json::Value>,
        mode: WriteSetMode,
        precondition: WritePrecondition,
        transforms: Vec<FieldTransform>,
    ) -> Result<PendingAtomicWriteResult> {
        precondition.validate()?;

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
                Document::with_id(locator.id.clone(), table.clone(), document)
            }
            WriteSetMode::Overwrite => overwrite_document(
                &locator,
                table.clone(),
                existing.as_ref(),
                document,
                self.service.now(),
            ),
            WriteSetMode::MergeAll => merge_document(
                &locator,
                table.clone(),
                existing.as_ref(),
                document,
                None,
                self.service.now(),
            ),
            WriteSetMode::MergeFields(mask) => merge_document(
                &locator,
                table.clone(),
                existing.as_ref(),
                document,
                Some(mask),
                self.service.now(),
            ),
        };
        let transform_results =
            apply_field_transforms_at(&mut current, &transforms, self.service.now())?;

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
        preserve_document_lifecycle_times(existing.as_ref(), &mut current, self.service.now());

        self.stage_write(
            table,
            locator.id.clone(),
            existing,
            Some(current),
            indexes,
            key.resource_path_binding().cloned(),
        )?;

        Ok(PendingAtomicWriteResult {
            update_time: Some(self.service.now()),
            transform_results,
        })
    }

    fn apply_patch_write(
        &self,
        key: WriteKey,
        field_patch: serde_json::Map<String, serde_json::Value>,
        mask: Vec<String>,
        precondition: WritePrecondition,
        transforms: Vec<FieldTransform>,
    ) -> Result<PendingAtomicWriteResult> {
        precondition.validate()?;

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
        apply_patch_mask(&mut current, &field_patch, &mask);
        let transform_results =
            apply_field_transforms_at(&mut current, &transforms, self.service.now())?;
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
        preserve_document_lifecycle_times(existing.as_ref(), &mut current, self.service.now());

        self.stage_write(
            table,
            locator.id.clone(),
            existing,
            Some(current),
            indexes,
            key.resource_path_binding().cloned(),
        )?;

        Ok(PendingAtomicWriteResult {
            update_time: Some(self.service.now()),
            transform_results,
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
        })
    }

    fn apply_transform_write(
        &self,
        key: WriteKey,
        transforms: Vec<FieldTransform>,
        precondition: WritePrecondition,
    ) -> Result<PendingAtomicWriteResult> {
        precondition.validate()?;
        if transforms.is_empty() {
            return Err(Error::InvalidInput(
                "transform writes must include at least one field transform".to_string(),
            ));
        }

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
        let transform_results =
            apply_field_transforms_at(&mut current, &transforms, self.service.now())?;
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
        preserve_document_lifecycle_times(existing.as_ref(), &mut current, self.service.now());

        self.stage_write(
            table,
            locator.id.clone(),
            existing,
            Some(current),
            indexes,
            key.resource_path_binding().cloned(),
        )?;

        Ok(PendingAtomicWriteResult {
            update_time: Some(self.service.now()),
            transform_results,
        })
    }

    fn load_batch_document(&self, key: &WriteKey) -> Result<Option<Document>> {
        let locator = key.locator();
        let document = self.current_document(&locator.table, &locator.id)?;
        self.active_state()?
            .read_dependencies
            .record_document(&locator.table, locator.id.clone());
        Ok(document)
    }

    fn ensure_write_precondition(
        &self,
        locator: &neovex_core::DocumentLocator,
        existing: Option<&Document>,
        precondition: &WritePrecondition,
    ) -> Result<()> {
        if let Some(update_time) = precondition.update_time {
            return Err(Error::InvalidInput(format!(
                "update-time preconditions are modeled but not executable yet (requested {})",
                update_time.0
            )));
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
    locator: &neovex_core::DocumentLocator,
    table: neovex_core::TableName,
    existing: Option<&Document>,
    fields: serde_json::Map<String, serde_json::Value>,
    update_time: Timestamp,
) -> Document {
    let mut document = Document::with_id(locator.id.clone(), table, serde_json::Map::new());
    apply_patch_mask(&mut document, &fields, &[]);
    preserve_document_lifecycle_times(existing, &mut document, update_time);
    document
}

fn merge_document(
    locator: &neovex_core::DocumentLocator,
    table: neovex_core::TableName,
    existing: Option<&Document>,
    patch: serde_json::Map<String, serde_json::Value>,
    mask: Option<Vec<String>>,
    update_time: Timestamp,
) -> Document {
    let mut document = existing
        .cloned()
        .unwrap_or_else(|| Document::with_id(locator.id.clone(), table, serde_json::Map::new()));
    apply_patch_mask(&mut document, &patch, mask.as_deref().unwrap_or(&[]));
    preserve_document_lifecycle_times(existing, &mut document, update_time);
    document
}

fn apply_patch_mask(
    document: &mut Document,
    patch: &serde_json::Map<String, serde_json::Value>,
    mask: &[String],
) {
    if mask.is_empty() {
        for (field, value) in patch {
            document.set_field(field.clone(), value.clone());
        }
        return;
    }

    for field in mask {
        let segments = split_field_path_segments(field);
        match patch_value_at_path(patch, &segments) {
            Some(value) => {
                set_document_field_path(document, &segments, value.clone());
            }
            None => {
                remove_document_field_path(document, &segments);
            }
        }
    }
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

    let joined = segments.join(".");
    document.typed_fields.remove(&joined);
    remove_value_at_path(&mut document.fields, segments);
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
            let mut next_values = match document.get_field(field_name) {
                Some(serde_json::Value::Array(values)) => values.clone(),
                _ => Vec::new(),
            };
            for value in values {
                if !next_values
                    .iter()
                    .any(|existing| firestore_transform_values_equivalent(existing, value))
                {
                    next_values.push(value.clone());
                }
            }
            document.set_field(
                field_name.to_string(),
                serde_json::Value::Array(next_values),
            );
            Ok(StoredValue::Json {
                value: serde_json::Value::Null,
            })
        }
        FieldTransformOperation::RemoveAllFromArray { values } => {
            let next_values = match document.get_field(field_name) {
                Some(serde_json::Value::Array(existing)) => existing
                    .iter()
                    .filter(|existing| {
                        !values
                            .iter()
                            .any(|value| firestore_transform_values_equivalent(existing, value))
                    })
                    .cloned()
                    .collect(),
                _ => Vec::new(),
            };
            document.set_field(
                field_name.to_string(),
                serde_json::Value::Array(next_values),
            );
            Ok(StoredValue::Json {
                value: serde_json::Value::Null,
            })
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

fn firestore_transform_values_equivalent(
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> bool {
    match (
        FiniteNumericTransformValue::from_document(left),
        FiniteNumericTransformValue::from_document(right),
    ) {
        (Some(left), Some(right)) => numeric_transform_values_equivalent(left, right),
        _ => left == right,
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
