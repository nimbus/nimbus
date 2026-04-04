use std::sync::Arc;

use neovex_core::{Error, Result, TenantId};
use neovex_storage::{ShadowMaterializer, ShadowMaterializerConfig, TenantStore};

use super::snapshot::rebuild_authoritative_snapshot;
use crate::EmbeddedReplica;
use crate::service::Service;
use crate::verification::{
    ConsistencyScope, ConsistencyVerificationReport, bootstrap_fingerprint,
    collect_durable_journal_bootstrap_mismatches, compare_materialized_journal_snapshots,
    snapshot_fingerprint,
};

impl Service {
    /// Builds a shadow materializer from the current authoritative journal state.
    pub async fn build_shadow_materializer_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        config: ShadowMaterializerConfig,
    ) -> Result<ShadowMaterializer> {
        let bootstrap = self
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;
        let mut after = bootstrap.resume_after;
        let mut tail = Vec::new();
        while after.0 < bootstrap.bootstrap_cut.0 {
            let page = self
                .stream_durable_journal_async(tenant_id.clone(), after, 256)
                .await?;
            let page_records = page
                .records
                .into_iter()
                .take_while(|record| record.sequence.0 <= bootstrap.bootstrap_cut.0)
                .collect::<Vec<_>>();
            let Some(last_record) = page_records.last() else {
                return Err(Error::Internal(format!(
                    "journal stream made no progress while building shadow materializer for tenant {} up to sequence {} from {}",
                    tenant_id, bootstrap.bootstrap_cut.0, after.0
                )));
            };
            after = last_record.sequence;
            tail.extend(page_records);
        }
        ShadowMaterializer::from_checkpoint_and_journal(bootstrap.snapshot, tail, config)
    }

    /// Verifies authoritative and derived tenant state against one bootstrap cut.
    pub async fn verify_consistency_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<ConsistencyVerificationReport> {
        let bootstrap = self
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await?;
        let journal_tail = self
            .read_durable_journal_suffix_to_sequence_async(&tenant_id, &bootstrap)
            .await?;
        let authoritative_snapshot = rebuild_authoritative_snapshot(&bootstrap, &journal_tail)?;

        let shadow = ShadowMaterializer::from_checkpoint_and_journal(
            bootstrap.snapshot.clone(),
            journal_tail.clone(),
            ShadowMaterializerConfig::default(),
        )?;
        let shadow_snapshot = shadow.current_snapshot();

        let replica = EmbeddedReplica::bootstrap_from_bootstrap(
            tenant_id.clone(),
            TenantStore::create_in_memory()?,
            bootstrap.clone(),
            journal_tail,
        )?;
        let replica_snapshot = replica.export_materialized_journal_snapshot()?;

        let mut mismatches = Vec::new();
        if let Some(mismatch) = compare_materialized_journal_snapshots(
            ConsistencyScope::AuthoritativeSnapshot,
            &authoritative_snapshot,
            ConsistencyScope::ShadowMaterializer,
            &shadow_snapshot,
        ) {
            mismatches.push(mismatch);
        }
        if let Some(mismatch) = compare_materialized_journal_snapshots(
            ConsistencyScope::AuthoritativeSnapshot,
            &authoritative_snapshot,
            ConsistencyScope::EmbeddedReplica,
            &replica_snapshot,
        ) {
            mismatches.push(mismatch);
        }
        mismatches.extend(collect_durable_journal_bootstrap_mismatches(
            &bootstrap.snapshot,
            &bootstrap,
        ));

        Ok(ConsistencyVerificationReport {
            tenant_id: tenant_id.to_string(),
            ok: mismatches.is_empty(),
            authoritative: snapshot_fingerprint(&authoritative_snapshot)?,
            shadow: snapshot_fingerprint(&shadow_snapshot)?,
            embedded_replica: snapshot_fingerprint(&replica_snapshot)?,
            bootstrap: bootstrap_fingerprint(&bootstrap)?,
            mismatches,
        })
    }
}
