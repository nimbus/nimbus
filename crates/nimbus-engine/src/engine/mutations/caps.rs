use std::sync::LazyLock;

use nimbus_core::{Error, Mutation, MutationCap, Result};
use serde::Serialize;
use tracing::warn;

use crate::tenant::TenantRuntime;

pub(in crate::engine) const DEFAULT_MUTATION_READ_BYTES: u64 = 1 << 24;
pub(in crate::engine) const DEFAULT_MUTATION_WRITE_BYTES: u64 = 1 << 24;
pub(in crate::engine) const DEFAULT_MUTATION_DOCUMENTS_SCANNED: u64 = 32_000;
pub(in crate::engine) const DEFAULT_MUTATION_DOCUMENTS_WRITTEN: u64 = 16_000;
pub(in crate::engine) const DEFAULT_MUTATION_INDEX_RANGE_CALLS: u64 = 4_096;
pub(in crate::engine) const DEFAULT_SYSTEM_MUTATION_WRITE_BYTES: u64 = 1 << 27;
pub(in crate::engine) const DEFAULT_SYSTEM_MUTATION_DOCUMENTS_WRITTEN: u64 = 40_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::engine) struct MutationUsage {
    pub(in crate::engine) read_bytes: u64,
    pub(in crate::engine) write_bytes: u64,
    pub(in crate::engine) documents_scanned: u64,
    pub(in crate::engine) documents_written: u64,
    pub(in crate::engine) index_range_calls: u64,
    pub(in crate::engine) system_write_bytes: u64,
    pub(in crate::engine) system_documents_written: u64,
}

impl MutationUsage {
    pub(in crate::engine) fn for_journal_admission(
        mutation: &Mutation,
        has_scheduled_bookkeeping: bool,
    ) -> Self {
        Self {
            write_bytes: serialized_len(mutation),
            documents_written: 1,
            system_write_bytes: has_scheduled_bookkeeping
                .then(|| serialized_len(&"scheduled_execution"))
                .unwrap_or(0),
            system_documents_written: u64::from(has_scheduled_bookkeeping),
            ..Self::default()
        }
    }

    pub(in crate::engine) fn record_documents_read<'a>(
        &mut self,
        documents: impl IntoIterator<Item = &'a nimbus_core::Document>,
    ) {
        for document in documents {
            self.documents_scanned = self.documents_scanned.saturating_add(1);
            self.read_bytes = self.read_bytes.saturating_add(serialized_len(document));
        }
    }

    pub(in crate::engine) fn record_index_range_call(&mut self) {
        self.index_range_calls = self.index_range_calls.saturating_add(1);
    }

    pub(in crate::engine) fn add_user_write<T: Serialize>(&mut self, value: &T) {
        self.documents_written = self.documents_written.saturating_add(1);
        self.write_bytes = self.write_bytes.saturating_add(serialized_len(value));
    }

    pub(in crate::engine) fn add_system_write<T: Serialize>(&mut self, value: &T) {
        self.system_documents_written = self.system_documents_written.saturating_add(1);
        self.system_write_bytes = self
            .system_write_bytes
            .saturating_add(serialized_len(value));
    }

    fn observed(self, cap: MutationCap) -> u64 {
        match cap {
            MutationCap::ReadBytes => self.read_bytes,
            MutationCap::WriteBytes => self.write_bytes,
            MutationCap::DocumentsScanned => self.documents_scanned,
            MutationCap::DocumentsWritten => self.documents_written,
            MutationCap::IndexRangeCalls => self.index_range_calls,
        }
    }
}

pub(in crate::engine) fn serialized_len<T: Serialize>(value: &T) -> u64 {
    serde_json::to_vec(value)
        .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy)]
struct CapSetting {
    proposed: u64,
    enforced: Option<u64>,
}

impl CapSetting {
    #[cfg(test)]
    const fn new(proposed: u64, enforced: Option<u64>) -> Self {
        Self { proposed, enforced }
    }

    fn from_env(name: &'static str, proposed_default: u64) -> Self {
        Self {
            proposed: env_positive_u64(&format!("NIMBUS_PROPOSED_{name}"))
                .unwrap_or(proposed_default),
            enforced: env_positive_u64(&format!("NIMBUS_{name}")),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::engine) struct MutationCapConfig {
    user: [CapSetting; 5],
    system_write_bytes: CapSetting,
    system_documents_written: CapSetting,
}

impl MutationCapConfig {
    fn from_env() -> Self {
        Self {
            user: [
                CapSetting::from_env("MUTATION_READ_BYTES", DEFAULT_MUTATION_READ_BYTES),
                CapSetting::from_env("MUTATION_WRITE_BYTES", DEFAULT_MUTATION_WRITE_BYTES),
                CapSetting::from_env(
                    "MUTATION_DOCUMENTS_SCANNED",
                    DEFAULT_MUTATION_DOCUMENTS_SCANNED,
                ),
                CapSetting::from_env(
                    "MUTATION_DOCUMENTS_WRITTEN",
                    DEFAULT_MUTATION_DOCUMENTS_WRITTEN,
                ),
                CapSetting::from_env(
                    "MUTATION_INDEX_RANGE_CALLS",
                    DEFAULT_MUTATION_INDEX_RANGE_CALLS,
                ),
            ],
            system_write_bytes: CapSetting::from_env(
                "SYSTEM_MUTATION_WRITE_BYTES",
                DEFAULT_SYSTEM_MUTATION_WRITE_BYTES,
            ),
            system_documents_written: CapSetting::from_env(
                "SYSTEM_MUTATION_DOCUMENTS_WRITTEN",
                DEFAULT_SYSTEM_MUTATION_DOCUMENTS_WRITTEN,
            ),
        }
    }

    #[cfg(test)]
    fn for_tests(proposed: [u64; 5], enforced: [Option<u64>; 5]) -> Self {
        Self {
            user: std::array::from_fn(|index| CapSetting::new(proposed[index], enforced[index])),
            system_write_bytes: CapSetting::new(1_000, Some(1_000)),
            system_documents_written: CapSetting::new(1_000, Some(1_000)),
        }
    }

    #[cfg(test)]
    fn with_system_write_limits(mut self, bytes: u64, documents: u64) -> Self {
        self.system_write_bytes = CapSetting::new(bytes, Some(bytes));
        self.system_documents_written = CapSetting::new(documents, Some(documents));
        self
    }
}

const CAPS: [MutationCap; 5] = [
    MutationCap::ReadBytes,
    MutationCap::WriteBytes,
    MutationCap::DocumentsScanned,
    MutationCap::DocumentsWritten,
    MutationCap::IndexRangeCalls,
];

static MUTATION_CAP_CONFIG: LazyLock<MutationCapConfig> =
    LazyLock::new(MutationCapConfig::from_env);

pub(in crate::engine) fn check_mutation_caps(
    runtime: &TenantRuntime,
    usage: MutationUsage,
) -> Result<()> {
    check_mutation_caps_with_config(runtime, usage, &MUTATION_CAP_CONFIG)
}

fn check_mutation_caps_with_config(
    runtime: &TenantRuntime,
    usage: MutationUsage,
    config: &MutationCapConfig,
) -> Result<()> {
    for (cap, setting) in CAPS.into_iter().zip(config.user) {
        check_one(runtime, cap, usage.observed(cap), setting)?;
    }
    check_one(
        runtime,
        MutationCap::WriteBytes,
        usage.system_write_bytes,
        config.system_write_bytes,
    )?;
    check_one(
        runtime,
        MutationCap::DocumentsWritten,
        usage.system_documents_written,
        config.system_documents_written,
    )?;
    Ok(())
}

fn check_one(
    runtime: &TenantRuntime,
    cap: MutationCap,
    observed: u64,
    setting: CapSetting,
) -> Result<()> {
    if observed > setting.proposed
        && runtime
            .commit_phase_metrics()
            .record_shadow_cap_violation(cap)
    {
        warn!(
            tenant = %runtime.tenant_id(),
            cap = cap.as_str(),
            observed,
            limit = setting.proposed,
            "mutation would exceed proposed prepare-time cap"
        );
    }
    if let Some(limit) = setting.enforced
        && observed > limit
    {
        return Err(Error::cap_exceeded(cap, observed, limit));
    }
    Ok(())
}

fn env_positive_u64(key: &str) -> Option<u64> {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{MutationCap, PrincipalContext, TenantId, Timestamp};
    use nimbus_storage::{ManualClock, NoopFaultInjector};
    use tempfile::TempDir;

    use super::*;
    use crate::Engine;

    fn runtime() -> (TempDir, Arc<Engine>, TenantId) {
        let data_dir = tempfile::tempdir().expect("tempdir should build");
        let tenant_id = TenantId::new("cap-tests").expect("tenant should parse");
        let clock = Arc::new(ManualClock::new(Timestamp(1)));
        let engine = Arc::new(
            Engine::new_with_simulation(data_dir.path(), clock, Arc::new(NoopFaultInjector))
                .expect("engine should build"),
        );
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should build");
        // Exercise the ordinary runtime acquisition path used by mutations.
        engine
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("execution unit should build");
        (data_dir, engine, tenant_id)
    }

    fn usage_for(cap: MutationCap, observed: u64) -> MutationUsage {
        let mut usage = MutationUsage::default();
        match cap {
            MutationCap::ReadBytes => usage.read_bytes = observed,
            MutationCap::WriteBytes => usage.write_bytes = observed,
            MutationCap::DocumentsScanned => usage.documents_scanned = observed,
            MutationCap::DocumentsWritten => usage.documents_written = observed,
            MutationCap::IndexRangeCalls => usage.index_range_calls = observed,
        }
        usage
    }

    #[test]
    fn every_cap_supports_shadow_enforce_and_exactly_at_limit_modes() {
        for (index, cap) in CAPS.into_iter().enumerate() {
            let (_data_dir, engine, tenant_id) = runtime();
            let tenant = engine
                .get_existing_tenant(&tenant_id)
                .expect("tenant should load");
            let mut proposed = [u64::MAX; 5];
            proposed[index] = 10;

            let shadow_config = MutationCapConfig::for_tests(proposed, [None; 5]);
            check_mutation_caps_with_config(&tenant, usage_for(cap, 11), &shadow_config)
                .expect("shadow mode must not reject");
            assert_eq!(tenant.commit_phase_metrics().shadow_cap_violations(cap), 1);
            assert_eq!(
                tenant
                    .commit_phase_metrics()
                    .snapshot()
                    .shadow_cap_logs_total,
                1,
                "the first violation must always be selected for logging"
            );

            let mut enforced = [None; 5];
            enforced[index] = Some(10);
            let enforce_config = MutationCapConfig::for_tests([u64::MAX; 5], enforced);
            let error =
                check_mutation_caps_with_config(&tenant, usage_for(cap, 11), &enforce_config)
                    .expect_err("enforced cap should reject an over-limit mutation");
            assert!(matches!(
                error,
                Error::CapExceeded {
                    cap: actual,
                    observed: 11,
                    limit: 10,
                } if actual == cap
            ));

            check_mutation_caps_with_config(&tenant, usage_for(cap, 10), &enforce_config)
                .expect("usage exactly at the cap should succeed");
        }
    }

    #[test]
    fn system_tier_writes_are_exempt_from_user_write_caps() {
        let (_data_dir, engine, tenant_id) = runtime();
        let tenant = engine
            .get_existing_tenant(&tenant_id)
            .expect("tenant should load");
        let config = MutationCapConfig::for_tests(
            [u64::MAX, 1, u64::MAX, 1, u64::MAX],
            [None, Some(1), None, Some(1), None],
        )
        .with_system_write_limits(100, 10);
        let usage = MutationUsage {
            system_write_bytes: 100,
            system_documents_written: 10,
            ..MutationUsage::default()
        };

        check_mutation_caps_with_config(&tenant, usage, &config)
            .expect("system writes should use their separate higher tier");
    }
}
