use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PrincipalClaimSource, PrincipalContext};
use crate::{Document, Error, Filter, FilterOp, Result};

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
        let left = self.left.resolve_constant_for_read(principal)?;
        let right = self.right.resolve_constant_for_read(principal)?;
        let missing_principal = self.left.depends_on_missing_principal(principal)
            || self.right.depends_on_missing_principal(principal);

        match (left, right) {
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
            _ if missing_principal => Ok(CompiledReadPredicate::AlwaysFalse),
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
        "_updateTime" => Some(Value::Number(document.update_time.0.into())),
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
