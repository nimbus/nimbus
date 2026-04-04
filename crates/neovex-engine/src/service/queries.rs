mod authorization;
mod documents;
mod journal;
mod materialized;
mod planner;
mod prepared;
mod query_api;
mod snapshot;
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
