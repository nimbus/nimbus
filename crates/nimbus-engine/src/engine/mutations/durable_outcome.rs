use nimbus_core::{Error, SequenceNumber};

use crate::tenant::TenantRuntime;

/// The only two safe interpretations of a durable persistence error.
///
/// Callers retain ownership of route-specific rollback and shutdown work. This
/// seam owns the evidence rule deciding whether rollback is safe at all.
#[derive(Debug)]
pub(crate) enum DurableWriteOutcome {
    Definitive(Error),
    Ambiguous(Error),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum DurableWriteRoute {
    Direct,
    ExecutionUnit,
    SchemaSet,
    SchemaDelete,
}

impl DurableWriteRoute {
    fn description(self) -> &'static str {
        match self {
            Self::Direct => "direct mutation",
            Self::ExecutionUnit => "mutation execution unit",
            Self::SchemaSet => "schema set",
            Self::SchemaDelete => "schema delete",
        }
    }
}

/// Classifies a failed durable write using durable evidence, never the error's
/// apparent transport or storage class.
///
/// A committer fence is the sole exception: the lease CAS is part of the same
/// transaction and proves rollback, so probing would add ambiguity rather than
/// resolve it. Every other error is definitive only when the durable head can
/// be read and is exactly the pre-write head.
pub(crate) fn classify_durable_write_error(
    runtime: &TenantRuntime,
    route: DurableWriteRoute,
    previous_durable_head: SequenceNumber,
    write_error: Error,
) -> DurableWriteOutcome {
    if matches!(write_error, Error::CommitterFenced { .. }) {
        return DurableWriteOutcome::Definitive(write_error);
    }

    let progress = journal_progress_for_classification(runtime, route);
    match progress {
        Ok(progress) if progress.durable_head == previous_durable_head => {
            DurableWriteOutcome::Definitive(write_error)
        }
        Ok(progress) => DurableWriteOutcome::Ambiguous(Error::Internal(format!(
            "{} outcome requires crash-and-replay: durable head changed from {previous_durable_head} to {} after persistence failed ({write_error})",
            route.description(),
            progress.durable_head,
        ))),
        Err(progress_error) => DurableWriteOutcome::Ambiguous(Error::Internal(format!(
            "{} outcome is ambiguous; crash-and-replay required: persistence failed ({write_error}) and durable progress could not be read ({progress_error})",
            route.description(),
        ))),
    }
}

fn journal_progress_for_classification(
    runtime: &TenantRuntime,
    route: DurableWriteRoute,
) -> nimbus_core::Result<nimbus_storage::JournalProgress> {
    #[cfg(test)]
    {
        let key = (runtime.tenant_id().clone(), route);
        let mut state = test_state()
            .lock()
            .expect("durable outcome classifier test-state lock should not be poisoned");
        *state.probes.entry(key.clone()).or_default() += 1;
        if state.unreadable.remove(&key) {
            return Err(Error::Internal(format!(
                "injected {} durable-progress read failure",
                route.description()
            )));
        }
    }
    #[cfg(not(test))]
    let _ = route;
    runtime.store().journal_progress()
}

#[cfg(test)]
#[derive(Default)]
struct ClassifierTestState {
    unreadable: std::collections::HashSet<(nimbus_core::TenantId, DurableWriteRoute)>,
    probes: std::collections::HashMap<(nimbus_core::TenantId, DurableWriteRoute), usize>,
}

#[cfg(test)]
fn test_state() -> &'static std::sync::Mutex<ClassifierTestState> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<ClassifierTestState>> =
        std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(ClassifierTestState::default()))
}

#[cfg(test)]
impl crate::Engine {
    pub(crate) fn fail_durable_outcome_progress_for_testing(
        &self,
        tenant_id: nimbus_core::TenantId,
        route: DurableWriteRoute,
    ) {
        test_state()
            .lock()
            .expect("durable outcome classifier test-state lock should not be poisoned")
            .unreadable
            .insert((tenant_id, route));
    }

    pub(crate) fn durable_outcome_probe_count_for_testing(
        &self,
        tenant_id: &nimbus_core::TenantId,
        route: DurableWriteRoute,
    ) -> usize {
        test_state()
            .lock()
            .expect("durable outcome classifier test-state lock should not be poisoned")
            .probes
            .get(&(tenant_id.clone(), route))
            .copied()
            .unwrap_or_default()
    }
}
