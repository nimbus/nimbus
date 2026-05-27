use nimbus_core::{Result, SequenceNumber};
use serde::{Deserialize, Serialize};

use crate::{
    CURRENT_STORAGE_FORMAT_VERSION, JournalProgress, LibsqlReplicaTenantStore, MySqlTenantStore,
    PostgresTenantStore, SqliteTenantStore, StorageFormatVersion, TableBackendLayout, TenantStore,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCapabilities {
    pub backend: String,
    pub backend_layout: TableBackendLayout,
    pub strong_reads: bool,
    pub eventual_reads: bool,
    pub tenant_event_journal: bool,
    pub retention_floor: bool,
    pub exact_summary: bool,
    pub encryption_posture: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageHealthDiagnostic {
    pub backend: String,
    pub backend_layout: TableBackendLayout,
    pub event_log_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    pub retention_floor: Option<SequenceNumber>,
    pub format_version: StorageFormatVersion,
    pub encryption_posture: String,
    pub freshness_lag: u64,
    pub last_recovery_status: String,
    pub exact_summary: bool,
}

fn capabilities(
    backend: &str,
    backend_layout: TableBackendLayout,
    eventual_reads: bool,
    encryption_posture: &str,
) -> StorageCapabilities {
    StorageCapabilities {
        backend: backend.to_string(),
        backend_layout,
        strong_reads: true,
        eventual_reads,
        tenant_event_journal: true,
        retention_floor: true,
        exact_summary: true,
        encryption_posture: encryption_posture.to_string(),
    }
}

fn diagnostic(
    capabilities: StorageCapabilities,
    progress: JournalProgress,
    retention_floor: Option<SequenceNumber>,
) -> StorageHealthDiagnostic {
    StorageHealthDiagnostic {
        backend: capabilities.backend,
        backend_layout: capabilities.backend_layout,
        event_log_head: progress.durable_head,
        applied_head: progress.applied_head,
        retention_floor,
        format_version: CURRENT_STORAGE_FORMAT_VERSION,
        encryption_posture: capabilities.encryption_posture,
        freshness_lag: progress
            .durable_head
            .0
            .saturating_sub(progress.applied_head.0),
        last_recovery_status: if progress.durable_head == progress.applied_head {
            "caught_up".to_string()
        } else {
            "pending_replay".to_string()
        },
        exact_summary: capabilities.exact_summary,
    }
}

impl TenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "redb",
            TableBackendLayout::RedbKeyspaceByTableId,
            false,
            "configured_per_store",
        )
    }

    pub fn storage_health_diagnostic(&self) -> Result<StorageHealthDiagnostic> {
        Ok(diagnostic(
            self.storage_capabilities(),
            self.journal_progress()?,
            self.retention_floor.lowest_pinned_sequence(),
        ))
    }
}

impl SqliteTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "sqlite",
            TableBackendLayout::SharedDocumentsByTableId,
            false,
            if self.is_encrypted() {
                "sqlcipher"
            } else {
                "not_configured"
            },
        )
    }

    pub fn storage_health_diagnostic(&self) -> Result<StorageHealthDiagnostic> {
        Ok(diagnostic(
            self.storage_capabilities(),
            self.journal_progress()?,
            self.retention_floor.lowest_pinned_sequence(),
        ))
    }
}

impl PostgresTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "postgres",
            TableBackendLayout::SharedDocumentsByTableId,
            false,
            "server_managed",
        )
    }

    pub fn storage_health_diagnostic(&self) -> Result<StorageHealthDiagnostic> {
        Ok(diagnostic(
            self.storage_capabilities(),
            self.journal_progress()?,
            self.retention_floor.lowest_pinned_sequence(),
        ))
    }
}

impl MySqlTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "mysql",
            TableBackendLayout::SharedDocumentsByTableId,
            false,
            "server_managed",
        )
    }

    pub fn storage_health_diagnostic(&self) -> Result<StorageHealthDiagnostic> {
        Ok(diagnostic(
            self.storage_capabilities(),
            self.journal_progress()?,
            self.retention_floor.lowest_pinned_sequence(),
        ))
    }
}

impl LibsqlReplicaTenantStore {
    pub fn storage_capabilities(&self) -> StorageCapabilities {
        capabilities(
            "libsql",
            TableBackendLayout::LibsqlReplicaSharedDocumentsByTableId,
            true,
            "replica_cache_optional",
        )
    }

    pub fn storage_health_diagnostic(&self) -> Result<StorageHealthDiagnostic> {
        Ok(diagnostic(
            self.storage_capabilities(),
            self.journal_progress()?,
            self.retention_floor.lowest_pinned_sequence(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_health_diagnostic_reports_backend_layout_and_heads() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let health = store
            .storage_health_diagnostic()
            .expect("diagnostic should load");
        assert_eq!(
            health.backend_layout,
            TableBackendLayout::RedbKeyspaceByTableId
        );
        assert_eq!(health.event_log_head, SequenceNumber(0));
        assert_eq!(health.applied_head, SequenceNumber(0));
        assert_eq!(health.format_version, CURRENT_STORAGE_FORMAT_VERSION);
    }
}
