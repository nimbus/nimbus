use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_core::SequenceNumber;
use nimbus_engine::{
    BootstrapFingerprint, ConsistencyEscalationReason, ConsistencyMismatch, ConsistencyScope,
    ConsistencyVerificationMode, ConsistencyVerificationReport, SnapshotFingerprint,
    VerificationAnchor, VerificationRootFingerprint,
};
use nimbus_storage::{
    MATERIALIZED_POSITION_VERSION, MaterializedPosition, MaterializedVerificationMetricsSnapshot,
};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};

use super::trace_consistency_report;

#[derive(Clone, Debug, Default)]
struct CapturingSubscriber {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

#[derive(Clone, Debug)]
struct CapturedEvent {
    level: Level,
    fields: BTreeMap<String, String>,
}

impl Subscriber for CapturingSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = EventFieldVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("event buffer should be available")
            .push(CapturedEvent {
                level: *event.metadata().level(),
                fields: visitor.fields,
            });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        Interest::always()
    }
}

#[derive(Debug, Default)]
struct EventFieldVisitor {
    fields: BTreeMap<String, String>,
}

impl Visit for EventFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

#[test]
fn consistency_reports_emit_structured_info_and_warning_events() {
    let subscriber = CapturingSubscriber::default();
    let events = subscriber.events.clone();
    tracing::dispatcher::with_default(&tracing::Dispatch::new(subscriber), || {
        let healthy = consistency_report(Vec::new());
        trace_consistency_report(&healthy, false);

        let mismatch = ConsistencyMismatch {
            invariant: "materialized_snapshot_match".to_string(),
            left_scope: ConsistencyScope::AuthoritativeSnapshot,
            right_scope: ConsistencyScope::ShadowMaterializer,
            path: "position".to_string(),
            left_description: "authoritative".to_string(),
            right_description: "shadow".to_string(),
        };
        let divergent = consistency_report(vec![mismatch]);
        trace_consistency_report(&divergent, true);
    });

    let events = events.lock().expect("event buffer should be available");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].level, Level::INFO);
    assert_eq!(events[1].level, Level::WARN);
    assert_eq!(
        events[0].fields.get("action").map(String::as_str),
        Some("\"tenant_consistency_verification\"")
    );
    assert_eq!(
        events[0].fields.get("mismatch_count").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        events[1]
            .fields
            .get("first_mismatch_invariant")
            .map(String::as_str),
        Some("\"materialized_snapshot_match\"")
    );
    assert_eq!(
        events[1].fields.get("force_full").map(String::as_str),
        Some("true")
    );
}

fn consistency_report(mismatches: Vec<ConsistencyMismatch>) -> ConsistencyVerificationReport {
    let position = MaterializedPosition::new(SequenceNumber(3), "0".repeat(64))
        .expect("test position should be valid");
    let fingerprint = SnapshotFingerprint {
        position: position.clone(),
        snapshot_version: 1,
        durable_head: 3,
        schema_table_count: 1,
        document_count: 3,
        scheduled_execution_count: 0,
    };
    let root = VerificationRootFingerprint {
        version: MATERIALIZED_POSITION_VERSION,
        applied_sequence: 3,
        root_hash: "0".repeat(64),
        leaf_count: 3,
        resident_bytes: 128,
    };
    ConsistencyVerificationReport {
        tenant_id: "demo".to_string(),
        ok: mismatches.is_empty(),
        mode: ConsistencyVerificationMode::FullScrub,
        anchor: VerificationAnchor {
            position: position.clone(),
            age_millis: 1,
        },
        event_count: 3,
        escalation_reason: Some(ConsistencyEscalationReason::ColdStart),
        authoritative_root: root.clone(),
        shadow_root: root.clone(),
        embedded_replica_root: root,
        authoritative: fingerprint.clone(),
        shadow: fingerprint.clone(),
        embedded_replica: fingerprint,
        bootstrap: BootstrapFingerprint {
            snapshot_position: position,
            resume_after_sequence: 3,
            bootstrap_cut_sequence: 3,
            cursor_floor_sequence: 1,
        },
        mismatches,
        metrics: MaterializedVerificationMetricsSnapshot::default(),
    }
}
