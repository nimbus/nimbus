mod authorization;
mod collection_ids;
mod documents;
mod journal;
mod materialized;
mod planner;
mod prepared;
mod query_api;
mod snapshot;
mod structured;
mod test_hooks;
mod verification;

pub(crate) use authorization::ReadAuthorization;
pub(crate) use materialized::{
    evaluate_with_index_cancellable_for_principal, should_use_materialized_surface_for_query,
};
#[cfg(test)]
pub(crate) use prepared::paginate_documents_for_docs_with_principal;
pub(crate) use prepared::{
    paginate_documents_for_store_with_principal, query_documents_for_docs_with_principal,
    query_documents_for_snapshot_and_principal_cancellable,
    query_documents_for_store_with_principal,
};
pub(crate) use snapshot::snapshot_table_documents;
pub(crate) use structured::{
    StructuredDocumentRow, collection_group_table_targets, ensure_structured_query_index,
    finalize_structured_documents, finalize_structured_rows,
    prepare_collection_group_structured_query, prepare_structured_query, structured_base_query,
};
