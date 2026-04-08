use neovex_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, FieldSchema, FieldType,
    IndexDefinition, PrincipalClaimSource, PrincipalContext, TableAccessPolicy, TableName,
    TableSchema,
};
use serde_json::json;
pub(crate) fn messages_table(name: &str) -> TableName {
    TableName::new(name).expect("table name should be valid")
}

pub(crate) fn principal_with_subject(subject: &str) -> PrincipalContext {
    PrincipalContext {
        authenticated: true,
        claims: serde_json::Map::from_iter([("subject".to_string(), json!(subject))]),
        verified_claims: serde_json::Map::new(),
    }
}

pub(crate) fn owner_matches_subject_rule(left: AccessValue) -> AccessRule {
    AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left,
            op: AccessOperator::Eq,
            right: AccessValue::PrincipalClaim {
                principal: PrincipalClaimSource::Identity,
                claim: "subject".to_string(),
            },
        }],
    }
}

pub(crate) fn read_only_owner_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        read: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        ..TableAccessPolicy::default()
    }
}

pub(crate) fn owner_write_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        create: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        update: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
        delete: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
        ..TableAccessPolicy::default()
    }
}

pub(crate) fn owner_read_write_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        read: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        create: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        update: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
        delete: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
    }
}

pub(crate) fn messages_schema(
    table: &str,
    indexes: Vec<IndexDefinition>,
    access_policy: Option<TableAccessPolicy>,
) -> TableSchema {
    TableSchema {
        table: messages_table(table),
        fields: vec![
            FieldSchema {
                name: "owner".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "body".to_string(),
                field_type: FieldType::String,
                required: true,
            },
        ],
        indexes,
        access_policy,
    }
}
