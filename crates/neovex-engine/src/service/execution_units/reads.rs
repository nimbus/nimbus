use std::collections::HashSet;

use neovex_core::{
    CollectionName, Document, DocumentId, DocumentPath, PaginatedQuery, PaginatedWindowDependency,
    Query, Result, StructuredQuery, TableName,
};

use crate::evaluator::{
    decode_cursor, evaluate_paginated_with_docs_cancellable_and_predicate,
    evaluate_query_with_docs_cancellable_and_predicate,
};

use super::super::queries::{
    ReadAuthorization, StructuredDocumentRow, collection_group_table_targets,
    ensure_structured_query_index, finalize_structured_documents, finalize_structured_rows,
    prepare_collection_group_structured_query, prepare_structured_query, structured_base_query,
};
use super::MutationExecutionUnit;

impl MutationExecutionUnit {
    pub fn get_document(
        &self,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Option<Document>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(table);
        let authorization = ReadAuthorization::for_table(table_schema, &self.principal)?;
        if authorization.impossible {
            return Ok(None);
        }

        let document = self.current_document(table, &document_id)?;
        self.active_state()?
            .read_dependencies
            .record_document(table, document_id);

        match document {
            Some(document) if authorization.allows_document(&self.principal, &document)? => {
                Ok(Some(document))
            }
            Some(_) | None => Ok(None),
        }
    }

    pub fn query_documents_cancellable(
        &self,
        query: &Query,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(&query.table);
        let authorization = ReadAuthorization::for_table(table_schema, &self.principal)?;
        if authorization.impossible {
            return Ok(Vec::new());
        }

        let merged_query = authorization.merge_query(query);
        self.record_query_dependency(&merged_query)?;
        let documents = self.materialize_table_view(&query.table, check_cancel)?;
        let mut include_document =
            |document: &Document| authorization.allows_document(&self.principal, document);
        let result = evaluate_query_with_docs_cancellable_and_predicate(
            documents,
            &merged_query,
            check_cancel,
            &mut include_document,
        )?;
        if let Some(limit) = query.limit {
            self.record_limited_window_dependency(&merged_query, limit, &result)?;
        }
        Ok(result)
    }

    pub fn query_documents_structured_cancellable(
        &self,
        table: &TableName,
        query: &StructuredQuery,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let prepared = prepare_structured_query(table, query)?;
        ensure_structured_query_index(self.schema_snapshot.get_table(table), &prepared)?;
        let base_query = structured_base_query(table, &prepared);
        let documents = self.query_documents_cancellable(&base_query, check_cancel)?;
        finalize_structured_documents(documents, &prepared, check_cancel)
    }

    pub fn query_collection_group_documents_structured_cancellable(
        &self,
        collection_group: &CollectionName,
        ancestor: Option<&DocumentPath>,
        query: &StructuredQuery,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<(DocumentPath, Document)>> {
        let prepared = prepare_collection_group_structured_query(query)?;
        let mut bindings = self
            .snapshot
            .scan_collection_group_bindings(collection_group)?;
        let state = self.active_state()?;
        bindings.extend(
            state
                .staged_writes
                .values()
                .filter_map(|entry| entry.resource_path_binding.clone())
                .filter(|binding| binding.collection_group() == collection_group),
        );
        drop(state);

        let targets = collection_group_table_targets(bindings, ancestor);
        for target in &targets {
            ensure_structured_query_index(
                self.schema_snapshot.get_table(&target.table),
                &prepared,
            )?;
        }

        let mut rows = Vec::new();
        for target in targets {
            check_cancel()?;
            let base_query = structured_base_query(&target.table, &prepared);
            let documents = self.query_documents_cancellable(&base_query, check_cancel)?;
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
                .collect()
        })
    }

    pub fn paginate_documents_cancellable(
        &self,
        query: &PaginatedQuery,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<neovex_core::Page> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(&query.query.table);
        let authorization = ReadAuthorization::for_table(table_schema, &self.principal)?;
        if authorization.impossible {
            let empty = neovex_core::Page {
                data: Vec::new(),
                has_more: false,
                next_cursor: None,
            };
            self.record_paginated_window_dependency(query, &empty)?;
            return Ok(empty);
        }

        let merged_query = authorization.merge_query(&query.query);
        let merged_paginated = PaginatedQuery {
            query: merged_query.clone(),
            page_size: query.page_size,
            after: query.after.clone(),
        };
        let documents = self.materialize_table_view(&query.query.table, check_cancel)?;
        let mut include_document =
            |document: &Document| authorization.allows_document(&self.principal, document);
        let page = evaluate_paginated_with_docs_cancellable_and_predicate(
            documents,
            &merged_paginated,
            check_cancel,
            &mut include_document,
        )?;
        self.record_paginated_window_dependency(&merged_paginated, &page)?;
        Ok(page)
    }

    pub(super) fn current_document(
        &self,
        table: &TableName,
        document_id: &DocumentId,
    ) -> Result<Option<Document>> {
        let state = self.active_state()?;
        if let Some(entry) = state
            .staged_writes
            .get(&(table.clone(), document_id.clone()))
        {
            return Ok(entry.current.clone());
        }
        drop(state);
        self.snapshot.get(table, document_id)
    }

    fn materialize_table_view(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.ensure_active()?;
        let mut documents =
            self.snapshot
                .scan_table_matching_cancellable(table, check_cancel, |_document| Ok(true))?;
        let state = self.active_state()?;
        let staged_ids = state
            .staged_writes
            .iter()
            .filter_map(|((entry_table, document_id), _)| {
                (entry_table == table).then_some(document_id.clone())
            })
            .collect::<HashSet<_>>();
        if !staged_ids.is_empty() {
            documents.retain(|document| !staged_ids.contains(&document.id));
        }
        for ((entry_table, _document_id), entry) in &state.staged_writes {
            if entry_table != table {
                continue;
            }
            if let Some(document) = entry.current.clone() {
                documents.push(document);
            }
        }
        Ok(documents)
    }

    fn record_query_dependency(&self, query: &Query) -> Result<()> {
        let mut state = self.active_state()?;
        if query.filters.is_empty() {
            state.read_dependencies.record_table(&query.table);
        } else {
            state
                .read_dependencies
                .record_predicate(neovex_core::PredicateDependency {
                    table: query.table.clone(),
                    filters: query.filters.clone(),
                });
        }
        Ok(())
    }

    fn record_limited_window_dependency(
        &self,
        query: &Query,
        limit: usize,
        documents: &[Document],
    ) -> Result<()> {
        if query.order.is_none() {
            return Ok(());
        }
        self.active_state()?
            .read_dependencies
            .record_paginated_window(PaginatedWindowDependency {
                table: query.table.clone(),
                filters: query.filters.clone(),
                order: query.order.clone(),
                start_sort_values: Vec::new(),
                start_doc_id: None,
                end_sort_values: documents
                    .last()
                    .map(|document| match query.order.as_ref() {
                        Some(order) => vec![document.get_field(&order.field).cloned()],
                        None => Vec::new(),
                    })
                    .unwrap_or_default(),
                end_doc_id: documents.last().map(|document| document.id.clone()),
                result_count: documents.len(),
                page_size: limit,
            });
        Ok(())
    }

    fn record_paginated_window_dependency(
        &self,
        paginated: &PaginatedQuery,
        page: &neovex_core::Page,
    ) -> Result<()> {
        let (start_sort_values, start_doc_id) = paginated
            .after
            .as_ref()
            .map(|cursor| decode_cursor(cursor, &paginated.query))
            .transpose()?
            .map_or((Vec::new(), None), |(sort_values, document_id)| {
                (sort_values, Some(document_id))
            });
        let end_document = page
            .data
            .last()
            .and_then(|value| value.get("_id").and_then(serde_json::Value::as_str))
            .and_then(|value| value.parse::<DocumentId>().ok());
        let end_sort_values = page
            .data
            .last()
            .map(|value| match paginated.query.order.as_ref() {
                Some(order) => vec![value.get(&order.field).cloned()],
                None => Vec::new(),
            })
            .unwrap_or_default();
        self.active_state()?
            .read_dependencies
            .record_paginated_window(PaginatedWindowDependency {
                table: paginated.query.table.clone(),
                filters: paginated.query.filters.clone(),
                order: paginated.query.order.clone(),
                start_sort_values,
                start_doc_id,
                end_sort_values,
                end_doc_id: end_document,
                result_count: page.data.len(),
                page_size: paginated.page_size,
            });
        Ok(())
    }
}
