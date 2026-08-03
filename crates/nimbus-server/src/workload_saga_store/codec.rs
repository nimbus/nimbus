use std::collections::BTreeSet;

use nimbus_core::{Document, DocumentId};
use nimbus_workloads::{WorkloadSagaRecord, WorkloadSagaStoreError};
use serde_json::{Map, Value, json};

const REQUIRED_FIELDS: [&str; 19] = [
    "formatVersion",
    "sagaId",
    "tenantId",
    "workloadId",
    "workloadKind",
    "desiredState",
    "desiredGeneration",
    "desiredDigest",
    "executable",
    "source",
    "sagaRevision",
    "phase",
    "recoveryEligible",
    "phaseDetail",
    "compiledNetworkPlan",
    "activationIntent",
    "publicationIntent",
    "admission",
    "lastTransition",
];
const OPTIONAL_FIELDS: [&str; 3] = ["successorIntent", "provisionDisposition", "failure"];

pub(crate) fn encode_workload_saga_record(
    record: &WorkloadSagaRecord,
) -> Result<Map<String, Value>, WorkloadSagaStoreError> {
    record.validate()?;
    let portable = serde_json::to_value(record).map_err(|_| WorkloadSagaStoreError::Corrupt)?;
    let portable = portable
        .as_object()
        .ok_or(WorkloadSagaStoreError::Corrupt)?;
    let key = object(portable, "key")?;
    let active = object(portable, "activeIntent")?;
    let network = object(active, "network")?;

    let mut fields = Map::new();
    copy(&mut fields, "formatVersion", portable, "formatVersion")?;
    copy(&mut fields, "sagaId", portable, "sagaId")?;
    copy(&mut fields, "tenantId", key, "tenantId")?;
    copy(&mut fields, "workloadId", key, "workloadId")?;
    copy(&mut fields, "workloadKind", active, "kind")?;
    copy(&mut fields, "desiredState", active, "desiredState")?;
    copy(&mut fields, "desiredGeneration", active, "generation")?;
    copy(&mut fields, "desiredDigest", active, "desiredDigest")?;
    copy(&mut fields, "executable", active, "executable")?;
    copy(&mut fields, "source", active, "source")?;
    copy(&mut fields, "sagaRevision", portable, "revision")?;
    copy(&mut fields, "phase", portable, "phase")?;
    fields.insert(
        "recoveryEligible".to_owned(),
        Value::Bool(record.requires_recovery()),
    );
    copy(&mut fields, "phaseDetail", portable, "phaseDetail")?;
    copy_optional(
        &mut fields,
        "provisionDisposition",
        portable,
        "provisionDisposition",
    )?;
    fields.insert(
        "compiledNetworkPlan".to_owned(),
        Value::Object(network.clone()),
    );
    copy(&mut fields, "activationIntent", active, "activation")?;
    copy(&mut fields, "publicationIntent", active, "publication")?;
    copy(&mut fields, "admission", active, "admission")?;
    copy(&mut fields, "lastTransition", portable, "lastTransition")?;
    copy_optional(&mut fields, "successorIntent", portable, "successorIntent")?;
    copy_optional(&mut fields, "failure", portable, "failure")?;
    Ok(fields)
}

pub(crate) fn decode_workload_saga_record(
    document: &Document,
) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
    validate_physical_shape(&document.fields)?;

    let fields = &document.fields;
    let portable = json!({
        "formatVersion": required(fields, "formatVersion")?,
        "sagaId": required(fields, "sagaId")?,
        "key": {
            "tenantId": required(fields, "tenantId")?,
            "workloadId": required(fields, "workloadId")?,
        },
        "activeIntent": {
            "kind": required(fields, "workloadKind")?,
            "desiredState": required(fields, "desiredState")?,
            "generation": required(fields, "desiredGeneration")?,
            "desiredDigest": required(fields, "desiredDigest")?,
            "executable": required(fields, "executable")?,
            "source": required(fields, "source")?,
            "network": required(fields, "compiledNetworkPlan")?,
            "activation": required(fields, "activationIntent")?,
            "publication": required(fields, "publicationIntent")?,
            "admission": required(fields, "admission")?,
        },
        "successorIntent": fields.get("successorIntent").cloned(),
        "revision": required(fields, "sagaRevision")?,
        "phase": required(fields, "phase")?,
        "phaseDetail": required(fields, "phaseDetail")?,
        "provisionDisposition": fields.get("provisionDisposition").cloned(),
        "lastTransition": required(fields, "lastTransition")?,
        "failure": fields.get("failure").cloned(),
    });
    let record: WorkloadSagaRecord =
        serde_json::from_value(portable).map_err(|_| WorkloadSagaStoreError::Corrupt)?;

    let expected_document_id = DocumentId::from_key(record.saga_id().as_str())
        .map_err(|_| WorkloadSagaStoreError::Corrupt)?;
    let recovery_eligible = fields
        .get("recoveryEligible")
        .and_then(Value::as_bool)
        .ok_or(WorkloadSagaStoreError::Corrupt)?;
    if document.id != expected_document_id || recovery_eligible != record.requires_recovery() {
        return Err(WorkloadSagaStoreError::Corrupt);
    }
    Ok(record)
}

fn validate_physical_shape(fields: &Map<String, Value>) -> Result<(), WorkloadSagaStoreError> {
    let allowed: BTreeSet<_> = REQUIRED_FIELDS
        .iter()
        .chain(OPTIONAL_FIELDS.iter())
        .copied()
        .collect();
    if fields.keys().any(|field| !allowed.contains(field.as_str()))
        || REQUIRED_FIELDS
            .iter()
            .any(|field| !fields.contains_key(*field))
    {
        return Err(WorkloadSagaStoreError::Corrupt);
    }
    for optional in OPTIONAL_FIELDS {
        if fields.get(optional).is_some_and(Value::is_null) {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
    }
    Ok(())
}

fn object<'a>(
    fields: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Map<String, Value>, WorkloadSagaStoreError> {
    fields
        .get(name)
        .and_then(Value::as_object)
        .ok_or(WorkloadSagaStoreError::Corrupt)
}

fn required(fields: &Map<String, Value>, name: &str) -> Result<Value, WorkloadSagaStoreError> {
    fields
        .get(name)
        .cloned()
        .ok_or(WorkloadSagaStoreError::Corrupt)
}

fn copy(
    target: &mut Map<String, Value>,
    target_name: &str,
    source: &Map<String, Value>,
    source_name: &str,
) -> Result<(), WorkloadSagaStoreError> {
    target.insert(target_name.to_owned(), required(source, source_name)?);
    Ok(())
}

fn copy_optional(
    target: &mut Map<String, Value>,
    target_name: &str,
    source: &Map<String, Value>,
    source_name: &str,
) -> Result<(), WorkloadSagaStoreError> {
    match source.get(source_name) {
        Some(Value::Null) | None => Ok(()),
        Some(value) => {
            target.insert(target_name.to_owned(), value.clone());
            Ok(())
        }
    }
}
