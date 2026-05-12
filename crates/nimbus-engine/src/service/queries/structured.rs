use std::collections::HashSet;
use std::future::{Future, pending};
use std::sync::Arc;

use nimbus_core::{
    AggregationOperator, CollectionName, CollectionPath, CompositeOperator, CountAggregation,
    Document, DocumentPath, Error, FieldFilterOperator, Filter, OrderDirection, PrincipalContext,
    Result, StructuredAggregation, StructuredAggregationQuery, StructuredAggregationResult,
    StructuredQuery, TableName, TenantId, UnaryFilterOperator,
};
use serde_json::{Map, Number, Value};

use crate::service::Service;

mod finalize;
mod prepare;
#[cfg(test)]
mod tests;

pub(crate) use self::finalize::{
    collection_group_table_targets, finalize_structured_documents, finalize_structured_rows,
    structured_base_query,
};
pub(crate) use self::prepare::{
    ensure_structured_query_index, prepare_collection_group_structured_query,
    prepare_structured_query,
};

#[derive(Debug, Clone)]
pub(crate) struct PreparedStructuredQuery {
    pushdown_filters: Vec<Filter>,
    filter: Option<PreparedFilter>,
    order_by: Vec<PreparedOrder>,
    projection: ProjectionMode,
    start_at: Option<PreparedCursor>,
    end_at: Option<PreparedCursor>,
    offset: usize,
    limit: Option<usize>,
    required_index_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectionMode {
    AllFields,
    SelectedFields(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedField {
    UserField(String),
    DocumentName,
}

impl PreparedField {
    fn user_field(&self) -> Option<&str> {
        match self {
            Self::UserField(field) => Some(field),
            Self::DocumentName => None,
        }
    }

    fn display_name(&self) -> &str {
        match self {
            Self::UserField(field) => field,
            Self::DocumentName => "__name__",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PreparedFilter {
    Composite {
        op: CompositeOperator,
        filters: Vec<PreparedFilter>,
    },
    Field {
        field: PreparedField,
        op: FieldFilterOperator,
        value: Value,
    },
    Unary {
        field: PreparedField,
        op: UnaryFilterOperator,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedOrder {
    field: PreparedField,
    direction: OrderDirection,
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedCursor {
    values: Vec<Value>,
    before: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentNameMode {
    LeafId,
    RelativePath,
}

#[derive(Debug, Clone)]
pub(crate) struct StructuredDocumentRow {
    pub(crate) document: Document,
    pub(crate) document_name: String,
    pub(crate) document_path: Option<DocumentPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CollectionGroupTableTarget {
    pub(crate) table: TableName,
    pub(crate) collection_path: CollectionPath,
}

#[derive(Debug, Default)]
struct PreparedFilterStatistics {
    referenced_fields: Vec<String>,
    inequality_fields: Vec<String>,
    has_or: bool,
    has_in: bool,
    has_not_in: bool,
    array_contains_any_count: usize,
    negative_filter_count: usize,
}

fn unsupported_structured_query_feature(feature: &str) -> Error {
    Error::InvalidInput(format!(
        "structured query feature not yet supported: {feature}"
    ))
}

fn unsupported_structured_aggregation_feature(feature: &str) -> Error {
    Error::InvalidInput(format!(
        "structured aggregation feature not yet supported: {feature}"
    ))
}

fn validate_structured_aggregation_query(query: &StructuredAggregationQuery) -> Result<()> {
    if query.aggregations.is_empty() {
        return Err(Error::InvalidInput(
            "structured aggregation queries must include at least one aggregation".to_string(),
        ));
    }
    if query.aggregations.len() > 5 {
        return Err(Error::InvalidInput(
            "structured aggregation queries support at most five aggregations".to_string(),
        ));
    }

    let mut seen_aliases = HashSet::new();
    for aggregation in &query.aggregations {
        if aggregation.alias.trim().is_empty() {
            return Err(Error::InvalidInput(
                "structured aggregation aliases must not be empty".to_string(),
            ));
        }
        if !seen_aliases.insert(aggregation.alias.clone()) {
            return Err(Error::InvalidInput(format!(
                "structured aggregation alias `{}` must be unique",
                aggregation.alias
            )));
        }
        match &aggregation.operator {
            AggregationOperator::Count(CountAggregation { up_to: Some(0) }) => {
                return Err(Error::InvalidInput(
                    "structured aggregation count `up_to` must be greater than zero".to_string(),
                ));
            }
            AggregationOperator::Count(_) => {}
            AggregationOperator::Sum(_) => {
                return Err(unsupported_structured_aggregation_feature(
                    "sum aggregations",
                ));
            }
            AggregationOperator::Avg(_) => {
                return Err(unsupported_structured_aggregation_feature(
                    "avg aggregations",
                ));
            }
        }
    }
    Ok(())
}

fn count_scan_limit(aggregations: &[StructuredAggregation]) -> Option<u32> {
    let mut max_up_to = None;
    for aggregation in aggregations {
        let AggregationOperator::Count(count) = &aggregation.operator else {
            return None;
        };
        let up_to = count.up_to?;
        let up_to = u32::try_from(up_to).unwrap_or(u32::MAX);
        max_up_to = Some(max_up_to.map_or(up_to, |current: u32| current.max(up_to)));
    }
    max_up_to
}

fn apply_structured_aggregation_limit(
    query: &StructuredQuery,
    aggregations: &[StructuredAggregation],
) -> StructuredQuery {
    let mut query = query.clone();
    if let Some(count_limit) = count_scan_limit(aggregations) {
        query.limit = Some(
            query
                .limit
                .map_or(count_limit, |limit| limit.min(count_limit)),
        );
    }
    query
}

fn count_value(count: u64) -> Result<Value> {
    let count = i64::try_from(count).map_err(|_| {
        Error::ResourceExhausted(
            "structured aggregation count exceeds Firestore int64 range".to_string(),
        )
    })?;
    Ok(Value::Number(Number::from(count)))
}

fn structured_aggregation_result_from_count(
    aggregations: &[StructuredAggregation],
    matched_document_count: usize,
) -> Result<StructuredAggregationResult> {
    let matched_document_count = u64::try_from(matched_document_count).map_err(|_| {
        Error::ResourceExhausted(
            "structured aggregation document count exceeds supported range".to_string(),
        )
    })?;
    let mut aggregate_fields = Map::new();
    for aggregation in aggregations {
        let value = match &aggregation.operator {
            AggregationOperator::Count(count) => count_value(
                count
                    .up_to
                    .map(|up_to| matched_document_count.min(up_to))
                    .unwrap_or(matched_document_count),
            )?,
            AggregationOperator::Sum(_) => {
                return Err(unsupported_structured_aggregation_feature(
                    "sum aggregations",
                ));
            }
            AggregationOperator::Avg(_) => {
                return Err(unsupported_structured_aggregation_feature(
                    "avg aggregations",
                ));
            }
        };
        aggregate_fields.insert(aggregation.alias.clone(), value);
    }
    Ok(StructuredAggregationResult { aggregate_fields })
}

fn push_unique(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

impl Service {
    /// Evaluates a structured aggregation query for one logical table.
    pub fn aggregate_documents_structured_with_principal_cancellable(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        aggregation_query: &StructuredAggregationQuery,
        principal: &PrincipalContext,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<StructuredAggregationResult> {
        validate_structured_aggregation_query(aggregation_query)?;
        let structured_query = apply_structured_aggregation_limit(
            &aggregation_query.structured_query,
            &aggregation_query.aggregations,
        );
        let documents = self.query_documents_structured_with_principal_cancellable(
            tenant_id,
            table,
            &structured_query,
            principal,
            check_cancel,
        )?;
        structured_aggregation_result_from_count(&aggregation_query.aggregations, documents.len())
    }

    /// Evaluates the currently supported structured-query subset for a tenant.
    pub fn query_documents_structured(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        query: &StructuredQuery,
    ) -> Result<Vec<Document>> {
        self.query_documents_structured_with_principal_cancellable(
            tenant_id,
            table,
            query,
            &PrincipalContext::anonymous(),
            &mut || Ok(()),
        )
    }

    /// Evaluates the currently supported structured-query subset for a principal.
    pub fn query_documents_structured_with_principal(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        query: &StructuredQuery,
        principal: &PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_structured_with_principal_cancellable(
            tenant_id,
            table,
            query,
            principal,
            &mut || Ok(()),
        )
    }

    /// Evaluates the currently supported structured-query subset asynchronously.
    pub async fn query_documents_structured_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        query: StructuredQuery,
    ) -> Result<Vec<Document>> {
        self.query_documents_structured_async_cancellable(
            tenant_id,
            table,
            query,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Evaluates the currently supported structured-query subset asynchronously for a principal.
    pub async fn query_documents_structured_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        query: StructuredQuery,
        principal: PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_structured_async_cancellable_with_principal(
            tenant_id,
            table,
            query,
            principal,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Evaluates the currently supported structured-query subset asynchronously with cancellation.
    pub async fn query_documents_structured_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        query: StructuredQuery,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.query_documents_structured_async_cancellable_with_principal(
            tenant_id,
            table,
            query,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Evaluates the currently supported structured-query subset asynchronously for a principal with cancellation.
    pub async fn query_documents_structured_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        query: StructuredQuery,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let prepared = prepare_structured_query(&table, &query)?;
        let schema = self.get_schema_async(tenant_id.clone()).await?;
        ensure_structured_query_index(schema.get_table(&table), &prepared)?;
        let base_query = structured_base_query(&table, &prepared);
        let documents = self
            .query_documents_async_cancellable_with_principal(
                tenant_id,
                base_query,
                principal,
                cancel_wait,
                check_cancel,
            )
            .await?;
        let mut no_op_cancel = || Ok(());
        finalize_structured_documents(documents, &prepared, &mut no_op_cancel)
    }

    /// Evaluates the currently supported structured-query subset while checking for cancellation between rows.
    pub fn query_documents_structured_with_principal_cancellable(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        query: &StructuredQuery,
        principal: &PrincipalContext,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let prepared = prepare_structured_query(table, query)?;
        let schema = self.get_schema(tenant_id)?;
        ensure_structured_query_index(schema.get_table(table), &prepared)?;
        let base_query = structured_base_query(table, &prepared);
        let documents = self.query_documents_with_principal_cancellable(
            tenant_id,
            &base_query,
            principal,
            check_cancel,
        )?;
        finalize_structured_documents(documents, &prepared, check_cancel)
    }

    /// Evaluates a structured query across every collection path bound to one
    /// collection group, optionally scoped to descendants of an ancestor
    /// document path.
    pub fn query_collection_group_documents_structured_with_principal_cancellable(
        &self,
        tenant_id: &TenantId,
        collection_group: &CollectionName,
        ancestor: Option<&DocumentPath>,
        query: &StructuredQuery,
        principal: &PrincipalContext,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<(DocumentPath, Document)>> {
        let prepared = prepare_collection_group_structured_query(query)?;
        let runtime = self.get_existing_tenant(tenant_id)?;
        let snapshot = runtime.store.read_snapshot()?;
        let targets = collection_group_table_targets(
            snapshot.scan_collection_group_bindings(collection_group)?,
            ancestor,
        );

        let schema = self.get_schema(tenant_id)?;
        for target in &targets {
            ensure_structured_query_index(schema.get_table(&target.table), &prepared)?;
        }

        let mut rows = Vec::new();
        for target in targets {
            check_cancel()?;
            let base_query = structured_base_query(&target.table, &prepared);
            let documents = self.query_documents_with_principal_cancellable(
                tenant_id,
                &base_query,
                principal,
                check_cancel,
            )?;
            rows.extend(documents.into_iter().map(|document| {
                let document_path =
                    DocumentPath::new(target.collection_path.clone(), document.id.clone());
                StructuredDocumentRow {
                    document_name: document_path.to_string(),
                    document,
                    document_path: Some(document_path),
                }
            }));
        }

        finalize_structured_rows(rows, &prepared, check_cancel).map(|rows| {
            rows.into_iter()
                .map(|row| {
                    (
                        row.document_path
                            .expect("collection-group rows should preserve document paths"),
                        row.document,
                    )
                })
                .collect::<Vec<_>>()
        })
    }

    /// Evaluates a structured aggregation query across every collection path
    /// bound to one collection group.
    pub fn aggregate_collection_group_documents_structured_with_principal_cancellable(
        &self,
        tenant_id: &TenantId,
        collection_group: &CollectionName,
        ancestor: Option<&DocumentPath>,
        aggregation_query: &StructuredAggregationQuery,
        principal: &PrincipalContext,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<StructuredAggregationResult> {
        validate_structured_aggregation_query(aggregation_query)?;
        let structured_query = apply_structured_aggregation_limit(
            &aggregation_query.structured_query,
            &aggregation_query.aggregations,
        );
        let rows = self.query_collection_group_documents_structured_with_principal_cancellable(
            tenant_id,
            collection_group,
            ancestor,
            &structured_query,
            principal,
            check_cancel,
        )?;
        structured_aggregation_result_from_count(&aggregation_query.aggregations, rows.len())
    }
}
