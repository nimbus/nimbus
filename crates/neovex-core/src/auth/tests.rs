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
