use neovex_core::{Document, Query, Result};
use neovex_storage::TenantStore;

use super::filtering::{filter_documents_cancellable, matches_filters};
use super::ordering::sort_documents;

/// Evaluates a query against a tenant store.
pub fn evaluate_query(store: &TenantStore, query: &Query) -> Result<Vec<Document>> {
    evaluate_query_cancellable(store, query, &mut || Ok(()))
}

/// Evaluates a query against a tenant store while checking for cancellation between rows.
pub fn evaluate_query_cancellable(
    store: &TenantStore,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    evaluate_query_cancellable_with_predicate(store, query, check_cancel, &mut |_| Ok(true))
}

/// Evaluates a query using preloaded documents instead of scanning the store.
pub fn evaluate_query_with_docs(documents: Vec<Document>, query: &Query) -> Result<Vec<Document>> {
    evaluate_query_with_docs_cancellable(documents, query, &mut || Ok(()))
}

/// Evaluates a query using preloaded documents while checking for cancellation between rows.
pub fn evaluate_query_with_docs_cancellable(
    documents: Vec<Document>,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    evaluate_query_with_docs_cancellable_and_predicate(documents, query, check_cancel, &mut |_| {
        Ok(true)
    })
}

pub(crate) fn evaluate_query_cancellable_with_predicate<F>(
    store: &TenantStore,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
    include_document: &mut F,
) -> Result<Vec<Document>>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let filtered = store.scan_table_matching_with_filters_cancellable(
        &query.table,
        &query.filters,
        check_cancel,
        |document| Ok(matches_filters(document, &query.filters)? && include_document(document)?),
    )?;
    finalize_query_documents(filtered, query, check_cancel)
}

pub(crate) fn evaluate_query_with_docs_cancellable_and_predicate<F>(
    documents: Vec<Document>,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
    include_document: &mut F,
) -> Result<Vec<Document>>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let filtered =
        filter_documents_cancellable(documents, &query.filters, check_cancel, include_document)?;
    finalize_query_documents(filtered, query, check_cancel)
}

fn finalize_query_documents(
    mut filtered: Vec<Document>,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    check_cancel()?;
    sort_documents(&mut filtered, query.order.as_ref())?;
    check_cancel()?;
    if let Some(limit) = query.limit {
        filtered.truncate(limit);
    }
    Ok(filtered)
}
