use std::num::NonZeroU64;

use nimbus_core::TenantId;

use crate::{
    TenantRetirementCursor, TenantRetirementError, TenantRetirementId, TenantRetirementPage,
    TenantRetirementPageRequest, TenantRetirementPhase, TenantRetirementRecord,
    TenantRetirementRevision, TenantRetirementSource, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion,
};

fn tenant(label: &str) -> TenantId {
    TenantId::new(format!("tenant-{label}")).expect("fixture tenant should validate")
}

fn source(label: &str, observed: bool) -> TenantRetirementSource {
    TenantRetirementSource::new(
        WorkloadProvisionSourceIdentity::standalone_sandbox(label, "standard")
            .expect("fixture source should validate"),
        WorkloadProvisionSourceGeneration::new(3),
        WorkloadProvisionSourceResourceVersion::new(format!("version-{label}"))
            .expect("fixture version should validate"),
        observed,
    )
}

fn record(label: &str, incarnation: u64) -> TenantRetirementRecord {
    TenantRetirementRecord::new(
        tenant(label),
        NonZeroU64::new(incarnation).expect("fixture incarnation is nonzero"),
        vec![source(label, true)],
    )
    .expect("fixture retirement should validate")
}

#[test]
fn identity_is_stable_incarnation_scoped_and_strict() {
    let first = TenantRetirementId::for_incarnation(&tenant("a"), NonZeroU64::new(1).unwrap());
    let replay = TenantRetirementId::for_incarnation(&tenant("a"), NonZeroU64::new(1).unwrap());
    let next = TenantRetirementId::for_incarnation(&tenant("a"), NonZeroU64::new(2).unwrap());
    assert_eq!(first, replay);
    assert_ne!(first, next);
    assert_eq!(
        first.as_str().parse::<TenantRetirementId>(),
        Ok(first.clone())
    );
    assert!("trt_deadbeef".parse::<TenantRetirementId>().is_err());
    assert!(
        first
            .as_str()
            .replacen("trt_", "wsg_", 1)
            .parse::<TenantRetirementId>()
            .is_err()
    );
}

#[test]
fn record_canonicalizes_sources_and_advances_exactly_one_phase() {
    let mut initial = TenantRetirementRecord::new(
        tenant("ordered"),
        NonZeroU64::new(4).unwrap(),
        vec![source("z", false), source("a", true)],
    )
    .unwrap();
    assert_eq!(initial.revision(), TenantRetirementRevision::new(0));
    assert_eq!(initial.sources()[0].identity().stable_name(), "a");
    for phase in [
        TenantRetirementPhase::ChildrenRecorded,
        TenantRetirementPhase::SourcesFinalized,
        TenantRetirementPhase::EngineDeleted,
        TenantRetirementPhase::Recorded,
    ] {
        initial = initial.advance(phase).unwrap();
    }
    assert!(initial.phase().is_terminal());
    assert_eq!(initial.revision(), TenantRetirementRevision::new(4));
    assert!(initial.advance(TenantRetirementPhase::Recorded).is_err());
}

#[test]
fn record_rejects_duplicate_workload_names_and_crossed_identity() {
    let duplicate = TenantRetirementRecord::new(
        tenant("duplicate"),
        NonZeroU64::new(1).unwrap(),
        vec![
            TenantRetirementSource::new(
                WorkloadProvisionSourceIdentity::standalone_sandbox("same", "one").unwrap(),
                WorkloadProvisionSourceGeneration::new(1),
                WorkloadProvisionSourceResourceVersion::new("one").unwrap(),
                false,
            ),
            TenantRetirementSource::new(
                WorkloadProvisionSourceIdentity::sandbox_backed_service("same").unwrap(),
                WorkloadProvisionSourceGeneration::new(1),
                WorkloadProvisionSourceResourceVersion::new("two").unwrap(),
                false,
            ),
        ],
    );
    assert!(matches!(
        duplicate,
        Err(TenantRetirementError::InvalidRecord(_))
    ));

    let original = record("crossed", 1);
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["tenantIncarnation"] = serde_json::json!("2");
    assert!(
        serde_json::from_value::<TenantRetirementRecord>(wire)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn wire_requires_canonical_decimal_revision_and_denies_unknown_fields() {
    let original = record("wire", 1);
    let mut wire = serde_json::to_value(&original).unwrap();
    assert_eq!(wire["revision"], "0");
    assert_eq!(wire["tenantIncarnation"], "1");
    wire["revision"] = serde_json::json!("00");
    assert!(serde_json::from_value::<TenantRetirementRecord>(wire).is_err());

    let mut wire = serde_json::to_value(&original).unwrap();
    wire["tenantIncarnation"] = serde_json::json!(1);
    assert!(serde_json::from_value::<TenantRetirementRecord>(wire).is_err());

    let mut wire = serde_json::to_value(&original).unwrap();
    wire["tenantIncarnation"] = serde_json::json!("01");
    assert!(serde_json::from_value::<TenantRetirementRecord>(wire).is_err());

    let mut wire = serde_json::to_value(&original).unwrap();
    wire["phase"] = serde_json::json!("recorded");
    assert!(
        serde_json::from_value::<TenantRetirementRecord>(wire)
            .unwrap()
            .validate()
            .is_err()
    );

    let mut wire = serde_json::to_value(&original).unwrap();
    wire["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<TenantRetirementRecord>(wire).is_err());
}

#[test]
fn active_page_is_bounded_ordered_and_excludes_terminal_records() {
    assert!(TenantRetirementPageRequest::new(None, 0).is_err());
    assert!(TenantRetirementPageRequest::new(None, 257).is_err());

    let mut records = vec![record("a", 1), record("b", 1)];
    records.sort_by(|left, right| left.retirement_id().cmp(right.retirement_id()));
    let request = TenantRetirementPageRequest::new(None, 2).unwrap();
    let first = TenantRetirementPage::active(&request, records.clone(), true).unwrap();
    let cursor = first.next_cursor().cloned().unwrap();
    assert_eq!(
        cursor,
        TenantRetirementCursor::for_record(records.last().unwrap()).unwrap()
    );

    let regressing = TenantRetirementPageRequest::new(Some(cursor), 2).unwrap();
    assert!(TenantRetirementPage::active(&regressing, records, false).is_err());

    let terminal = record("terminal", 1)
        .advance(TenantRetirementPhase::ChildrenRecorded)
        .unwrap()
        .advance(TenantRetirementPhase::SourcesFinalized)
        .unwrap()
        .advance(TenantRetirementPhase::EngineDeleted)
        .unwrap()
        .advance(TenantRetirementPhase::Recorded)
        .unwrap();
    assert!(TenantRetirementPage::active(&request, vec![terminal.clone()], false).is_err());
    assert!(TenantRetirementPage::retained(&request, vec![terminal], false).is_ok());
}
