use nimbus_core::{DocumentId, Error, PrincipalContext, Result, TableName};

#[derive(Clone, Copy, Default)]
pub enum MutationActor<'a> {
    #[default]
    Anonymous,
    Principal(&'a PrincipalContext),
}

impl<'a> MutationActor<'a> {
    pub const fn anonymous() -> Self {
        Self::Anonymous
    }

    pub const fn with_principal(principal: &'a PrincipalContext) -> Self {
        Self::Principal(principal)
    }

    pub(super) fn resolve(self, anonymous: &'a PrincipalContext) -> &'a PrincipalContext {
        match self {
            Self::Anonymous => anonymous,
            Self::Principal(principal) => principal,
        }
    }
}

pub struct AsyncMutationContext<Fut, Check> {
    pub(super) principal: PrincipalContext,
    pub(super) cancel_wait: Fut,
    pub(super) check_cancel: Check,
}

impl<Fut, Check> AsyncMutationContext<Fut, Check> {
    pub fn anonymous(cancel_wait: Fut, check_cancel: Check) -> Self {
        Self {
            principal: PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        }
    }

    pub fn with_principal(
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Self {
        Self {
            principal,
            cancel_wait,
            check_cancel,
        }
    }
}

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
