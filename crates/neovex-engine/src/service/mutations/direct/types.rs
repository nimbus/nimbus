use neovex_core::{DocumentId, Error, PrincipalContext, Result, TableName};

#[derive(Clone)]
pub(in crate::service::mutations) enum MutationExecutionMode {
    Immediate,
    Scheduled { execution_id: String },
}

pub(in crate::service::mutations) enum MutationExecutionResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

pub(super) struct UpdateMutationRequest<'a> {
    pub(super) table: TableName,
    pub(super) id: DocumentId,
    pub(super) patch: serde_json::Map<String, serde_json::Value>,
    pub(super) principal: &'a PrincipalContext,
}

pub(super) fn expect_immediate_result(
    result: MutationExecutionResult,
    scheduled_message: &'static str,
) -> Option<DocumentId> {
    match result {
        MutationExecutionResult::Immediate(document_id) => document_id,
        MutationExecutionResult::Scheduled(_) => unreachable!("{scheduled_message}"),
    }
}

pub(super) fn expect_scheduled_applied(
    result: MutationExecutionResult,
    immediate_message: &'static str,
) -> bool {
    match result {
        MutationExecutionResult::Scheduled(applied) => applied,
        MutationExecutionResult::Immediate(_) => unreachable!("{immediate_message}"),
    }
}

pub(super) fn expect_immediate_document_id(
    document_id: Option<DocumentId>,
    missing_message: &'static str,
) -> Result<DocumentId> {
    document_id.ok_or_else(|| Error::Internal(missing_message.to_string()))
}

pub(super) fn expect_immediate_unit(
    document_id: Option<DocumentId>,
    unexpected_message: &'static str,
) -> Result<()> {
    match document_id {
        None => Ok(()),
        Some(_) => Err(Error::Internal(unexpected_message.to_string())),
    }
}
