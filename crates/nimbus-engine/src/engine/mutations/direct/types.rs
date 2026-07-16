use nimbus_core::{DocumentId, Error, PrincipalContext, Result};

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
pub(in crate::engine::mutations) enum MutationExecutionMode {
    Immediate,
    Scheduled { execution_id: String },
}

pub(in crate::engine::mutations) enum MutationExecutionResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

pub(super) fn expect_immediate_result(
    result: MutationExecutionResult,
    scheduled_message: &'static str,
) -> Result<Option<DocumentId>> {
    match result {
        MutationExecutionResult::Immediate(document_id) => Ok(document_id),
        MutationExecutionResult::Scheduled(_) => {
            Err(Error::Internal(scheduled_message.to_string()))
        }
    }
}

pub(super) fn expect_scheduled_applied(
    result: MutationExecutionResult,
    immediate_message: &'static str,
) -> Result<bool> {
    match result {
        MutationExecutionResult::Scheduled(applied) => Ok(applied),
        MutationExecutionResult::Immediate(_) => {
            Err(Error::Internal(immediate_message.to_string()))
        }
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

#[cfg(test)]
mod tests {
    use nimbus_core::{DocumentId, Error};

    use super::{MutationExecutionResult, expect_immediate_result, expect_scheduled_applied};

    #[test]
    fn expect_immediate_result_reports_scheduled_variant_as_internal_error() {
        let error = expect_immediate_result(
            MutationExecutionResult::Scheduled(false),
            "scheduled result",
        )
        .expect_err("scheduled result should be rejected by immediate helper");

        assert!(matches!(error, Error::Internal(message) if message == "scheduled result"));
    }

    #[test]
    fn expect_scheduled_applied_reports_immediate_variant_as_internal_error() {
        let document_id = DocumentId::from_key("unexpected").expect("document id should parse");
        let error = expect_scheduled_applied(
            MutationExecutionResult::Immediate(Some(document_id)),
            "immediate result",
        )
        .expect_err("immediate result should be rejected by scheduled helper");

        assert!(matches!(error, Error::Internal(message) if message == "immediate result"));
    }

    #[test]
    fn expect_result_helpers_return_matching_variants() {
        let document_id = DocumentId::from_key("expected").expect("document id should parse");
        assert_eq!(
            expect_immediate_result(
                MutationExecutionResult::Immediate(Some(document_id.clone())),
                "scheduled result",
            )
            .expect("immediate result should pass"),
            Some(document_id)
        );
        assert!(
            expect_scheduled_applied(MutationExecutionResult::Scheduled(true), "immediate result")
                .expect("scheduled result should pass")
        );
    }
}
