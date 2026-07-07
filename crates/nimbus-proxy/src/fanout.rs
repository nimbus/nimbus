//! EE4: egress decision-event fan-out seam + per-tenant decision counters.
//!
//! The PEP's best-effort terminal telemetry sink is the `Arc<DecisionLogger>`
//! closure. This module adds a composable multi-sink fan-out over it so
//! node-wide consumers — the tenant-admission-audit plan's hash-chained OCSF
//! collector (TAA2), metrics, per-tenant counters — can subscribe to terminal
//! events without participating in the durable-before-response audit commit.
//!
//! Per-tenant counters live on [`TenantFairness`] (the same node-wide,
//! registration-keyed home as the EE3 budgets): node-wide per-tenant keying is
//! derived from the tenant id the sandbox layer passes at PEP registration —
//! captured once into the counter sink, never looked up per event.
//!
//! OCSF spool content and schema stay TAA's; this module owns only the seam.
//!
//! ## Subscriber contract: sinks MUST be non-blocking
//!
//! Sinks run synchronously, inline, on the request task (including terminal
//! deny paths). A slow subscriber adds its latency directly to request
//! handling. A
//! subscriber with untrusted latency (network, disk spool, OCSF collector)
//! must decouple itself behind a bounded channel + drop counter and hand this
//! seam only the cheap enqueue.

use std::sync::Arc;

use crate::decision_log::{DecisionLogger, EgressDecisionLog};
use crate::fairness::TenantFairness;

/// Compose decision-log sinks: every event is delivered to every sink, in
/// order. These sinks are best-effort telemetry only; durable audit commits use
/// the separate fallible sink on `WorkloadPepConfig`.
pub fn fan_out_decision_loggers(sinks: Vec<DecisionLogger>) -> DecisionLogger {
    Arc::new(move |log: EgressDecisionLog| {
        let Some((last, rest)) = sinks.split_last() else {
            return;
        };
        for sink in rest {
            sink(log.clone());
        }
        last(log);
    })
}

/// A decision sink that counts allowed/denied events on the owning tenant's
/// fairness handle (captured here at construction — registration time).
pub fn tenant_decision_counter_sink(fairness: Arc<TenantFairness>) -> DecisionLogger {
    Arc::new(move |log: EgressDecisionLog| {
        if log.is_allowed() {
            fairness.record_decision_allowed();
        } else {
            fairness.record_decision_denied();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(raw: &str) -> TenantId {
        TenantId::new(raw).expect("test tenant id")
    }
    use crate::fairness::FairnessRegistry;
    use nimbus_core::TenantId;
    use std::sync::Mutex;

    fn allowed_log() -> EgressDecisionLog {
        EgressDecisionLog::synthetic_for_test(true)
    }

    fn denied_log() -> EgressDecisionLog {
        EgressDecisionLog::synthetic_for_test(false)
    }

    #[test]
    fn fan_out_delivers_every_event_to_every_sink_in_order() {
        let seen: Arc<Mutex<Vec<(&'static str, bool)>>> = Arc::new(Mutex::new(Vec::new()));
        let a = {
            let seen = Arc::clone(&seen);
            Arc::new(move |log: EgressDecisionLog| {
                seen.lock().unwrap().push(("a", log.is_allowed()));
            }) as DecisionLogger
        };
        let b = {
            let seen = Arc::clone(&seen);
            Arc::new(move |log: EgressDecisionLog| {
                seen.lock().unwrap().push(("b", log.is_allowed()));
            }) as DecisionLogger
        };

        let fanned = fan_out_decision_loggers(vec![a, b]);
        fanned(allowed_log());
        fanned(denied_log());

        assert_eq!(
            *seen.lock().unwrap(),
            vec![("a", true), ("b", true), ("a", false), ("b", false)],
            "every sink sees every event, in registration order — the first \
             (baseline) sink is never bypassed"
        );
    }

    #[test]
    fn empty_fan_out_is_a_no_op() {
        let fanned = fan_out_decision_loggers(Vec::new());
        fanned(allowed_log());
    }

    #[test]
    fn tenant_counters_attribute_decisions_per_tenant() {
        let registry = FairnessRegistry::new();
        let a = registry.tenant(&tid("tenant-a"));
        let b = registry.tenant(&tid("tenant-b"));

        let sink_a = tenant_decision_counter_sink(Arc::clone(&a));
        sink_a(allowed_log());
        sink_a(allowed_log());
        sink_a(denied_log());

        assert_eq!(a.decisions_allowed(), 2);
        assert_eq!(a.decisions_denied(), 1);
        assert_eq!(
            (b.decisions_allowed(), b.decisions_denied()),
            (0, 0),
            "one tenant's decisions must never count on another's meters"
        );
    }
}
