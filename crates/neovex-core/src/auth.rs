use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{Document, Error, Filter, FilterOp, Result};

/// Normalized authenticated principal context passed from the transport boundary
/// into the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PrincipalContext {
    #[serde(default)]
    pub authenticated: bool,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub claims: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub verified_claims: Map<String, Value>,
}

impl PrincipalContext {
    /// Returns an anonymous principal context.
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// Returns a stable snapshot fingerprint for subscription ownership and
    /// conservative invalidation.
    pub fn snapshot(&self) -> Result<PrincipalSnapshot> {
        let bytes =
            serde_json::to_vec(self).map_err(|error| Error::Serialization(error.to_string()))?;
        let digest = Sha256::digest(bytes);
        Ok(PrincipalSnapshot {
            digest: format!("{digest:x}"),
        })
    }

    fn claim(&self, source: PrincipalClaimSource, claim: &str) -> Option<&Value> {
        match source {
            PrincipalClaimSource::Identity => self.claims.get(claim),
            PrincipalClaimSource::VerifiedIdentity => self.verified_claims.get(claim),
        }
    }
}

/// Stable fingerprint of the principal context captured when a subscription was
/// registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalSnapshot {
    pub digest: String,
}

/// Table-local declarative access policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TableAccessPolicy {
    #[serde(default)]
    pub read: AccessRule,
    #[serde(default)]
    pub create: AccessRule,
    #[serde(default)]
    pub update: AccessRule,
    #[serde(default)]
    pub delete: AccessRule,
}

impl TableAccessPolicy {
    pub fn rule_for(&self, action: AccessAction) -> &AccessRule {
        match action {
            AccessAction::Read => &self.read,
            AccessAction::Create => &self.create,
            AccessAction::Update => &self.update,
            AccessAction::Delete => &self.delete,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.read.validate_for(AccessAction::Read)?;
        self.create.validate_for(AccessAction::Create)?;
        self.update.validate_for(AccessAction::Update)?;
        self.delete.validate_for(AccessAction::Delete)?;
        Ok(())
    }
}

/// Access operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessAction {
    Read,
    Create,
    Update,
    Delete,
}

/// Conjunctive access rule for a single operation kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AccessRule {
    #[serde(default)]
    pub require_authenticated: bool,
    #[serde(default)]
    pub predicates: Vec<AccessPredicate>,
}

impl AccessRule {
    pub fn is_unrestricted(&self) -> bool {
        !self.require_authenticated && self.predicates.is_empty()
    }

    pub fn validate_for(&self, action: AccessAction) -> Result<()> {
        for predicate in &self.predicates {
            predicate.validate_for(action)?;
        }
        Ok(())
    }

    pub fn allows(
        &self,
        principal: &PrincipalContext,
        candidate_document: Option<&Document>,
        existing_document: Option<&Document>,
    ) -> Result<bool> {
        if self.require_authenticated && !principal.authenticated {
            return Ok(false);
        }

        for predicate in &self.predicates {
            if !predicate.matches(principal, candidate_document, existing_document)? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub fn compile_read_filters(&self, principal: &PrincipalContext) -> Result<CompiledReadRule> {
        if self.require_authenticated && !principal.authenticated {
            return Ok(CompiledReadRule {
                impossible: true,
                planner_filters: Vec::new(),
            });
        }

        let mut planner_filters = Vec::new();
        for predicate in &self.predicates {
            match predicate.compile_read_filter(principal)? {
                CompiledReadPredicate::PlannerFilter(filter) => planner_filters.push(filter),
                CompiledReadPredicate::AlwaysFalse => {
                    return Ok(CompiledReadRule {
                        impossible: true,
                        planner_filters: Vec::new(),
                    });
                }
                CompiledReadPredicate::ResidualOnly => {}
            }
        }

        Ok(CompiledReadRule {
            impossible: false,
            planner_filters,
        })
    }
}

/// Read-planner view of a declarative access rule.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledReadRule {
    pub impossible: bool,
    pub planner_filters: Vec<Filter>,
}

enum CompiledReadPredicate {
    PlannerFilter(Filter),
    AlwaysFalse,
    ResidualOnly,
}

/// A single comparison predicate inside an access rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessPredicate {
    pub left: AccessValue,
    pub op: AccessOperator,
    pub right: AccessValue,
}

impl AccessPredicate {
    fn validate_for(&self, action: AccessAction) -> Result<()> {
        self.left.validate_for(action)?;
        self.right.validate_for(action)?;
        Ok(())
    }

    fn matches(
        &self,
        principal: &PrincipalContext,
        candidate_document: Option<&Document>,
        existing_document: Option<&Document>,
    ) -> Result<bool> {
        let Some(left) = self
            .left
            .resolve(principal, candidate_document, existing_document)?
        else {
            return Ok(false);
        };
        let Some(right) = self
            .right
            .resolve(principal, candidate_document, existing_document)?
        else {
            return Ok(false);
        };
        compare_access_values(&left, self.op, &right)
    }

    fn compile_read_filter(&self, principal: &PrincipalContext) -> Result<CompiledReadPredicate> {
        match (
            self.left.resolve_constant_for_read(principal)?,
            self.right.resolve_constant_for_read(principal)?,
        ) {
            (Some(left), None) if self.right.is_document_field() => {
                let field = self
                    .right
                    .document_field_name()
                    .expect("document field should carry a field name");
                Ok(CompiledReadPredicate::PlannerFilter(Filter {
                    field,
                    op: invert_filter_op(self.op),
                    value: left,
                }))
            }
            (None, Some(right)) if self.left.is_document_field() => {
                let field = self
                    .left
                    .document_field_name()
                    .expect("document field should carry a field name");
                Ok(CompiledReadPredicate::PlannerFilter(Filter {
                    field,
                    op: self.op.into(),
                    value: right,
                }))
            }
            (Some(left), Some(right)) => {
                if compare_access_values(&left, self.op, &right)? {
                    Ok(CompiledReadPredicate::ResidualOnly)
                } else {
                    Ok(CompiledReadPredicate::AlwaysFalse)
                }
            }
            (Some(_), None) | (None, Some(_))
                if self.left.depends_on_missing_principal(principal)
                    || self.right.depends_on_missing_principal(principal) =>
            {
                Ok(CompiledReadPredicate::AlwaysFalse)
            }
            _ => Ok(CompiledReadPredicate::ResidualOnly),
        }
    }
}

/// Comparison operator inside an access predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessOperator {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl From<AccessOperator> for FilterOp {
    fn from(value: AccessOperator) -> Self {
        match value {
            AccessOperator::Eq => Self::Eq,
            AccessOperator::Neq => Self::Neq,
            AccessOperator::Gt => Self::Gt,
            AccessOperator::Gte => Self::Gte,
            AccessOperator::Lt => Self::Lt,
            AccessOperator::Lte => Self::Lte,
        }
    }
}

/// Declarative reference to a principal claim or document field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum AccessValue {
    Literal {
        value: Value,
    },
    PrincipalClaim {
        principal: PrincipalClaimSource,
        claim: String,
    },
    DocumentField {
        field: String,
    },
    ExistingDocumentField {
        field: String,
    },
}

impl AccessValue {
    fn validate_for(&self, action: AccessAction) -> Result<()> {
        match self {
            Self::Literal { .. } => Ok(()),
            Self::PrincipalClaim { claim, .. } => {
                if claim.is_empty() {
                    return Err(Error::InvalidInput(
                        "principal claim names cannot be empty".to_string(),
                    ));
                }
                Ok(())
            }
            Self::DocumentField { field } => {
                if field.is_empty() {
                    return Err(Error::InvalidInput(
                        "document field names cannot be empty".to_string(),
                    ));
                }
                if matches!(action, AccessAction::Delete) {
                    return Err(Error::InvalidInput(
                        "delete access rules cannot reference the candidate document".to_string(),
                    ));
                }
                Ok(())
            }
            Self::ExistingDocumentField { field } => {
                if field.is_empty() {
                    return Err(Error::InvalidInput(
                        "existing document field names cannot be empty".to_string(),
                    ));
                }
                if matches!(action, AccessAction::Read | AccessAction::Create) {
                    return Err(Error::InvalidInput(
                        "read and create access rules cannot reference an existing document"
                            .to_string(),
                    ));
                }
                Ok(())
            }
        }
    }

    fn resolve(
        &self,
        principal: &PrincipalContext,
        candidate_document: Option<&Document>,
        existing_document: Option<&Document>,
    ) -> Result<Option<Value>> {
        match self {
            Self::Literal { value } => Ok(Some(value.clone())),
            Self::PrincipalClaim {
                principal: source,
                claim,
            } => Ok(principal.claim(*source, claim).cloned()),
            Self::DocumentField { field } => {
                Ok(candidate_document.and_then(|document| document_field_value(document, field)))
            }
            Self::ExistingDocumentField { field } => {
                Ok(existing_document.and_then(|document| document_field_value(document, field)))
            }
        }
    }

    fn resolve_constant_for_read(&self, principal: &PrincipalContext) -> Result<Option<Value>> {
        match self {
            Self::Literal { value } => Ok(Some(value.clone())),
            Self::PrincipalClaim {
                principal: source,
                claim,
            } => Ok(principal.claim(*source, claim).cloned()),
            Self::DocumentField { .. } => Ok(None),
            Self::ExistingDocumentField { .. } => Ok(None),
        }
    }

    fn is_document_field(&self) -> bool {
        matches!(self, Self::DocumentField { .. })
    }

    fn document_field_name(&self) -> Option<String> {
        match self {
            Self::DocumentField { field } => Some(field.clone()),
            _ => None,
        }
    }

    fn depends_on_missing_principal(&self, principal: &PrincipalContext) -> bool {
        match self {
            Self::PrincipalClaim {
                principal: source,
                claim,
            } => principal.claim(*source, claim).is_none(),
            _ => false,
        }
    }
}

/// Claim bag source inside a normalized principal context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalClaimSource {
    Identity,
    VerifiedIdentity,
}

/// Returns a stable revision fingerprint for a table access policy.
pub fn policy_revision_id(policy: Option<&TableAccessPolicy>) -> Result<String> {
    let bytes =
        serde_json::to_vec(&policy).map_err(|error| Error::Serialization(error.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}

fn invert_filter_op(op: AccessOperator) -> FilterOp {
    match op {
        AccessOperator::Eq => FilterOp::Eq,
        AccessOperator::Neq => FilterOp::Neq,
        AccessOperator::Gt => FilterOp::Lt,
        AccessOperator::Gte => FilterOp::Lte,
        AccessOperator::Lt => FilterOp::Gt,
        AccessOperator::Lte => FilterOp::Gte,
    }
}

fn document_field_value(document: &Document, field: &str) -> Option<Value> {
    match field {
        "_id" => Some(Value::String(document.id.to_string())),
        "_creationTime" => Some(Value::Number(document.creation_time.0.into())),
        _ => document.get_field(field).cloned(),
    }
}

fn compare_access_values(left: &Value, op: AccessOperator, right: &Value) -> Result<bool> {
    match op {
        AccessOperator::Eq => Ok(left == right),
        AccessOperator::Neq => Ok(left != right),
        AccessOperator::Gt => Ok(compare_policy_values(left, right)? == Ordering::Greater),
        AccessOperator::Gte => Ok(matches!(
            compare_policy_values(left, right)?,
            Ordering::Greater | Ordering::Equal
        )),
        AccessOperator::Lt => Ok(compare_policy_values(left, right)? == Ordering::Less),
        AccessOperator::Lte => Ok(matches!(
            compare_policy_values(left, right)?,
            Ordering::Less | Ordering::Equal
        )),
    }
}

fn compare_policy_values(left: &Value, right: &Value) -> Result<Ordering> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => {
            let left = left
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            let right = right
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            left.partial_cmp(&right).ok_or_else(|| {
                Error::InvalidInput("invalid numeric ordering comparison".to_string())
            })
        }
        _ => Err(Error::InvalidInput(
            "comparisons only support string and number values in phase 1".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::TableName;

    fn owner_policy() -> AccessRule {
        AccessRule {
            require_authenticated: true,
            predicates: vec![AccessPredicate {
                left: AccessValue::DocumentField {
                    field: "owner".to_string(),
                },
                op: AccessOperator::Eq,
                right: AccessValue::PrincipalClaim {
                    principal: PrincipalClaimSource::Identity,
                    claim: "subject".to_string(),
                },
            }],
        }
    }

    fn owner_document(owner: &str) -> Document {
        Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("owner".to_string(), json!(owner))]),
        )
    }

    fn owner_principal(owner: &str) -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: Map::from_iter([("subject".to_string(), json!(owner))]),
            verified_claims: Map::new(),
        }
    }

    #[test]
    fn principal_snapshot_is_stable() {
        let principal = owner_principal("ada");
        let left = principal
            .snapshot()
            .expect("snapshot should serialize principal");
        let right = principal
            .snapshot()
            .expect("snapshot should serialize principal");

        assert_eq!(left, right);
    }

    #[test]
    fn read_rule_compiles_principal_equality_into_filter() {
        let compiled = owner_policy()
            .compile_read_filters(&owner_principal("ada"))
            .expect("policy should compile");

        assert!(!compiled.impossible);
        assert_eq!(
            compiled.planner_filters,
            vec![Filter {
                field: "owner".to_string(),
                op: FilterOp::Eq,
                value: json!("ada"),
            }]
        );
    }

    #[test]
    fn read_rule_becomes_impossible_without_required_claim() {
        let compiled = owner_policy()
            .compile_read_filters(&PrincipalContext {
                authenticated: true,
                claims: Map::new(),
                verified_claims: Map::new(),
            })
            .expect("policy should compile");

        assert!(compiled.impossible);
    }

    #[test]
    fn access_rule_matches_candidate_document() {
        let allowed = owner_policy()
            .allows(&owner_principal("ada"), Some(&owner_document("ada")), None)
            .expect("policy evaluation should succeed");
        let denied = owner_policy()
            .allows(&owner_principal("ada"), Some(&owner_document("lin")), None)
            .expect("policy evaluation should succeed");

        assert!(allowed);
        assert!(!denied);
    }

    #[test]
    fn policy_revision_changes_when_policy_changes() {
        let empty = policy_revision_id(None).expect("empty policy should hash");
        let guarded = policy_revision_id(Some(&TableAccessPolicy {
            read: owner_policy(),
            ..TableAccessPolicy::default()
        }))
        .expect("policy should hash");

        assert_ne!(empty, guarded);
    }
}
