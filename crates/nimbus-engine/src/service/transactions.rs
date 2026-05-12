use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nimbus_core::{
    AtomicWriteBatch, AtomicWriteBatchOutcome, CollectionName, Document, DocumentId, DocumentPath,
    Error, PrincipalContext, Result, StructuredQuery, TableName, TenantId, Timestamp,
    TransactionSession, TransactionSessionMode, TransactionSessionToken,
};
use rand::RngCore;

use super::MutationExecutionUnit;
use super::Service;

const TRANSACTION_SESSION_TTL_MS: u64 = 60_000;
const MAX_ACTIVE_TRANSACTION_SESSIONS: usize = 256;
const TRANSACTION_SESSION_TOKEN_PREFIX: &str = "txn_";
const TRANSACTION_SESSION_TOKEN_BYTES: usize = 24;

#[derive(Clone)]
struct StoredTransactionSession {
    tenant_id: TenantId,
    principal: PrincipalContext,
    session: TransactionSession,
    execution_unit: Arc<MutationExecutionUnit>,
}

#[derive(Default)]
pub(super) struct TransactionSessionRegistry {
    sessions: HashMap<TransactionSessionToken, StoredTransactionSession>,
}

impl TransactionSessionRegistry {
    fn insert(&mut self, session: StoredTransactionSession, now: Timestamp) -> Result<()> {
        self.prune_expired(now);
        if self.sessions.len() >= MAX_ACTIVE_TRANSACTION_SESSIONS {
            return Err(Error::ResourceExhausted(format!(
                "too many active transaction sessions; limit is {MAX_ACTIVE_TRANSACTION_SESSIONS}"
            )));
        }
        self.sessions.insert(session.session.token.clone(), session);
        Ok(())
    }

    fn clone_active(
        &mut self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
        now: Timestamp,
    ) -> Result<StoredTransactionSession> {
        let Some(session) = self.sessions.get(token).cloned() else {
            return Err(transaction_session_not_found());
        };
        self.ensure_access(token, tenant_id, principal, now, session, false)
    }

    fn take_active(
        &mut self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
        now: Timestamp,
    ) -> Result<StoredTransactionSession> {
        let Some(session) = self.sessions.remove(token) else {
            return Err(transaction_session_not_found());
        };
        self.ensure_access(token, tenant_id, principal, now, session, true)
    }

    fn ensure_access(
        &mut self,
        token: &TransactionSessionToken,
        tenant_id: &TenantId,
        principal: &PrincipalContext,
        now: Timestamp,
        session: StoredTransactionSession,
        already_removed: bool,
    ) -> Result<StoredTransactionSession> {
        if session.session.expires_at <= now {
            if !already_removed {
                self.sessions.remove(token);
            }
            return Err(transaction_session_expired());
        }
        if &session.tenant_id != tenant_id {
            if !already_removed {
                self.sessions.remove(token);
            }
            return Err(Error::PermissionDenied(
                "transaction session tenant mismatch".to_string(),
            ));
        }
        if &session.principal != principal {
            if !already_removed {
                self.sessions.remove(token);
            }
            return Err(Error::PermissionDenied(
                "transaction session principal mismatch".to_string(),
            ));
        }
        Ok(session)
    }

    fn prune_expired(&mut self, now: Timestamp) -> usize {
        let before = self.sessions.len();
        self.sessions
            .retain(|_, session| session.session.expires_at > now);
        before.saturating_sub(self.sessions.len())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.sessions.len()
    }
}

impl Service {
    /// Begins a server-owned transaction session so transports keep only an
    /// opaque token instead of storing raw execution units locally.
    pub fn begin_transaction_session(
        self: &Arc<Self>,
        tenant_id: TenantId,
        principal: PrincipalContext,
        mode: TransactionSessionMode,
    ) -> Result<TransactionSession> {
        let started_at = self.now();
        let session = TransactionSession {
            token: generate_transaction_session_token()?,
            mode,
            started_at,
            expires_at: Timestamp(started_at.0.saturating_add(TRANSACTION_SESSION_TTL_MS)),
        };
        let execution_unit =
            self.begin_mutation_execution_unit(tenant_id.clone(), principal.clone())?;
        self.transaction_sessions
            .write()
            .expect("transaction session lock should not be poisoned")
            .insert(
                StoredTransactionSession {
                    tenant_id,
                    principal,
                    session: session.clone(),
                    execution_unit,
                },
                started_at,
            )?;
        Ok(session)
    }

    /// Reads one document through the pinned transaction snapshot and staged writes.
    pub fn get_document_in_transaction(
        &self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Option<Document>> {
        let execution_unit = self.transaction_execution_unit(tenant_id, token, principal)?;
        execution_unit.get_document(table, document_id)
    }

    /// Evaluates one structured query through the pinned transaction snapshot.
    pub fn query_documents_structured_in_transaction(
        &self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
        table: &TableName,
        query: &StructuredQuery,
    ) -> Result<Vec<Document>> {
        let execution_unit = self.transaction_execution_unit(tenant_id, token, principal)?;
        execution_unit.query_documents_structured_cancellable(table, query, &mut || Ok(()))
    }

    /// Evaluates one collection-group structured query through the pinned
    /// transaction snapshot.
    pub fn query_collection_group_documents_structured_in_transaction(
        &self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
        collection_group: &CollectionName,
        ancestor: Option<&DocumentPath>,
        query: &StructuredQuery,
    ) -> Result<Vec<(DocumentPath, Document)>> {
        let execution_unit = self.transaction_execution_unit(tenant_id, token, principal)?;
        execution_unit.query_collection_group_documents_structured_cancellable(
            collection_group,
            ancestor,
            query,
            &mut || Ok(()),
        )
    }

    /// Commits a transaction session exactly once, optionally applying an
    /// atomic write batch as part of the final commit request.
    pub fn commit_transaction_session(
        &self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
        batch: Option<AtomicWriteBatch>,
    ) -> Result<AtomicWriteBatchOutcome> {
        let session = self
            .transaction_sessions
            .write()
            .expect("transaction session lock should not be poisoned")
            .take_active(tenant_id, token, principal, self.now())?;
        if matches!(session.session.mode, TransactionSessionMode::ReadOnly)
            && batch.as_ref().is_some_and(|batch| !batch.writes.is_empty())
        {
            return Err(Error::InvalidInput(
                "read-only transaction session cannot commit writes".to_string(),
            ));
        }

        match batch {
            Some(batch) if !batch.writes.is_empty() => {
                session.execution_unit.execute_atomic_write_batch(batch)
            }
            Some(_) | None => {
                commit_transaction_without_writes(&session.execution_unit, self.now())
            }
        }
    }

    /// Rolls back a transaction session by dropping the pinned execution unit.
    pub fn rollback_transaction_session(
        &self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
    ) -> Result<()> {
        let _ = self
            .transaction_sessions
            .write()
            .expect("transaction session lock should not be poisoned")
            .take_active(tenant_id, token, principal, self.now())?;
        Ok(())
    }

    /// Removes expired transaction sessions and returns the number pruned.
    pub fn prune_expired_transaction_sessions(&self) -> usize {
        self.transaction_sessions
            .write()
            .expect("transaction session lock should not be poisoned")
            .prune_expired(self.now())
    }

    #[cfg(test)]
    fn active_transaction_session_count(&self) -> usize {
        self.transaction_sessions
            .read()
            .expect("transaction session lock should not be poisoned")
            .len()
    }

    fn transaction_execution_unit(
        &self,
        tenant_id: &TenantId,
        token: &TransactionSessionToken,
        principal: &PrincipalContext,
    ) -> Result<Arc<MutationExecutionUnit>> {
        Ok(self
            .transaction_sessions
            .write()
            .expect("transaction session lock should not be poisoned")
            .clone_active(tenant_id, token, principal, self.now())?
            .execution_unit)
    }
}

fn generate_transaction_session_token() -> Result<TransactionSessionToken> {
    let mut bytes = [0_u8; TRANSACTION_SESSION_TOKEN_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    TransactionSessionToken::new(format!(
        "{TRANSACTION_SESSION_TOKEN_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes)
    ))
}

fn transaction_session_not_found() -> Error {
    Error::InvalidInput("transaction session is not active; begin a new transaction".to_string())
}

fn transaction_session_expired() -> Error {
    Error::InvalidInput("transaction session expired; begin a new transaction".to_string())
}

fn commit_transaction_without_writes(
    execution_unit: &MutationExecutionUnit,
    now: Timestamp,
) -> Result<AtomicWriteBatchOutcome> {
    let commit = execution_unit.commit()?;
    let commit_time = commit
        .as_ref()
        .map(|commit| commit.timestamp)
        .unwrap_or(now);
    Ok(AtomicWriteBatchOutcome {
        commit,
        commit_time,
        write_results: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{
        AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, Error, PrincipalContext,
        TableName, TenantId, Timestamp, TransactionSessionMode, WriteKey, WritePrecondition,
    };
    use nimbus_storage::{ManualClock, NoopFaultInjector};
    use nimbus_testing::ServiceFixture;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{Service, TRANSACTION_SESSION_TTL_MS};
    use crate::test_support::{messages_table, principal_with_subject};

    fn patch_body_batch(
        table: &TableName,
        document_id: &DocumentId,
        body: &str,
    ) -> AtomicWriteBatch {
        AtomicWriteBatch {
            writes: vec![AtomicWrite::Patch {
                key: WriteKey::from(DocumentLocator::new(table.clone(), document_id.clone())),
                field_patch: serde_json::Map::from_iter([("body".to_string(), json!(body))]),
                mask: vec!["body".to_string()],
                precondition: WritePrecondition::default(),
                transforms: Vec::new(),
            }],
        }
    }

    #[test]
    fn transaction_session_reads_and_commits_atomic_batch() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
        let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
        let table = messages_table("messages_txn_session_commit");
        let document_id = service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([("body".to_string(), json!("before"))]),
            )
            .expect("fixture insert should succeed");
        let principal = PrincipalContext::anonymous();

        let session = service
            .begin_transaction_session(
                tenant_id.clone(),
                principal.clone(),
                TransactionSessionMode::ReadWrite,
            )
            .expect("transaction session should start");
        let read_back = service
            .get_document_in_transaction(
                &tenant_id,
                &session.token,
                &principal,
                &table,
                document_id.clone(),
            )
            .expect("transactional read should succeed")
            .expect("document should exist");
        assert_eq!(read_back.get_field("body"), Some(&json!("before")));

        let outcome = service
            .commit_transaction_session(
                &tenant_id,
                &session.token,
                &principal,
                Some(patch_body_batch(&table, &document_id, "after")),
            )
            .expect("transaction commit should succeed");

        assert_eq!(outcome.write_results.len(), 1);
        assert!(outcome.commit.is_some(), "commit should produce an entry");
        assert_eq!(
            service
                .get_document(&tenant_id, &table, document_id.clone())
                .expect("document should exist after commit")
                .get_field("body"),
            Some(&json!("after"))
        );
        assert!(matches!(
            service.rollback_transaction_session(&tenant_id, &session.token, &principal),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn transaction_session_point_reads_stay_on_the_begin_snapshot() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
        let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
        let table = messages_table("messages_txn_session_snapshot");
        let document_id = service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([("body".to_string(), json!("before"))]),
            )
            .expect("fixture insert should succeed");
        let principal = PrincipalContext::anonymous();

        let session = service
            .begin_transaction_session(
                tenant_id.clone(),
                principal.clone(),
                TransactionSessionMode::ReadOnly,
            )
            .expect("transaction session should start");
        service
            .update_document(
                &tenant_id,
                table.clone(),
                document_id.clone(),
                serde_json::Map::from_iter([("body".to_string(), json!("outside"))]),
            )
            .expect("outside update should succeed");

        let read_back = service
            .get_document_in_transaction(
                &tenant_id,
                &session.token,
                &principal,
                &table,
                document_id,
            )
            .expect("transactional point read should succeed")
            .expect("document should exist");

        assert_eq!(read_back.get_field("body"), Some(&json!("before")));
    }

    #[test]
    fn transaction_session_rollback_removes_active_session() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
        let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
        let principal = PrincipalContext::anonymous();

        let session = service
            .begin_transaction_session(
                tenant_id.clone(),
                principal.clone(),
                TransactionSessionMode::ReadWrite,
            )
            .expect("transaction session should start");
        assert_eq!(service.active_transaction_session_count(), 1);

        service
            .rollback_transaction_session(&tenant_id, &session.token, &principal)
            .expect("rollback should succeed");

        assert_eq!(service.active_transaction_session_count(), 0);
        assert!(matches!(
            service.rollback_transaction_session(&tenant_id, &session.token, &principal),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn transaction_session_expiry_prunes_the_session() {
        let data_dir = tempdir().expect("service tempdir should create");
        let clock = Arc::new(ManualClock::new(Timestamp(5_000)));
        let service = Arc::new(
            Service::new_with_simulation(
                data_dir.path(),
                clock.clone(),
                Arc::new(NoopFaultInjector),
            )
            .expect("service should create"),
        );
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let principal = PrincipalContext::anonymous();

        let session = service
            .begin_transaction_session(
                tenant_id.clone(),
                principal.clone(),
                TransactionSessionMode::ReadOnly,
            )
            .expect("transaction session should start");
        assert_eq!(service.active_transaction_session_count(), 1);

        clock.advance_ms(TRANSACTION_SESSION_TTL_MS.saturating_add(1));
        let error = service
            .rollback_transaction_session(&tenant_id, &session.token, &principal)
            .expect_err("expired transaction should fail");

        assert!(matches!(error, Error::InvalidInput(_)));
        assert_eq!(service.active_transaction_session_count(), 0);
        assert_eq!(service.prune_expired_transaction_sessions(), 0);
    }

    #[test]
    fn transaction_session_principal_mismatch_invalidates_the_session() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
        let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
        let owner = principal_with_subject("owner-1");
        let intruder = principal_with_subject("owner-2");

        let session = service
            .begin_transaction_session(
                tenant_id.clone(),
                owner.clone(),
                TransactionSessionMode::ReadWrite,
            )
            .expect("transaction session should start");
        assert_eq!(service.active_transaction_session_count(), 1);

        let error = service
            .rollback_transaction_session(&tenant_id, &session.token, &intruder)
            .expect_err("principal mismatch should fail");

        assert!(matches!(error, Error::PermissionDenied(_)));
        assert_eq!(service.active_transaction_session_count(), 0);
        assert!(matches!(
            service.rollback_transaction_session(&tenant_id, &session.token, &owner),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn transaction_session_commit_reports_conflicts_from_tracked_reads() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
        let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
        let table = messages_table("messages_txn_session_conflict");
        let document_id = service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([("body".to_string(), json!("before"))]),
            )
            .expect("fixture insert should succeed");
        let principal = PrincipalContext::anonymous();

        let session = service
            .begin_transaction_session(
                tenant_id.clone(),
                principal.clone(),
                TransactionSessionMode::ReadWrite,
            )
            .expect("transaction session should start");
        service
            .get_document_in_transaction(
                &tenant_id,
                &session.token,
                &principal,
                &table,
                document_id.clone(),
            )
            .expect("transactional read should succeed");

        service
            .update_document(
                &tenant_id,
                table.clone(),
                document_id.clone(),
                serde_json::Map::from_iter([("body".to_string(), json!("outside"))]),
            )
            .expect("outside update should succeed");

        let error = service
            .commit_transaction_session(
                &tenant_id,
                &session.token,
                &principal,
                Some(patch_body_batch(&table, &document_id, "inside")),
            )
            .expect_err("conflicting commit should fail");

        assert!(matches!(error, Error::Conflict(_)));
        assert_eq!(service.active_transaction_session_count(), 0);
        assert_eq!(
            service
                .get_document(&tenant_id, &table, document_id)
                .expect("document should still exist")
                .get_field("body"),
            Some(&json!("outside"))
        );
    }
}
