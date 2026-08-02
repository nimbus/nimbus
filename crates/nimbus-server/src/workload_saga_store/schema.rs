use std::sync::Arc;

use nimbus_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, Error, FieldSchema, FieldType,
    IndexDefinition, PrincipalClaimSource, TableAccessPolicy, TableName, TableSchema, TenantId,
};
use nimbus_engine::Engine;
use nimbus_workloads::WorkloadSagaStoreError;
use serde_json::Value;

pub(crate) const WORKLOAD_SAGA_TABLE: &str = "_workload_sagas";
pub(crate) const WORKLOAD_SAGA_TENANT: &str = "_nimbus";

pub(crate) async fn prepare_exact_schema(
    engine: &Arc<Engine>,
) -> Result<(), WorkloadSagaStoreError> {
    let tenant = workload_saga_tenant()?;
    let table = workload_saga_table()?;
    engine
        .ensure_tenant_ready_async(tenant.clone())
        .await
        .map_err(unavailable)?;

    match engine
        .get_table_schema_async(tenant.clone(), table.clone())
        .await
    {
        Ok(existing) => verify_exact_schema(&existing),
        Err(Error::SchemaNotFound(_)) => {
            engine
                .set_table_schema_async(tenant.clone(), exact_table_schema())
                .await
                .map_err(unavailable)?;
            let installed = engine
                .get_table_schema_async(tenant, table)
                .await
                .map_err(unavailable)?;
            verify_exact_schema(&installed)
        }
        Err(error) => Err(unavailable(error)),
    }
}

pub(crate) fn workload_saga_tenant() -> Result<TenantId, WorkloadSagaStoreError> {
    TenantId::new(WORKLOAD_SAGA_TENANT).map_err(unavailable)
}

pub(crate) fn workload_saga_table() -> Result<TableName, WorkloadSagaStoreError> {
    TableName::new(WORKLOAD_SAGA_TABLE).map_err(unavailable)
}

pub(crate) fn exact_table_schema() -> TableSchema {
    TableSchema {
        table: TableName::new(WORKLOAD_SAGA_TABLE)
            .expect("private workload-saga table name should be valid"),
        fields: vec![
            field("formatVersion", FieldType::Number, true),
            field("sagaId", FieldType::String, true),
            field("tenantId", FieldType::String, true),
            field("workloadId", FieldType::String, true),
            field("workloadKind", FieldType::String, true),
            field("desiredState", FieldType::String, true),
            field("desiredGeneration", FieldType::String, true),
            field("desiredDigest", FieldType::String, true),
            field("executable", FieldType::Object, true),
            field("sagaRevision", FieldType::String, true),
            field("phase", FieldType::String, true),
            field("recoveryEligible", FieldType::Boolean, true),
            field("phaseDetail", FieldType::Object, true),
            field("compiledNetworkPlan", FieldType::Object, true),
            field("activationIntent", FieldType::String, true),
            field("publicationIntent", FieldType::String, true),
            field("admission", FieldType::Object, true),
            field("successorIntent", FieldType::Object, false),
            field("lastTransition", FieldType::Object, true),
            field("failure", FieldType::Object, false),
        ],
        indexes: vec![
            IndexDefinition::new("by_tenantId_and_workloadId", ["tenantId", "workloadId"]),
            IndexDefinition::new("by_recovery", ["recoveryEligible", "sagaId"]),
            IndexDefinition::new("by_tenantId_and_phase", ["tenantId", "phase"]),
            IndexDefinition::new("by_desiredState_and_phase", ["desiredState", "phase"]),
        ],
        access_policy: Some(system_access_policy()),
    }
}

fn verify_exact_schema(existing: &TableSchema) -> Result<(), WorkloadSagaStoreError> {
    let mut expected = exact_table_schema();
    expected.reconcile_index_metadata(Some(existing));
    if &expected == existing {
        Ok(())
    } else {
        Err(WorkloadSagaStoreError::Corrupt)
    }
}

fn field(name: &str, field_type: FieldType, required: bool) -> FieldSchema {
    FieldSchema {
        name: name.to_owned(),
        field_type,
        required,
    }
}

fn system_access_policy() -> TableAccessPolicy {
    let rule = AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left: AccessValue::PrincipalClaim {
                principal: PrincipalClaimSource::Identity,
                claim: "sub".to_owned(),
            },
            op: AccessOperator::Eq,
            right: AccessValue::Literal {
                value: Value::String("system".to_owned()),
            },
        }],
    };
    TableAccessPolicy {
        read: rule.clone(),
        create: rule.clone(),
        update: rule.clone(),
        delete: rule,
    }
}

fn unavailable(_error: Error) -> WorkloadSagaStoreError {
    WorkloadSagaStoreError::Unavailable
}
