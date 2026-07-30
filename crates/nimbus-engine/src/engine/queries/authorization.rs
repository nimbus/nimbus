use nimbus_core::{
    AccessAction, AccessRule, AccessValue, Document, Filter, PrincipalContext, Query, Result,
    TableSchema,
};

/// The reserved document fields a read rule can name that are engine lifecycle
/// metadata rather than stored fields.
const TIMESTAMP_FIELDS: [&str; 2] = ["_creationTime", "_updateTime"];

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

/// A table's read rule resolved for one principal, applicable to documents a
/// caller assembles itself instead of reading through a scan.
///
/// The scan APIs authorize what they read from the store. A caller that hands
/// back item-level data reconstructed from somewhere else — the DynamoDB
/// adapter returning stream records built from captured images is the case this
/// exists for — never passes through those APIs, and so needs the same rule
/// applied at the point it discloses the data.
#[derive(Debug, Clone)]
pub struct DocumentReadFilter {
    authorization: ReadAuthorization,
    principal: PrincipalContext,
}

impl DocumentReadFilter {
    pub(crate) fn new(authorization: ReadAuthorization, principal: PrincipalContext) -> Self {
        Self {
            authorization,
            principal,
        }
    }

    /// Whether the table restricts reads at all. When it does not, every
    /// document is readable and per-document evaluation can be skipped.
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        self.authorization.rule.is_none()
    }

    /// Whether no document at all can satisfy the rule for this principal —
    /// an unauthenticated caller against an authenticated-only table, or a rule
    /// resting on a claim the principal does not carry.
    #[must_use]
    pub fn denies_everything(&self) -> bool {
        self.authorization.impossible
    }

    /// Whether the rule's decision can depend on `_creationTime` or
    /// `_updateTime`.
    ///
    /// These are engine lifecycle metadata, not stored fields. A caller
    /// reconstructing documents from data that does not carry them cannot
    /// evaluate such a rule faithfully, and should withhold the document rather
    /// than authorize it against a guessed timestamp.
    #[must_use]
    pub fn depends_on_document_timestamps(&self) -> bool {
        let Some(rule) = &self.authorization.rule else {
            return false;
        };
        rule.predicates.iter().any(|predicate| {
            names_timestamp_field(&predicate.left) || names_timestamp_field(&predicate.right)
        })
    }

    /// Whether `document` is readable by the principal this filter resolved for.
    ///
    /// # Errors
    /// Propagates a rule evaluation error, such as an unorderable comparison.
    pub fn allows(&self, document: &Document) -> Result<bool> {
        self.authorization
            .allows_document(&self.principal, document)
    }
}

fn names_timestamp_field(value: &AccessValue) -> bool {
    match value {
        AccessValue::DocumentField { field } | AccessValue::ExistingDocumentField { field } => {
            TIMESTAMP_FIELDS.contains(&field.as_str())
        }
        AccessValue::Literal { .. } | AccessValue::PrincipalClaim { .. } => false,
    }
}
