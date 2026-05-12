use std::collections::BTreeMap;

use nimbus_core::{
    FirestoreTriggerMetadata, Result, TenantId, TriggerCloudEvent, TriggerEvent,
    TriggerExecutionPrincipal, TriggerInvocationAncestry, TriggerInvocationKey,
    TriggerInvocationRecord,
};

use super::dispatch::TriggerCommitCandidate;
use super::registry::TriggerRegistry;

const DEFAULT_DATABASE_ID: &str = "(default)";
pub(crate) const DEFAULT_TRIGGER_CHAIN_DEPTH_LIMIT: u32 = 8;

pub(crate) fn build_trigger_invocation_records(
    tenant_id: &TenantId,
    registry: &TriggerRegistry,
    candidate: &TriggerCommitCandidate,
) -> Result<Vec<TriggerInvocationRecord>> {
    let matches = registry.lookup(candidate.event_type, &candidate.binding.document_path);
    let mut records = Vec::with_capacity(matches.len());
    for matched in matches {
        let params = matched
            .path_match
            .params()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let key = TriggerInvocationKey::new(
            matched.registration.id().to_string(),
            candidate.event_id.clone(),
        )?;
        let event = TriggerEvent::new(
            TriggerCloudEvent::new(
                candidate.event_id.clone(),
                format!(
                    "//firestore.googleapis.com/projects/{}/databases/{}",
                    tenant_id, DEFAULT_DATABASE_ID
                ),
                candidate.event_type,
                candidate.commit.timestamp,
                format!("documents/{}", candidate.binding.document_path),
            ),
            FirestoreTriggerMetadata::new(
                tenant_id.to_string(),
                DEFAULT_DATABASE_ID,
                candidate.binding.document_path.clone(),
                params,
            ),
            candidate.data.clone(),
            candidate.commit,
            TriggerExecutionPrincipal::service(nimbus_core::PrincipalContext::system()),
        );
        let ancestry = candidate.write_origin.as_ref().map(|origin| {
            TriggerInvocationAncestry::new(origin.invocation.clone(), origin.child_depth())
        });
        let record = if ancestry
            .as_ref()
            .is_some_and(|ancestry| ancestry.depth > DEFAULT_TRIGGER_CHAIN_DEPTH_LIMIT)
        {
            TriggerInvocationRecord::terminal_with_ancestry(
                key,
                candidate.commit.sequence,
                event,
                ancestry,
                candidate.commit.timestamp,
                format!(
                    "trigger chain depth {} exceeds configured limit {}",
                    candidate
                        .write_origin
                        .as_ref()
                        .map(|origin| origin.child_depth())
                        .unwrap_or_default(),
                    DEFAULT_TRIGGER_CHAIN_DEPTH_LIMIT
                ),
            )
        } else {
            TriggerInvocationRecord::pending_with_ancestry(
                key,
                candidate.commit.sequence,
                event,
                ancestry,
            )
        };
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use nimbus_core::{
        DocumentEventData, DocumentId, DocumentLocator, DocumentPath, FirestoreCloudEventType,
        ResourcePathBinding, SequenceNumber, TableName, Timestamp, TriggerCommitMetadata,
        TriggerWriteOrigin,
    };

    use crate::{TriggerRegistration, triggers::registry::TriggerRegistry};

    use super::*;
    use crate::triggers::dispatch::TriggerCommitCandidate;

    fn registration(
        id: &str,
        event_type: FirestoreCloudEventType,
        pattern: &[&str],
    ) -> TriggerRegistration {
        TriggerRegistration::new(
            id,
            event_type,
            nimbus_core::DocumentTriggerPattern::from_segments(pattern)
                .expect("pattern should parse"),
        )
        .expect("registration should parse")
    }

    fn candidate_with_origin(write_origin: Option<TriggerWriteOrigin>) -> TriggerCommitCandidate {
        let document_id = DocumentId::from_key("alpha").expect("document id should parse");
        let table = TableName::new("tasks").expect("table should parse");
        TriggerCommitCandidate {
            event_id: "evt-1".to_string(),
            event_type: FirestoreCloudEventType::Written,
            binding: ResourcePathBinding::new(
                DocumentLocator::new(table, document_id),
                DocumentPath::from_segments(["tasks", "alpha"]).expect("path should parse"),
            ),
            commit: TriggerCommitMetadata::new(SequenceNumber(7), Timestamp(70)),
            data: DocumentEventData::new(None, None, None),
            write_origin,
        }
    }

    #[test]
    fn root_candidates_materialize_without_ancestry() {
        let tenant_id = TenantId::new("demo").expect("tenant id should parse");
        let registry = TriggerRegistry::new();
        registry
            .replace(vec![registration(
                "firebase:tasksWritten",
                FirestoreCloudEventType::Written,
                &["tasks", "{taskId}"],
            )])
            .expect("registry should accept registration");

        let records =
            build_trigger_invocation_records(&tenant_id, &registry, &candidate_with_origin(None))
                .expect("records should materialize");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].ancestry, None);
        assert_eq!(records[0].depth(), 0);
    }

    #[test]
    fn nested_candidates_materialize_child_ancestry() {
        let tenant_id = TenantId::new("demo").expect("tenant id should parse");
        let registry = TriggerRegistry::new();
        registry
            .replace(vec![registration(
                "firebase:tasksWritten",
                FirestoreCloudEventType::Written,
                &["tasks", "{taskId}"],
            )])
            .expect("registry should accept registration");
        let origin = TriggerWriteOrigin::new(
            TriggerInvocationKey::new("firebase:parent", "evt-parent")
                .expect("invocation key should parse"),
            2,
        );

        let records = build_trigger_invocation_records(
            &tenant_id,
            &registry,
            &candidate_with_origin(Some(origin)),
        )
        .expect("records should materialize");

        assert_eq!(records.len(), 1);
        let ancestry = records[0]
            .ancestry
            .as_ref()
            .expect("child invocation should store ancestry");
        assert_eq!(ancestry.parent.registration_id, "firebase:parent");
        assert_eq!(ancestry.depth, 3);
    }

    #[test]
    fn over_depth_candidates_materialize_terminal_failure_without_running() {
        let tenant_id = TenantId::new("demo").expect("tenant id should parse");
        let registry = TriggerRegistry::new();
        registry
            .replace(vec![registration(
                "firebase:tasksWritten",
                FirestoreCloudEventType::Written,
                &["tasks", "{taskId}"],
            )])
            .expect("registry should accept registration");
        let origin = TriggerWriteOrigin::new(
            TriggerInvocationKey::new("firebase:parent", "evt-parent")
                .expect("invocation key should parse"),
            DEFAULT_TRIGGER_CHAIN_DEPTH_LIMIT,
        );

        let records = build_trigger_invocation_records(
            &tenant_id,
            &registry,
            &candidate_with_origin(Some(origin)),
        )
        .expect("records should materialize");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].depth(), DEFAULT_TRIGGER_CHAIN_DEPTH_LIMIT + 1);
        assert!(matches!(
            records[0].state,
            nimbus_core::TriggerInvocationState::TerminalFailure { attempt: 0, .. }
        ));
    }
}
