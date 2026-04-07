use std::sync::{Arc, Condvar, Mutex};

use neovex_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, FieldSchema, FieldType,
    IndexDefinition, PrincipalClaimSource, PrincipalContext, TableAccessPolicy, TableName,
    TableSchema,
};
use serde_json::json;
use tokio::sync::Notify;

use neovex_storage::{FaultInjector, FaultPoint};

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

pub(crate) struct BlockingFaultInjector {
    point: FaultPoint,
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

impl BlockingFaultInjector {
    pub(crate) fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    pub(crate) async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    pub(crate) fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

impl FaultInjector for BlockingFaultInjector {
    fn check(&self, point: FaultPoint) -> neovex_core::Result<()> {
        if point != self.point {
            return Ok(());
        }
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking fault injector should wait for release");
        }
        Ok(())
    }
}
