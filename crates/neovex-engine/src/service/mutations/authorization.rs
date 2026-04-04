use neovex_core::{
    AccessAction, AccessRule, Document, Error, PrincipalContext, Result, TableSchema,
};

fn mutation_access_rule(
    table_schema: Option<&TableSchema>,
    action: AccessAction,
) -> Option<&AccessRule> {
    table_schema
        .and_then(|table_schema| table_schema.access_policy.as_ref())
        .map(|policy| policy.rule_for(action))
        .filter(|rule| !rule.is_unrestricted())
}

pub(crate) fn enforce_mutation_authorization(
    table_schema: Option<&TableSchema>,
    action: AccessAction,
    principal: &PrincipalContext,
    candidate_document: Option<&Document>,
    existing_document: Option<&Document>,
) -> Result<()> {
    let Some(rule) = mutation_access_rule(table_schema, action) else {
        return Ok(());
    };

    if rule.allows(principal, candidate_document, existing_document)? {
        return Ok(());
    }

    Err(Error::PermissionDenied(match action {
        AccessAction::Create => "create access denied".to_string(),
        AccessAction::Update => "update access denied".to_string(),
        AccessAction::Delete => "delete access denied".to_string(),
        AccessAction::Read => "read access denied".to_string(),
    }))
}
