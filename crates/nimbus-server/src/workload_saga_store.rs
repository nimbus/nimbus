//! Server-owned Engine adapter for durable workload-saga state.
//!
//! Compute owns saga choreography. This adapter remains in the server because
//! it translates that portable store contract onto the server-composed Engine
//! mutation path; embedders may inject it through the public constructor.

use std::sync::Arc;

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, Error, PrincipalContext, WriteKey,
    WritePrecondition, WriteSetMode,
};
use nimbus_engine::Engine;
use nimbus_workloads::{
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

mod codec;
mod recovery;
mod restart_candidates;
mod schema;
mod tenant_enumeration;

use self::codec::{decode_workload_saga_record, encode_workload_saga_record};
use self::schema::{prepare_exact_schema, workload_saga_table, workload_saga_tenant};

pub struct EngineWorkloadSagaStore {
    engine: Arc<Engine>,
}

impl EngineWorkloadSagaStore {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }

    pub(crate) async fn prepare(&self) -> Result<(), WorkloadSagaStoreError> {
        prepare_exact_schema(&self.engine).await
    }
}

pub(crate) async fn prepare_for_server(engine: &Arc<Engine>) -> nimbus_core::Result<()> {
    prepare_exact_schema(engine).await.map_err(|error| {
        Error::Internal(format!(
            "durable workload-saga schema preparation failed: {error}"
        ))
    })
}

impl WorkloadSagaStore for EngineWorkloadSagaStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.prepare().await?;
            let engine = Arc::clone(&self.engine);
            let key = key.clone();
            tokio::task::spawn_blocking(move || load_blocking(&engine, &key))
                .await
                .map_err(|_| WorkloadSagaStoreError::Unavailable)?
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.prepare().await?;
            let engine = Arc::clone(&self.engine);
            tokio::task::spawn_blocking(move || compare_and_swap_blocking(&engine, expected, next))
                .await
                .map_err(|_| WorkloadSagaStoreError::Ambiguous)?
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move {
            self.prepare().await?;
            recovery::list_recoverable(&self.engine, request).await
        })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadRestartCandidatePage> {
        Box::pin(async move {
            self.prepare().await?;
            restart_candidates::list_restart_candidates(&self.engine, request).await
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a nimbus_core::TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move {
            self.prepare().await?;
            tenant_enumeration::list_for_tenant(&self.engine, tenant_id, request).await
        })
    }
}

fn load_blocking(
    engine: &Arc<Engine>,
    key: &WorkloadSagaKey,
) -> Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError> {
    let tenant = workload_saga_tenant()?;
    let table = workload_saga_table()?;
    let document_id = document_id(key)?;
    let unit = engine
        .begin_mutation_execution_unit(tenant, PrincipalContext::system())
        .map_err(|_| WorkloadSagaStoreError::Unavailable)?;
    unit.get_document(&table, document_id)
        .map_err(|_| WorkloadSagaStoreError::Unavailable)?
        .as_ref()
        .map(decode_workload_saga_record)
        .transpose()
}

fn compare_and_swap_blocking(
    engine: &Arc<Engine>,
    expected: WorkloadSagaExpected,
    next: WorkloadSagaRecord,
) -> Result<WorkloadSagaCommit, WorkloadSagaStoreError> {
    next.validate()?;
    let tenant = workload_saga_tenant()?;
    let table = workload_saga_table()?;
    let document_id = document_id(next.key())?;
    let unit = engine
        .begin_mutation_execution_unit(tenant, PrincipalContext::system())
        .map_err(|_| WorkloadSagaStoreError::Unavailable)?;
    let loaded_document = unit
        .get_document(&table, document_id.clone())
        .map_err(|_| WorkloadSagaStoreError::Unavailable)?;
    let loaded = loaded_document
        .as_ref()
        .map(decode_workload_saga_record)
        .transpose()?;

    if loaded.as_ref() == Some(&next) {
        return Ok(WorkloadSagaCommit::Unchanged);
    }
    if loaded.as_ref().is_some_and(|current| {
        current.last_transition().transition_id() == next.last_transition().transition_id()
    }) {
        return Err(WorkloadSagaStoreError::InvalidTransition(
            nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "transition id is already bound to different saga content",
            ),
        ));
    }

    let precondition = match (expected, loaded.as_ref(), loaded_document.as_ref()) {
        (WorkloadSagaExpected::Missing, None, None) => {
            if next.revision().as_u64() != 0 || next.last_transition().source_phase().is_some() {
                return Err(WorkloadSagaStoreError::InvalidTransition(
                    nimbus_workloads::WorkloadSagaError::InvalidTransition(
                        "missing-store creation requires the initial revision",
                    ),
                ));
            }
            WritePrecondition::exists(false)
        }
        (WorkloadSagaExpected::Missing, Some(current), _) => {
            return Err(conflict(expected, Some(current.revision())));
        }
        (WorkloadSagaExpected::Revision(revision), Some(current), Some(document))
            if current.revision() == revision =>
        {
            current.validate_successor(&next)?;
            WritePrecondition::update_time(document.update_time)
        }
        (WorkloadSagaExpected::Revision(_), Some(current), _) => {
            return Err(conflict(expected, Some(current.revision())));
        }
        (WorkloadSagaExpected::Revision(_), None, _) => {
            return Err(conflict(expected, None));
        }
        _ => return Err(WorkloadSagaStoreError::Corrupt),
    };

    unit.stage_atomic_write_batch(
        AtomicWriteBatch::new(vec![AtomicWrite::Set {
            key: WriteKey::from(DocumentLocator::new(table, document_id)),
            document: encode_workload_saga_record(&next)?,
            typed_fields: Default::default(),
            mode: WriteSetMode::Overwrite,
            precondition,
            transforms: Vec::new(),
        }])
        .map_err(|_| WorkloadSagaStoreError::Unavailable)?,
    )
    .map_err(|_| WorkloadSagaStoreError::Unavailable)?;

    match unit.commit() {
        Ok(Some(_)) => Ok(WorkloadSagaCommit::Applied),
        Ok(None) => Err(WorkloadSagaStoreError::Unavailable),
        Err(Error::Conflict { .. }) => {
            let observed = load_blocking(engine, next.key())
                .ok()
                .flatten()
                .map(|record| record.revision());
            Err(conflict(expected, observed))
        }
        Err(_) => Err(WorkloadSagaStoreError::Ambiguous),
    }
}

fn document_id(key: &WorkloadSagaKey) -> Result<DocumentId, WorkloadSagaStoreError> {
    DocumentId::from_key(key.saga_id().as_str()).map_err(|_| WorkloadSagaStoreError::Corrupt)
}

fn conflict(
    expected: WorkloadSagaExpected,
    observed: Option<nimbus_workloads::WorkloadSagaRevision>,
) -> WorkloadSagaStoreError {
    WorkloadSagaStoreError::Conflict { expected, observed }
}

#[cfg(test)]
#[path = "workload_saga_store/tests/mod.rs"]
mod tests;
