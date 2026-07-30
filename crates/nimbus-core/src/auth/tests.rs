use serde_json::{Map, json};

use super::*;
use crate::{Document, Filter, FilterOp, TableName};

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

/// A rule naming document lifecycle metadata must stay residual rather than
/// compile to a planner filter.
///
/// A planner filter resolves its field from the stored field map, where
/// `_creationTime` never appears — pushing one down would match no row at all,
/// turning a rule every real document satisfies into a rule that denies
/// everything. Residual evaluation resolves it from the document header, which
/// is the value a rule-level comparison sees.
#[test]
fn read_rule_keeps_lifecycle_metadata_predicates_residual() {
    let rule = AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left: AccessValue::DocumentField {
                field: "_creationTime".to_string(),
            },
            op: AccessOperator::Gt,
            right: AccessValue::Literal { value: json!(0) },
        }],
    };

    let compiled = rule
        .compile_read_filters(&owner_principal("ada"))
        .expect("policy should compile");
    assert!(!compiled.impossible);
    assert!(
        compiled.planner_filters.is_empty(),
        "a lifecycle-metadata predicate pushed down as a planner filter would match no stored \
         field and silently deny every row: {:?}",
        compiled.planner_filters
    );

    let mut document = owner_document("ada");
    document.creation_time = crate::Timestamp(7);
    assert!(
        rule.allows(&owner_principal("ada"), Some(&document), None)
            .expect("policy evaluation should succeed"),
        "residual evaluation must read _creationTime from the document header"
    );

    document.creation_time = crate::Timestamp(0);
    assert!(
        !rule
            .allows(&owner_principal("ada"), Some(&document), None)
            .expect("policy evaluation should succeed"),
        "the predicate must still discriminate: a zero creation time fails `> 0`"
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
