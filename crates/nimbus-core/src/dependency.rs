use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    CollectionName, CommitEntry, Document, DocumentId, Error, Filter, IndexId, OrderBy, Query,
    Result, TableId, TableName, TenantEventRecord, WriteOpType,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencySet {
    pub tables: HashSet<TableDependency>,
    #[serde(default)]
    pub collection_groups: HashSet<CollectionName>,
    #[serde(default)]
    pub missing_tables: HashSet<TableName>,
    #[serde(default)]
    pub missing_predicates: Vec<MissingPredicateDependency>,
    pub documents: HashSet<DocumentDependency>,
    pub index_ranges: Vec<IndexRangeDependency>,
    pub predicates: Vec<PredicateDependency>,
    pub paginated_windows: Vec<PaginatedWindowDependency>,
    #[serde(skip, default)]
    index_range_set: HashSet<IndexRangeDependency>,
    #[serde(skip, default)]
    predicate_set: HashSet<PredicateDependency>,
    #[serde(skip, default)]
    paginated_window_set: HashSet<PaginatedWindowDependency>,
}

impl PartialEq for DependencySet {
    fn eq(&self, other: &Self) -> bool {
        self.tables == other.tables
            && self.collection_groups == other.collection_groups
            && self.missing_tables == other.missing_tables
            && self.missing_predicates == other.missing_predicates
            && self.documents == other.documents
            && self.index_ranges == other.index_ranges
            && self.predicates == other.predicates
            && self.paginated_windows == other.paginated_windows
    }
}

impl DependencySet {
    pub fn touched_tables(&self) -> HashSet<TableName> {
        self.tables
            .iter()
            .map(|dependency| dependency.table.clone())
            .chain(self.missing_tables.iter().cloned())
            .chain(
                self.missing_predicates
                    .iter()
                    .map(|dependency| dependency.table.clone()),
            )
            .chain(
                self.documents
                    .iter()
                    .map(|dependency| dependency.table.clone()),
            )
            .chain(
                self.index_ranges
                    .iter()
                    .map(|dependency| dependency.table.clone()),
            )
            .chain(
                self.predicates
                    .iter()
                    .map(|dependency| dependency.table.clone()),
            )
            .chain(
                self.paginated_windows
                    .iter()
                    .map(|dependency| dependency.table.clone()),
            )
            .collect()
    }

    pub fn touches_table(&self, table: &TableName) -> bool {
        self.touched_tables().contains(table)
    }

    pub fn from_engine_query(query: &Query, table_id: Option<TableId>) -> Self {
        let mut dependencies = Self::default();
        let Some(table_id) = table_id else {
            if query.filters.is_empty() {
                dependencies.record_missing_table(&query.table);
            } else {
                dependencies.record_missing_predicate(&query.table, query.filters.clone());
            }
            return dependencies;
        };

        if query.filters.is_empty() {
            dependencies.record_table(&query.table, &table_id);
        } else {
            dependencies.record_predicate(PredicateDependency {
                table: query.table.clone(),
                table_id,
                filters: query.filters.clone(),
            });
        }
        dependencies
    }

    pub fn record_table(&mut self, table: &TableName, table_id: &TableId) {
        self.tables.insert(TableDependency {
            table: table.clone(),
            table_id: table_id.clone(),
        });
    }

    pub fn record_collection_group(&mut self, collection_group: &CollectionName) {
        self.collection_groups.insert(collection_group.clone());
    }

    pub fn record_missing_table(&mut self, table: &TableName) {
        self.missing_tables.insert(table.clone());
    }

    pub fn record_missing_predicate(&mut self, table: &TableName, filters: Vec<Filter>) {
        if filters.is_empty() {
            self.record_missing_table(table);
            return;
        }
        let dependency = MissingPredicateDependency {
            table: table.clone(),
            filters,
        };
        if !self.missing_predicates.contains(&dependency) {
            self.missing_predicates.push(dependency);
        }
    }

    pub fn record_document(
        &mut self,
        table: &TableName,
        table_id: &TableId,
        document_id: DocumentId,
    ) {
        self.documents.insert(DocumentDependency {
            table: table.clone(),
            table_id: table_id.clone(),
            document_id,
        });
    }

    pub fn record_index_range(&mut self, dependency: IndexRangeDependency) {
        self.rebuild_index_range_set_if_needed();
        if self.index_range_set.insert(dependency.clone()) {
            self.index_ranges.push(dependency);
        }
    }

    pub fn record_predicate(&mut self, dependency: PredicateDependency) {
        if dependency.filters.is_empty() {
            return;
        }
        self.rebuild_predicate_set_if_needed();
        if self.predicate_set.insert(dependency.clone()) {
            self.predicates.push(dependency);
        }
    }

    pub fn record_paginated_window(&mut self, dependency: PaginatedWindowDependency) {
        self.rebuild_paginated_window_set_if_needed();
        if self.paginated_window_set.insert(dependency.clone()) {
            self.paginated_windows.push(dependency);
        }
    }

    pub fn extend(&mut self, other: &DependencySet) {
        for dependency in &other.tables {
            self.record_table(&dependency.table, &dependency.table_id);
        }
        for collection_group in &other.collection_groups {
            self.record_collection_group(collection_group);
        }
        for table in &other.missing_tables {
            self.record_missing_table(table);
        }
        for dependency in &other.missing_predicates {
            self.record_missing_predicate(&dependency.table, dependency.filters.clone());
        }
        for dependency in &other.documents {
            self.record_document(
                &dependency.table,
                &dependency.table_id,
                dependency.document_id.clone(),
            );
        }
        for dependency in &other.index_ranges {
            self.record_index_range(dependency.clone());
        }
        for dependency in &other.predicates {
            self.record_predicate(dependency.clone());
        }
        for dependency in &other.paginated_windows {
            self.record_paginated_window(dependency.clone());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
            && self.collection_groups.is_empty()
            && self.missing_tables.is_empty()
            && self.missing_predicates.is_empty()
            && self.documents.is_empty()
            && self.index_ranges.is_empty()
            && self.predicates.is_empty()
            && self.paginated_windows.is_empty()
    }

    fn rebuild_index_range_set_if_needed(&mut self) {
        if self.index_range_set.len() == self.index_ranges.len() {
            return;
        }
        self.index_range_set = self.index_ranges.iter().cloned().collect();
    }

    fn rebuild_predicate_set_if_needed(&mut self) {
        if self.predicate_set.len() == self.predicates.len() {
            return;
        }
        self.predicate_set = self.predicates.iter().cloned().collect();
    }

    fn rebuild_paginated_window_set_if_needed(&mut self) {
        if self.paginated_window_set.len() == self.paginated_windows.len() {
            return;
        }
        self.paginated_window_set = self.paginated_windows.iter().cloned().collect();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableDependency {
    pub table: TableName,
    pub table_id: TableId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentDependency {
    pub table: TableName,
    pub table_id: TableId,
    pub document_id: DocumentId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MissingPredicateDependency {
    pub table: TableName,
    pub filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexRangeDependency {
    pub table: TableName,
    pub table_id: TableId,
    pub index_id: IndexId,
    pub index_name: String,
    pub field: String,
    pub start: Option<Value>,
    pub end: Option<Value>,
    pub start_inclusive: bool,
    pub end_inclusive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PredicateDependency {
    pub table: TableName,
    pub table_id: TableId,
    pub filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaginatedWindowDependency {
    pub table: TableName,
    pub table_id: TableId,
    pub filters: Vec<Filter>,
    pub order: Option<OrderBy>,
    pub start_sort_values: Vec<Option<Value>>,
    pub start_doc_id: Option<DocumentId>,
    pub end_sort_values: Vec<Option<Value>>,
    pub end_doc_id: Option<DocumentId>,
    pub result_count: usize,
    pub page_size: usize,
}

pub fn commit_intersects_dependency_set<F>(
    commit: &CommitEntry,
    dependencies: &DependencySet,
    candidate_documents: &[Document],
    mut resolve_document: F,
) -> bool
where
    F: FnMut(&TableName, DocumentId) -> Result<Option<Document>>,
{
    writes_intersect_dependency_set(
        &commit.writes,
        dependencies,
        candidate_documents,
        &mut resolve_document,
    )
}

pub fn durable_record_intersects_dependency_set<F>(
    record: &TenantEventRecord,
    dependencies: &DependencySet,
    candidate_documents: &[Document],
    mut resolve_document: F,
) -> bool
where
    F: FnMut(&TableName, DocumentId) -> Result<Option<Document>>,
{
    writes_intersect_dependency_set(
        &record.writes,
        dependencies,
        candidate_documents,
        &mut resolve_document,
    )
}

fn writes_intersect_dependency_set<F>(
    writes: &[crate::WriteOp],
    dependencies: &DependencySet,
    candidate_documents: &[Document],
    resolve_document: &mut F,
) -> bool
where
    F: FnMut(&TableName, DocumentId) -> Result<Option<Document>>,
{
    let candidate_documents = candidate_documents
        .iter()
        .map(|document| ((document.table.clone(), document.id.clone()), document))
        .collect::<HashMap<(TableName, DocumentId), &Document>>();

    writes.iter().any(|write| {
        write_intersects_dependency_set(write, dependencies, &candidate_documents, resolve_document)
    })
}

fn write_intersects_dependency_set<F>(
    write: &crate::WriteOp,
    dependencies: &DependencySet,
    candidate_documents: &HashMap<(TableName, DocumentId), &Document>,
    resolve_document: &mut F,
) -> bool
where
    F: FnMut(&TableName, DocumentId) -> Result<Option<Document>>,
{
    if write.resource_path_binding.as_ref().is_some_and(|binding| {
        dependencies
            .collection_groups
            .contains(binding.collection_group())
    }) {
        return true;
    }

    if dependencies.missing_tables.contains(&write.table) {
        return true;
    }

    let relevant_missing_predicates = dependencies
        .missing_predicates
        .iter()
        .filter(|dependency| dependency.table == write.table)
        .collect::<Vec<_>>();

    if dependencies
        .tables
        .iter()
        .any(|dependency| dependency.table_id == write.table_id)
    {
        return true;
    }

    if dependencies.documents.iter().any(|dependency| {
        dependency.table_id == write.table_id && dependency.document_id == write.doc_id
    }) {
        return true;
    }

    if !relevant_missing_predicates.is_empty() {
        if let Some(document) = write.current.as_ref()
            && relevant_missing_predicates.iter().any(|dependency| {
                filters_match_document(document, &dependency.filters).unwrap_or(true)
            })
        {
            return true;
        }
        if let Some(document) = write.previous.as_ref()
            && relevant_missing_predicates.iter().any(|dependency| {
                filters_match_document(document, &dependency.filters).unwrap_or(true)
            })
        {
            return true;
        }
        if let Some(document) = candidate_documents
            .get(&(write.table.clone(), write.doc_id.clone()))
            .copied()
            && relevant_missing_predicates.iter().any(|dependency| {
                filters_match_document(document, &dependency.filters).unwrap_or(true)
            })
        {
            return true;
        }
        if matches!(write.op_type, WriteOpType::Delete) {
            return true;
        }
    }

    let relevant_predicates = dependencies
        .predicates
        .iter()
        .filter(|dependency| dependency.table_id == write.table_id)
        .collect::<Vec<_>>();
    let relevant_paginated_windows = dependencies
        .paginated_windows
        .iter()
        .filter(|dependency| dependency.table_id == write.table_id)
        .collect::<Vec<_>>();
    let mut relevant_index_ranges = dependencies
        .index_ranges
        .iter()
        .filter(|dependency| dependency.table_id == write.table_id);

    let has_relevant_dependencies = !relevant_predicates.is_empty()
        || !relevant_paginated_windows.is_empty()
        || relevant_index_ranges.clone().next().is_some();
    if !has_relevant_dependencies {
        return false;
    }

    if let Some(document) = write.current.as_ref()
        && document_intersects_dependencies(
            document,
            &relevant_predicates,
            &relevant_paginated_windows,
            &mut relevant_index_ranges.clone(),
        )
    {
        return true;
    }

    if let Some(document) = write.previous.as_ref()
        && document_intersects_dependencies(
            document,
            &relevant_predicates,
            &relevant_paginated_windows,
            &mut relevant_index_ranges.clone(),
        )
    {
        return true;
    }

    if let Some(document) = candidate_documents
        .get(&(write.table.clone(), write.doc_id.clone()))
        .copied()
    {
        return document_intersects_dependencies(
            document,
            &relevant_predicates,
            &relevant_paginated_windows,
            &mut relevant_index_ranges,
        );
    }

    if matches!(write.op_type, WriteOpType::Delete) {
        return true;
    }

    match resolve_document(&write.table, write.doc_id.clone()) {
        Ok(Some(document)) => document_intersects_dependencies(
            &document,
            &relevant_predicates,
            &relevant_paginated_windows,
            &mut relevant_index_ranges,
        ),
        Ok(None) | Err(_) => true,
    }
}

fn document_intersects_dependencies<'a>(
    document: &Document,
    relevant_predicates: &[&PredicateDependency],
    relevant_paginated_windows: &[&PaginatedWindowDependency],
    relevant_index_ranges: &mut impl Iterator<Item = &'a IndexRangeDependency>,
) -> bool {
    if relevant_paginated_windows
        .iter()
        .any(|dependency| document_may_affect_paginated_window(document, dependency))
    {
        return true;
    }

    if relevant_predicates
        .iter()
        .any(|dependency| document_matches_predicate_dependency(document, dependency))
    {
        return true;
    }

    relevant_index_ranges.any(|dependency| {
        document_matches_index_range_dependency(document.get_field(&dependency.field), dependency)
    })
}

fn document_matches_predicate_dependency(
    document: &Document,
    dependency: &PredicateDependency,
) -> bool {
    filters_match_document(document, &dependency.filters).unwrap_or(true)
}

fn filters_match_document(document: &Document, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            crate::FilterOp::Eq => field_value == &filter.value,
            crate::FilterOp::Neq => field_value != &filter.value,
            crate::FilterOp::Gt => {
                compare_filter_values(field_value, &filter.value)? == std::cmp::Ordering::Greater
            }
            crate::FilterOp::Gte => matches!(
                compare_filter_values(field_value, &filter.value)?,
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            ),
            crate::FilterOp::Lt => {
                compare_filter_values(field_value, &filter.value)? == std::cmp::Ordering::Less
            }
            crate::FilterOp::Lte => matches!(
                compare_filter_values(field_value, &filter.value)?,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            ),
        };

        if !matched {
            return Ok(false);
        }
    }

    Ok(true)
}

fn compare_filter_values(left: &Value, right: &Value) -> Result<std::cmp::Ordering> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => {
            let left = left
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            let right = right
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            left.partial_cmp(&right).ok_or_else(|| {
                Error::InvalidInput("invalid numeric ordering comparison".to_string())
            })
        }
        _ => Err(Error::InvalidInput(
            "comparisons only support string and number fields in phase 1".to_string(),
        )),
    }
}

fn document_matches_index_range_dependency(
    value: Option<&Value>,
    dependency: &IndexRangeDependency,
) -> bool {
    let Some(value) = value else {
        return false;
    };
    value_matches_bounds(value, dependency)
}

fn document_may_affect_paginated_window(
    document: &Document,
    dependency: &PaginatedWindowDependency,
) -> bool {
    if !filters_match_document(document, &dependency.filters).unwrap_or(true) {
        return false;
    }

    if let Some(start_doc_id) = dependency.start_doc_id.as_ref() {
        match compare_document_to_boundary(
            document,
            dependency.order.as_ref(),
            &dependency.start_sort_values,
            start_doc_id,
        ) {
            Ok(std::cmp::Ordering::Greater) => {}
            Ok(_) => return false,
            Err(_) => return true,
        }
    }

    if dependency.result_count >= dependency.page_size
        && let Some(end_doc_id) = dependency.end_doc_id.as_ref()
    {
        match compare_document_to_boundary(
            document,
            dependency.order.as_ref(),
            &dependency.end_sort_values,
            end_doc_id,
        ) {
            Ok(std::cmp::Ordering::Greater) => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
    }

    true
}

fn compare_document_to_boundary(
    document: &Document,
    order: Option<&OrderBy>,
    boundary_sort_values: &[Option<Value>],
    boundary_doc_id: &DocumentId,
) -> Result<std::cmp::Ordering> {
    let ordering = match order {
        Some(order) => {
            let boundary_value = boundary_sort_values.first().and_then(Option::as_ref);
            let ordering =
                compare_runtime_order_field(document.get_field(&order.field), boundary_value)?;
            match order.direction {
                crate::OrderDirection::Asc => ordering,
                crate::OrderDirection::Desc => ordering.reverse(),
            }
        }
        None if !boundary_sort_values.is_empty() => {
            return Err(Error::InvalidInput(
                "invalid paginated dependency boundary".to_string(),
            ));
        }
        None => std::cmp::Ordering::Equal,
    };

    Ok(ordering.then_with(|| document.id.cmp(boundary_doc_id)))
}

fn compare_runtime_order_field(
    left: Option<&Value>,
    right: Option<&Value>,
) -> Result<std::cmp::Ordering> {
    match (left, right) {
        (Some(left), Some(right)) => compare_filter_values(left, right),
        (Some(_), None) => Ok(std::cmp::Ordering::Less),
        (None, Some(_)) => Ok(std::cmp::Ordering::Greater),
        (None, None) => Ok(std::cmp::Ordering::Equal),
    }
}

fn value_matches_bounds(value: &Value, dependency: &IndexRangeDependency) -> bool {
    if let Some(start) = dependency.start.as_ref() {
        let Some(ordering) = compare_index_values(value, start) else {
            return true;
        };
        if ordering == std::cmp::Ordering::Less
            || (ordering == std::cmp::Ordering::Equal && !dependency.start_inclusive)
        {
            return false;
        }
    }

    if let Some(end) = dependency.end.as_ref() {
        let Some(ordering) = compare_index_values(value, end) else {
            return true;
        };
        if ordering == std::cmp::Ordering::Greater
            || (ordering == std::cmp::Ordering::Equal && !dependency.end_inclusive)
        {
            return false;
        }
    }

    true
}

fn compare_index_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .and_then(|(left, right)| left.partial_cmp(&right)),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{SequenceNumber, TableId, Timestamp, WriteOp};

    fn tasks_table() -> TableName {
        TableName::new("tasks").expect("table should be valid")
    }

    fn document_with_fields(
        table: TableName,
        document_id: DocumentId,
        fields: serde_json::Map<String, Value>,
    ) -> Document {
        Document {
            id: document_id,
            table,
            creation_time: Timestamp::now(),
            update_time: Timestamp::now(),
            fields,
            typed_fields: Default::default(),
        }
    }

    fn single_write_commit(
        table: TableName,
        table_id: TableId,
        op_type: WriteOpType,
        doc_id: DocumentId,
    ) -> CommitEntry {
        CommitEntry {
            sequence: SequenceNumber(1),
            timestamp: Timestamp::now(),
            writes: vec![WriteOp {
                table,
                table_id,
                op_type,
                doc_id,
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: None,
            }],
        }
    }

    #[test]
    fn table_dependency_matches_writes_on_the_same_table() {
        let table = tasks_table();
        let table_id = TableId::new();
        let commit = single_write_commit(
            table.clone(),
            table_id.clone(),
            WriteOpType::Insert,
            DocumentId::new(),
        );
        let mut dependencies = DependencySet::default();
        dependencies.record_table(&table, &table_id);

        assert!(commit_intersects_dependency_set(
            &commit,
            &dependencies,
            &[],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn document_dependency_matches_only_the_target_document() {
        let table = tasks_table();
        let table_id = TableId::new();
        let target_id = DocumentId::new();
        let other_id = DocumentId::new();
        let mut dependencies = DependencySet::default();
        dependencies.record_document(&table, &table_id, target_id.clone());

        assert!(commit_intersects_dependency_set(
            &single_write_commit(
                table.clone(),
                table_id.clone(),
                WriteOpType::Update,
                target_id,
            ),
            &dependencies,
            &[],
            |_, _| Ok(None),
        ));
        assert!(!commit_intersects_dependency_set(
            &single_write_commit(table, table_id, WriteOpType::Update, other_id),
            &dependencies,
            &[],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn table_dependency_uses_table_id_not_reused_table_name() {
        let table = tasks_table();
        let old_table_id = TableId::new();
        let new_table_id = TableId::new();
        let mut dependencies = DependencySet::default();
        dependencies.record_table(&table, &old_table_id);

        assert!(!commit_intersects_dependency_set(
            &single_write_commit(table, new_table_id, WriteOpType::Insert, DocumentId::new(),),
            &dependencies,
            &[],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn document_dependency_uses_table_id_not_reused_table_name_and_document_key() {
        let table = tasks_table();
        let old_table_id = TableId::new();
        let new_table_id = TableId::new();
        let document_id = DocumentId::new();
        let mut dependencies = DependencySet::default();
        dependencies.record_document(&table, &old_table_id, document_id.clone());

        assert!(!commit_intersects_dependency_set(
            &single_write_commit(table, new_table_id, WriteOpType::Update, document_id),
            &dependencies,
            &[],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn missing_table_dependency_matches_first_write_for_that_name() {
        let table = tasks_table();
        let mut dependencies = DependencySet::default();
        dependencies.record_missing_table(&table);

        assert!(commit_intersects_dependency_set(
            &single_write_commit(
                table,
                TableId::new(),
                WriteOpType::Insert,
                DocumentId::new(),
            ),
            &dependencies,
            &[],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn missing_predicate_dependency_matches_only_possible_first_writes() {
        let table = tasks_table();
        let matching_id = DocumentId::new();
        let nonmatching_id = DocumentId::new();
        let mut dependencies = DependencySet::default();
        dependencies.record_missing_predicate(
            &table,
            vec![Filter {
                field: "status".to_string(),
                op: crate::FilterOp::Eq,
                value: json!("active"),
            }],
        );
        let matching = document_with_fields(
            table.clone(),
            matching_id.clone(),
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        );
        let nonmatching = document_with_fields(
            table.clone(),
            nonmatching_id.clone(),
            serde_json::Map::from_iter([("status".to_string(), json!("archived"))]),
        );

        assert!(commit_intersects_dependency_set(
            &single_write_commit(
                table.clone(),
                TableId::new(),
                WriteOpType::Insert,
                matching_id,
            ),
            &dependencies,
            &[matching],
            |_, _| Ok(None),
        ));
        assert!(!commit_intersects_dependency_set(
            &single_write_commit(table, TableId::new(), WriteOpType::Insert, nonmatching_id),
            &dependencies,
            &[nonmatching],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn index_range_dependency_matches_documents_inside_the_range() {
        let table = tasks_table();
        let table_id = TableId::new();
        let doc_id = DocumentId::new();
        let commit = single_write_commit(
            table.clone(),
            table_id.clone(),
            WriteOpType::Insert,
            doc_id.clone(),
        );
        let document = document_with_fields(
            table.clone(),
            doc_id,
            serde_json::Map::from_iter([("rank".to_string(), json!(3))]),
        );
        let mut dependencies = DependencySet::default();
        dependencies.record_index_range(IndexRangeDependency {
            table,
            table_id,
            index_id: IndexId::new(),
            index_name: "by_rank".to_string(),
            field: "rank".to_string(),
            start: Some(json!(2)),
            end: Some(json!(5)),
            start_inclusive: true,
            end_inclusive: true,
        });

        assert!(commit_intersects_dependency_set(
            &commit,
            &dependencies,
            &[document],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn paginated_window_dependency_respects_filters() {
        let table = tasks_table();
        let table_id = TableId::new();
        let doc_id = DocumentId::new();
        let commit = single_write_commit(
            table.clone(),
            table_id.clone(),
            WriteOpType::Insert,
            doc_id.clone(),
        );
        let matching = document_with_fields(
            table.clone(),
            doc_id,
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        );
        let mut dependencies = DependencySet::default();
        dependencies.record_paginated_window(PaginatedWindowDependency {
            table,
            table_id,
            filters: vec![Filter {
                field: "status".to_string(),
                op: crate::FilterOp::Eq,
                value: json!("active"),
            }],
            order: None,
            start_sort_values: Vec::new(),
            start_doc_id: None,
            end_sort_values: Vec::new(),
            end_doc_id: None,
            result_count: 1,
            page_size: 10,
        });

        assert!(commit_intersects_dependency_set(
            &commit,
            &dependencies,
            &[matching],
            |_, _| Ok(None),
        ));
    }

    #[test]
    fn dependency_set_roundtrip_rebuilds_hash_backed_dedup_state() {
        let table = tasks_table();
        let table_id = TableId::new();
        let index_dependency = IndexRangeDependency {
            table: table.clone(),
            table_id: table_id.clone(),
            index_id: IndexId::new(),
            index_name: "by_rank".to_string(),
            field: "rank".to_string(),
            start: Some(json!(1)),
            end: Some(json!(3)),
            start_inclusive: true,
            end_inclusive: true,
        };
        let predicate_dependency = PredicateDependency {
            table: table.clone(),
            table_id: table_id.clone(),
            filters: vec![Filter {
                field: "status".to_string(),
                op: crate::FilterOp::Eq,
                value: json!("active"),
            }],
        };
        let paginated_dependency = PaginatedWindowDependency {
            table,
            table_id,
            filters: predicate_dependency.filters.clone(),
            order: None,
            start_sort_values: Vec::new(),
            start_doc_id: None,
            end_sort_values: Vec::new(),
            end_doc_id: None,
            result_count: 1,
            page_size: 10,
        };

        let mut dependencies = DependencySet::default();
        dependencies.record_index_range(index_dependency.clone());
        dependencies.record_predicate(predicate_dependency.clone());
        dependencies.record_paginated_window(paginated_dependency.clone());

        let serialized =
            serde_json::to_string(&dependencies).expect("dependency set should serialize");
        let mut decoded: DependencySet =
            serde_json::from_str(&serialized).expect("dependency set should deserialize");

        decoded.record_index_range(index_dependency);
        decoded.record_predicate(predicate_dependency);
        decoded.record_paginated_window(paginated_dependency);

        assert_eq!(decoded.index_ranges.len(), 1);
        assert_eq!(decoded.predicates.len(), 1);
        assert_eq!(decoded.paginated_windows.len(), 1);
    }
}

/// Property tests over the conflict predicate's interval edges (PPSC2
/// criterion). Every case supplies the written document through
/// `candidate_documents` so the decision comes from the document image, never
/// from the fail-closed resolver fallback; a separate case pins that fallback.
#[cfg(test)]
mod interval_edge_proptests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;
    use crate::{FilterOp, SequenceNumber, TableId, Timestamp, WriteOp};

    const RANK_FIELD: &str = "rank";

    fn ranked_document(table: &TableName, rank: Option<Value>) -> Document {
        let mut fields = serde_json::Map::new();
        if let Some(rank) = rank {
            fields.insert(RANK_FIELD.to_string(), rank);
        }
        Document {
            id: DocumentId::new(),
            table: table.clone(),
            creation_time: Timestamp::now(),
            update_time: Timestamp::now(),
            fields,
            typed_fields: Default::default(),
        }
    }

    fn insert_commit(table: &TableName, table_id: &TableId, document: &Document) -> CommitEntry {
        CommitEntry {
            sequence: SequenceNumber(1),
            timestamp: Timestamp::now(),
            writes: vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(document.clone()),
            }],
        }
    }

    fn index_range_dependencies(
        table: &TableName,
        table_id: &TableId,
        start: Option<Value>,
        end: Option<Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> DependencySet {
        let mut dependencies = DependencySet::default();
        dependencies.record_index_range(IndexRangeDependency {
            table: table.clone(),
            table_id: table_id.clone(),
            index_id: IndexId::new(),
            index_name: "by_rank".to_string(),
            field: RANK_FIELD.to_string(),
            start,
            end,
            start_inclusive,
            end_inclusive,
        });
        dependencies
    }

    fn intersects(commit: &CommitEntry, dependencies: &DependencySet, document: &Document) -> bool {
        commit_intersects_dependency_set(
            commit,
            dependencies,
            std::slice::from_ref(document),
            |_, _| Ok(None),
        )
    }

    /// Independent oracle for a closed/open numeric interval.
    fn interval_oracle(
        value: i64,
        start: Option<i64>,
        end: Option<i64>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> bool {
        let above_start = match start {
            Some(start) if start_inclusive => value >= start,
            Some(start) => value > start,
            None => true,
        };
        let below_end = match end {
            Some(end) if end_inclusive => value <= end,
            Some(end) => value < end,
            None => true,
        };
        above_start && below_end
    }

    proptest! {
        /// Index-range decisions agree with the interval oracle across the
        /// whole numeric lattice, including both-edge equality with every
        /// inclusivity combination.
        #[test]
        fn index_range_numeric_edges_match_interval_oracle(
            value in -8i64..=8,
            bound_a in -8i64..=8,
            bound_b in -8i64..=8,
            start_bounded in any::<bool>(),
            end_bounded in any::<bool>(),
            start_inclusive in any::<bool>(),
            end_inclusive in any::<bool>(),
        ) {
            let (start, end) = (bound_a.min(bound_b), bound_a.max(bound_b));
            let start = start_bounded.then_some(start);
            let end = end_bounded.then_some(end);
            let table = TableName::new("ranked").expect("table should be valid");
            let table_id = TableId::new();
            let document = ranked_document(&table, Some(json!(value)));
            let commit = insert_commit(&table, &table_id, &document);
            let dependencies = index_range_dependencies(
                &table,
                &table_id,
                start.map(|bound| json!(bound)),
                end.map(|bound| json!(bound)),
                start_inclusive,
                end_inclusive,
            );

            prop_assert_eq!(
                intersects(&commit, &dependencies, &document),
                interval_oracle(value, start, end, start_inclusive, end_inclusive),
            );
        }

        /// A value sitting exactly on a bound conflicts iff that bound is
        /// inclusive — the half-open edge cannot leak a phantom in either
        /// direction.
        #[test]
        fn index_range_equal_edge_follows_inclusivity(
            value in -8i64..=8,
            inclusive in any::<bool>(),
            edge_is_start in any::<bool>(),
        ) {
            let table = TableName::new("ranked").expect("table should be valid");
            let table_id = TableId::new();
            let document = ranked_document(&table, Some(json!(value)));
            let commit = insert_commit(&table, &table_id, &document);
            let (start, end) = if edge_is_start {
                (Some(json!(value)), None)
            } else {
                (None, Some(json!(value)))
            };
            let dependencies =
                index_range_dependencies(&table, &table_id, start, end, inclusive, inclusive);

            prop_assert_eq!(intersects(&commit, &dependencies, &document), inclusive);
        }

        /// Cross-type comparisons are unorderable, so the predicate must fail
        /// closed and report a conflict regardless of the bounds.
        #[test]
        fn index_range_incomparable_types_fail_closed(
            bound in -8i64..=8,
            start_inclusive in any::<bool>(),
            end_inclusive in any::<bool>(),
        ) {
            let table = TableName::new("ranked").expect("table should be valid");
            let table_id = TableId::new();
            let document = ranked_document(&table, Some(json!("not-a-number")));
            let commit = insert_commit(&table, &table_id, &document);
            let dependencies = index_range_dependencies(
                &table,
                &table_id,
                Some(json!(bound)),
                Some(json!(bound)),
                start_inclusive,
                end_inclusive,
            );

            prop_assert!(intersects(&commit, &dependencies, &document));
        }

        /// A document without the indexed field can never satisfy the range,
        /// so it must not conflict no matter where the bounds sit.
        #[test]
        fn index_range_missing_field_never_matches(
            bound in -8i64..=8,
            start_inclusive in any::<bool>(),
            end_inclusive in any::<bool>(),
        ) {
            let table = TableName::new("ranked").expect("table should be valid");
            let table_id = TableId::new();
            let document = ranked_document(&table, None);
            let commit = insert_commit(&table, &table_id, &document);
            let dependencies = index_range_dependencies(
                &table,
                &table_id,
                Some(json!(bound)),
                Some(json!(bound)),
                start_inclusive,
                end_inclusive,
            );

            prop_assert!(!intersects(&commit, &dependencies, &document));
        }

        /// Predicate-dependency filters (Gt/Gte/Lt/Lte) agree with the same
        /// interval oracle at and around their comparison edges.
        #[test]
        fn predicate_filter_numeric_edges_match_interval_oracle(
            value in -8i64..=8,
            bound_a in -8i64..=8,
            bound_b in -8i64..=8,
            start_inclusive in any::<bool>(),
            end_inclusive in any::<bool>(),
        ) {
            let (start, end) = (bound_a.min(bound_b), bound_a.max(bound_b));
            let table = TableName::new("ranked").expect("table should be valid");
            let table_id = TableId::new();
            let document = ranked_document(&table, Some(json!(value)));
            let commit = insert_commit(&table, &table_id, &document);
            let mut dependencies = DependencySet::default();
            dependencies.record_predicate(PredicateDependency {
                table: table.clone(),
                table_id: table_id.clone(),
                filters: vec![
                    Filter {
                        field: RANK_FIELD.to_string(),
                        op: if start_inclusive { FilterOp::Gte } else { FilterOp::Gt },
                        value: json!(start),
                    },
                    Filter {
                        field: RANK_FIELD.to_string(),
                        op: if end_inclusive { FilterOp::Lte } else { FilterOp::Lt },
                        value: json!(end),
                    },
                ],
            });

            prop_assert_eq!(
                intersects(&commit, &dependencies, &document),
                interval_oracle(value, Some(start), Some(end), start_inclusive, end_inclusive),
            );
        }

        /// String bounds order lexicographically with the same edge
        /// inclusivity semantics as numbers.
        #[test]
        fn index_range_string_equal_edge_follows_inclusivity(
            raw in "[a-c]{1,3}",
            inclusive in any::<bool>(),
            edge_is_start in any::<bool>(),
        ) {
            let table = TableName::new("ranked").expect("table should be valid");
            let table_id = TableId::new();
            let document = ranked_document(&table, Some(json!(raw.clone())));
            let commit = insert_commit(&table, &table_id, &document);
            let (start, end) = if edge_is_start {
                (Some(json!(raw)), None)
            } else {
                (None, Some(json!(raw)))
            };
            let dependencies =
                index_range_dependencies(&table, &table_id, start, end, inclusive, inclusive);

            prop_assert_eq!(intersects(&commit, &dependencies, &document), inclusive);
        }
    }

    /// The resolver fallback itself stays fail-closed: with no document image
    /// available anywhere, an unresolvable write conflicts.
    #[test]
    fn unresolvable_write_fails_closed_against_an_index_range() {
        let table = TableName::new("ranked").expect("table should be valid");
        let table_id = TableId::new();
        let dependencies = index_range_dependencies(
            &table,
            &table_id,
            Some(json!(0)),
            Some(json!(0)),
            true,
            true,
        );
        let commit = CommitEntry {
            sequence: SequenceNumber(1),
            timestamp: Timestamp::now(),
            writes: vec![WriteOp {
                table: table.clone(),
                table_id,
                op_type: WriteOpType::Update,
                doc_id: DocumentId::new(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: None,
            }],
        };

        assert!(commit_intersects_dependency_set(
            &commit,
            &dependencies,
            &[],
            |_, _| Ok(None),
        ));
    }
}
