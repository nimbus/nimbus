use std::collections::HashSet;

use neovex_core::{
    Document, DocumentId, PaginatedQuery, PaginatedWindowDependency, Query, Result, TableName,
};

use crate::evaluator::{
    decode_cursor, evaluate_paginated_with_docs_cancellable_and_predicate,
    evaluate_query_with_docs_cancellable_and_predicate,
};

use super::super::queries::ReadAuthorization;
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

        let document = self.current_document(table, document_id)?;
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
        document_id: DocumentId,
    ) -> Result<Option<Document>> {
        let state = self.active_state()?;
        if let Some(entry) = state.staged_writes.get(&(table.clone(), document_id)) {
            return Ok(entry.current.clone());
        }
        drop(state);
        self.snapshot.get(table, &document_id)
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
                (entry_table == table).then_some(*document_id)
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
                end_doc_id: documents.last().map(|document| document.id),
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
