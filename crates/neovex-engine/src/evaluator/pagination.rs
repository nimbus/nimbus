use std::cmp::Ordering;

use neovex_core::{Document, Error, Page, PaginatedQuery, Result};
use neovex_storage::TenantStore;

use super::cursor::{
    compare_document_to_cursor, cursor_sort_values_for_document, decode_cursor, encode_cursor,
};
use super::filtering::{filter_documents_cancellable, matches_filters};
use super::ordering::sort_documents;

/// Evaluates a paginated query.
pub fn evaluate_paginated(store: &TenantStore, paginated: &PaginatedQuery) -> Result<Page> {
    evaluate_paginated_cancellable(store, paginated, &mut || Ok(()))
}

/// Evaluates a paginated query while checking for cancellation between rows.
pub fn evaluate_paginated_cancellable(
    store: &TenantStore,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    evaluate_paginated_cancellable_with_predicate(store, paginated, check_cancel, &mut |_| Ok(true))
}

/// Evaluates a paginated query using preloaded documents instead of scanning the store.
pub fn evaluate_paginated_with_docs(
    documents: Vec<Document>,
    paginated: &PaginatedQuery,
) -> Result<Page> {
    evaluate_paginated_with_docs_cancellable(documents, paginated, &mut || Ok(()))
}

/// Evaluates a paginated query using preloaded documents while checking for cancellation.
pub fn evaluate_paginated_with_docs_cancellable(
    documents: Vec<Document>,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    evaluate_paginated_with_docs_cancellable_and_predicate(
        documents,
        paginated,
        check_cancel,
        &mut |_| Ok(true),
    )
}

pub(crate) fn evaluate_paginated_cancellable_with_predicate<F>(
    store: &TenantStore,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
    include_document: &mut F,
) -> Result<Page>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let filtered = store.scan_table_matching_with_filters_cancellable(
        &paginated.query.table,
        &paginated.query.filters,
        check_cancel,
        |document| {
            Ok(matches_filters(document, &paginated.query.filters)? && include_document(document)?)
        },
    )?;
    evaluate_paginated_with_filtered_docs_cancellable(filtered, paginated, check_cancel)
}

pub(crate) fn evaluate_paginated_with_docs_cancellable_and_predicate<F>(
    documents: Vec<Document>,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
    include_document: &mut F,
) -> Result<Page>
where
    F: FnMut(&Document) -> Result<bool>,
{
    if paginated.page_size == 0 {
        return Err(Error::InvalidInput(
            "page_size must be greater than zero".to_string(),
        ));
    }

    let mut unbounded_query = paginated.query.clone();
    unbounded_query.limit = None;
    let filtered = filter_documents_cancellable(
        documents,
        &unbounded_query.filters,
        check_cancel,
        include_document,
    )?;
    evaluate_paginated_with_filtered_docs_cancellable(filtered, paginated, check_cancel)
}

fn evaluate_paginated_with_filtered_docs_cancellable(
    mut filtered: Vec<Document>,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    if paginated.page_size == 0 {
        return Err(Error::InvalidInput(
            "page_size must be greater than zero".to_string(),
        ));
    }

    let mut unbounded_query = paginated.query.clone();
    unbounded_query.limit = None;
    check_cancel()?;
    sort_documents(&mut filtered, unbounded_query.order.as_ref())?;

    let start_index = if let Some(cursor) = &paginated.after {
        let (cursor_sort_values, cursor_doc_id) = decode_cursor(cursor, &unbounded_query)?;
        let mut start = filtered.len();
        for (index, document) in filtered.iter().enumerate() {
            check_cancel()?;
            if compare_document_to_cursor(
                document,
                paginated.query.order.as_ref(),
                &cursor_sort_values,
                &cursor_doc_id,
            )? == Ordering::Greater
            {
                start = index;
                break;
            }
        }
        start
    } else {
        0
    };

    let remaining = &filtered[start_index..];
    let window: Vec<_> = remaining.iter().take(paginated.page_size + 1).collect();
    let has_more = window.len() > paginated.page_size;
    let page_docs = window
        .into_iter()
        .take(paginated.page_size)
        .collect::<Vec<_>>();

    let next_cursor = if has_more {
        check_cancel()?;
        page_docs
            .last()
            .map(|document| {
                let sort_values =
                    cursor_sort_values_for_document(paginated.query.order.as_ref(), document);
                encode_cursor(&sort_values, &document.id, &unbounded_query)
            })
            .transpose()?
    } else {
        None
    };

    check_cancel()?;
    Ok(Page {
        data: page_docs
            .into_iter()
            .map(|document| document.to_json())
            .collect(),
        next_cursor,
        has_more,
    })
}
