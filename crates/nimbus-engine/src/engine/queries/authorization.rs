use nimbus_core::{
    AccessAction, AccessRule, Document, Filter, PrincipalContext, Query, Result, TableSchema,
};

#[derive(Debug, Clone)]
pub(crate) struct ReadAuthorization {
    rule: Option<AccessRule>,
    planner_filters: Vec<Filter>,
    pub(crate) impossible: bool,
}

impl ReadAuthorization {
    pub(crate) fn for_table(
        table_schema: Option<&TableSchema>,
        principal: &PrincipalContext,
    ) -> Result<Self> {
        let rule = table_schema
            .and_then(|table_schema| table_schema.access_policy.as_ref())
            .map(|policy| policy.rule_for(AccessAction::Read).clone())
            .filter(|rule| !rule.is_unrestricted());
        let Some(rule) = rule else {
            return Ok(Self {
                rule: None,
                planner_filters: Vec::new(),
                impossible: false,
            });
        };

        let compiled = rule.compile_read_filters(principal)?;
        Ok(Self {
            rule: Some(rule),
            planner_filters: compiled.planner_filters,
            impossible: compiled.impossible,
        })
    }

    pub(crate) fn merge_query(&self, query: &Query) -> Query {
        if self.planner_filters.is_empty() {
            return query.clone();
        }

        let mut merged = query.clone();
        merged.filters.extend(self.planner_filters.clone());
        merged
    }

    pub(crate) fn allows_document(
        &self,
        principal: &PrincipalContext,
        document: &Document,
    ) -> Result<bool> {
        match &self.rule {
            Some(rule) => rule.allows(principal, Some(document), None),
            None => Ok(true),
        }
    }
}
