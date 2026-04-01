use neovex_core::{CommitEntry, Document, DocumentId, TableName, TenantId, WriteOpType};

use super::super::RuntimeReadSet;
use super::matching::{
    document_matches_index_read, document_matches_predicate_read,
    document_may_affect_paginated_window,
};

pub(crate) fn commit_intersects_runtime_read_set(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    commit: &CommitEntry,
    read_set: &RuntimeReadSet,
    deleted_documents: &[Document],
) -> bool {
    commit.writes.iter().any(|write| {
        write_intersects_runtime_read_set(service, tenant_id, write, read_set, deleted_documents)
    })
}

fn write_intersects_runtime_read_set(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    write: &neovex_core::WriteOp,
    read_set: &RuntimeReadSet,
    deleted_documents: &[Document],
) -> bool {
    if read_set.tables.contains(&write.table) {
        return true;
    }

    if read_set
        .documents
        .contains(&(write.table.clone(), write.doc_id))
    {
        return true;
    }

    let relevant_predicates = read_set
        .predicates
        .iter()
        .filter(|read| read.table == write.table)
        .collect::<Vec<_>>();
    let relevant_paginated_windows = read_set
        .paginated_windows
        .iter()
        .filter(|read| read.table == write.table)
        .collect::<Vec<_>>();
    let mut relevant_index_reads = read_set
        .index_ranges
        .iter()
        .filter(|read| read.table == write.table);

    match write.op_type {
        WriteOpType::Delete => {
            if let Some(document) =
                deleted_document_snapshot(&write.table, &write.doc_id, deleted_documents)
            {
                if relevant_paginated_windows
                    .iter()
                    .any(|read| document_may_affect_paginated_window(document, read))
                {
                    return true;
                }
                if relevant_predicates
                    .iter()
                    .any(|read| document_matches_predicate_read(document, read))
                {
                    return true;
                }
                return relevant_index_reads.any(|read| {
                    document_matches_index_read(document.get_field(&read.field), read)
                });
            }

            !relevant_predicates.is_empty()
                || !relevant_paginated_windows.is_empty()
                || relevant_index_reads.count() > 0
        }
        WriteOpType::Insert | WriteOpType::Update => {
            let Ok(document) = service.get_document(tenant_id, &write.table, write.doc_id) else {
                return true;
            };
            if relevant_paginated_windows
                .iter()
                .any(|read| document_may_affect_paginated_window(&document, read))
            {
                return true;
            }
            if relevant_predicates
                .iter()
                .any(|read| document_matches_predicate_read(&document, read))
            {
                return true;
            }
            relevant_index_reads
                .any(|read| document_matches_index_read(document.get_field(&read.field), read))
        }
    }
}

fn deleted_document_snapshot<'a>(
    table: &TableName,
    document_id: &DocumentId,
    deleted_documents: &'a [Document],
) -> Option<&'a Document> {
    deleted_documents
        .iter()
        .find(|document| &document.table == table && document.id == *document_id)
}
