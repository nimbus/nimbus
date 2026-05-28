use std::sync::{Arc, Weak};

use nimbus_core::{TableName, TenantId};
use nimbus_engine::{
    CommittedMutationEvent, CommittedMutationObserver, Service, TableSchemaChangeEvent,
    TableSchemaChangeObserver,
};
use tracing::warn;

use super::{is_reserved_tenant_id, record_table_state_async};

const TABLE_PROJECTION_OBSERVER: &str = "nimbus-system-table-projection";

struct TableProjectionObserver {
    service: Weak<Service>,
    projection_lock: Arc<tokio::sync::Mutex<()>>,
}

impl CommittedMutationObserver for TableProjectionObserver {
    fn committed_mutation_applied(&self, event: CommittedMutationEvent) {
        if is_reserved_tenant_id(&event.tenant_id) {
            return;
        }
        let tables = event
            .commit
            .affected_tables()
            .into_iter()
            .collect::<Vec<_>>();
        if tables.is_empty() {
            return;
        }
        self.project_tables(event.tenant_id, tables);
    }
}

impl TableSchemaChangeObserver for TableProjectionObserver {
    fn table_schema_changed(&self, event: TableSchemaChangeEvent) {
        if is_reserved_tenant_id(&event.tenant_id) {
            return;
        }
        self.project_tables(event.tenant_id, vec![event.table]);
    }
}

impl TableProjectionObserver {
    fn project_tables(&self, tenant_id: TenantId, mut tables: Vec<TableName>) {
        let Some(service) = self.service.upgrade() else {
            return;
        };
        let projection_lock = self.projection_lock.clone();
        tables.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!(
                tenant_id = %tenant_id,
                "skipping system table projection because no tokio runtime is active"
            );
            return;
        };
        handle.spawn(async move {
            let _projection_guard = projection_lock.lock().await;
            for table in tables {
                if let Err(error) = record_table_state_async(&service, &tenant_id, &table).await {
                    warn!(
                        tenant_id = %tenant_id,
                        table = %table,
                        error = %error,
                        "failed to project committed table state into _nimbus"
                    );
                }
            }
        });
    }
}

pub fn install_table_projection_observer(service: &Arc<Service>) {
    let observer = Arc::new(TableProjectionObserver {
        service: Arc::downgrade(service),
        projection_lock: Arc::new(tokio::sync::Mutex::new(())),
    });
    service.install_committed_mutation_observer(TABLE_PROJECTION_OBSERVER, observer.clone());
    service.install_table_schema_change_observer(TABLE_PROJECTION_OBSERVER, observer);
}
