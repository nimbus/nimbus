use nimbus_core::{CommitEntry, Document, TenantId, commit_intersects_dependency_set};

use super::super::RuntimeReadSet;

pub fn commit_intersects_runtime_read_set(
    engine: &nimbus_engine::Engine,
    tenant_id: &TenantId,
    commit: &CommitEntry,
    read_set: &RuntimeReadSet,
    deleted_documents: &[Document],
) -> bool {
    let dependencies = read_set.dependency_set();
    commit_intersects_dependency_set(
        commit,
        &dependencies,
        deleted_documents,
        |table, document_id| engine.get_document(tenant_id, table, document_id).map(Some),
    )
}
