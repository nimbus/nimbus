use std::collections::HashMap;
use std::sync::Arc;

use nimbus_core::{PrincipalContext, TransactionSessionMode, TransactionSessionToken};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use super::super::connection::ConnectionState;
use super::super::error::{BAD_VALUE, MongoError, TOO_MANY_LOGICAL_SESSIONS, WRITE_CONFLICT};
use super::tenant::{default_tenant_context, resolve_tenant_context};

const NO_SUCH_TRANSACTION: i32 = 251;
const NO_SUCH_TRANSACTION_NAME: &str = "NoSuchTransaction";
const TRANSACTION_COMMITTED: i32 = 256;
pub(crate) const MAX_SESSIONS_PER_CONNECTION: usize = 128;

pub fn start_session(
    body: &bson::Document,
    conn: &mut ConnectionState,
) -> Result<bson::Document, MongoError> {
    let _ = body;
    let lsid = conn.session_store.create_session()?;
    Ok(bson::doc! {
        "id": lsid,
        "ok": 1.0,
    })
}

pub fn end_sessions(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
    principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let ids = body
        .get_array("endSessions")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "endSessions requires an array of session IDs".into(),
        })?;

    for id_bson in ids {
        if let Some(doc) = id_bson.as_document()
            && let Some(uuid) = extract_session_uuid(doc)
        {
            conn.session_store.end_session(&uuid, engine, principal);
        }
    }

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn refresh_sessions(
    body: &bson::Document,
    conn: &mut ConnectionState,
) -> Result<bson::Document, MongoError> {
    let ids = body
        .get_array("refreshSessions")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "refreshSessions requires an array of session IDs".into(),
        })?;
    let _ = ids;
    let _ = conn;
    Ok(bson::doc! { "ok": 1.0 })
}

pub fn commit_transaction(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
    principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let lsid = SessionStore::extract_lsid(body).ok_or_else(|| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "commitTransaction requires lsid".into(),
    })?;

    let session = conn
        .session_store
        .get_session_mut(&lsid)
        .ok_or_else(no_such_transaction)?;

    let token = session
        .transaction_token
        .take()
        .ok_or_else(no_such_transaction)?;
    session.transaction_started = false;
    let tenant_context = session
        .tenant_context
        .clone()
        .unwrap_or_else(|| default_tenant_context("mongodb transaction commit", principal));
    let tenant_id = tenant_context.tenant_id().clone();
    engine
        .commit_transaction_session(&tenant_id, &token, principal, None)
        .map_err(|e| match e {
            nimbus_core::Error::Conflict(_) | nimbus_core::Error::PreconditionFailed(_) => {
                MongoError::Command {
                    code: WRITE_CONFLICT.code,
                    code_name: WRITE_CONFLICT.code_name.into(),
                    message: e.to_string(),
                }
            }
            _ => MongoError::from(e),
        })?;

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn abort_transaction(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
    principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let lsid = SessionStore::extract_lsid(body).ok_or_else(|| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "abortTransaction requires lsid".into(),
    })?;

    let session = conn
        .session_store
        .get_session_mut(&lsid)
        .ok_or_else(no_such_transaction)?;

    let token = session
        .transaction_token
        .take()
        .ok_or_else(no_such_transaction)?;
    session.transaction_started = false;
    let tenant_context = session
        .tenant_context
        .clone()
        .unwrap_or_else(|| default_tenant_context("mongodb transaction abort", principal));
    let tenant_id = tenant_context.tenant_id().clone();

    let _ = engine.rollback_transaction_session(&tenant_id, &token, principal);

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn handle_start_transaction(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
    principal: &PrincipalContext,
) -> Result<(), MongoError> {
    if !body.get_bool("startTransaction").unwrap_or(false) {
        return Ok(());
    }

    let lsid = SessionStore::extract_lsid(body).ok_or_else(|| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "startTransaction requires lsid".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or("default");
    let tenant_context = resolve_tenant_context(db_name, "mongodb transaction start", principal)?;
    let tenant_id = tenant_context.tenant_id().clone();

    let session = conn
        .session_store
        .get_session_mut(&lsid)
        .ok_or_else(no_such_transaction)?;

    if session.transaction_started {
        return Err(MongoError::Command {
            code: TRANSACTION_COMMITTED,
            code_name: "TransactionCommitted".into(),
            message: "transaction already in progress".into(),
        });
    }

    let txn_session = engine.begin_transaction_session(
        tenant_id.clone(),
        principal.clone(),
        TransactionSessionMode::ReadWrite,
    )?;

    session.transaction_token = Some(txn_session.token);
    session.transaction_started = true;
    session.tenant_context = Some(tenant_context);

    Ok(())
}

fn extract_session_uuid(doc: &bson::Document) -> Option<Vec<u8>> {
    match doc.get("id")? {
        bson::Bson::Binary(bin) => Some(bin.bytes.clone()),
        _ => None,
    }
}

fn no_such_transaction() -> MongoError {
    MongoError::Command {
        code: NO_SUCH_TRANSACTION,
        code_name: NO_SUCH_TRANSACTION_NAME.into(),
        message: "no transaction is in progress".into(),
    }
}

#[derive(Default)]
pub struct SessionStore {
    sessions: HashMap<Vec<u8>, SessionState>,
}

pub struct SessionState {
    pub transaction_token: Option<TransactionSessionToken>,
    pub transaction_started: bool,
    pub tenant_context: Option<TenantIsolationContext>,
}

impl SessionStore {
    pub fn create_session(&mut self) -> Result<bson::Document, MongoError> {
        if self.sessions.len() >= MAX_SESSIONS_PER_CONNECTION {
            return Err(MongoError::Command {
                code: TOO_MANY_LOGICAL_SESSIONS.code,
                code_name: TOO_MANY_LOGICAL_SESSIONS.code_name.into(),
                message: format!(
                    "too many logical sessions on this MongoDB connection; limit is {MAX_SESSIONS_PER_CONNECTION}"
                ),
            });
        }
        let uuid_bytes = generate_uuid_v4();
        self.sessions.insert(
            uuid_bytes.clone(),
            SessionState {
                transaction_token: None,
                transaction_started: false,
                tenant_context: None,
            },
        );
        Ok(bson::doc! {
            "id": bson::Binary {
                subtype: bson::spec::BinarySubtype::Uuid,
                bytes: uuid_bytes,
            }
        })
    }

    pub fn get_session_mut(&mut self, uuid: &[u8]) -> Option<&mut SessionState> {
        self.sessions.get_mut(uuid)
    }

    pub fn end_session(&mut self, uuid: &[u8], engine: &Arc<Engine>, principal: &PrincipalContext) {
        if let Some(session) = self.sessions.remove(uuid)
            && let Some(token) = session.transaction_token
        {
            let tenant_context = session.tenant_context.unwrap_or_else(|| {
                default_tenant_context("mongodb end session rollback", principal)
            });
            let tenant_id = tenant_context.tenant_id().clone();
            let _ = engine.rollback_transaction_session(&tenant_id, &token, principal);
        }
    }

    pub fn extract_lsid(body: &bson::Document) -> Option<Vec<u8>> {
        let lsid_doc = body.get_document("lsid").ok()?;
        extract_session_uuid(lsid_doc)
    }

    pub fn active_transaction_token(
        &self,
        body: &bson::Document,
    ) -> Option<TransactionSessionToken> {
        let lsid = Self::extract_lsid(body)?;
        let session = self.sessions.get(&lsid)?;
        if session.transaction_started && session.transaction_token.is_some() {
            session.transaction_token.clone()
        } else {
            None
        }
    }

    #[cfg(test)]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

fn generate_uuid_v4() -> Vec<u8> {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes).expect("system RNG should not fail");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes.to_vec()
}

#[cfg(test)]
mod tests {
    use super::super::super::connection::ConnectionState;
    use super::*;
    use nimbus_core::TenantId;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    fn test_principal() -> PrincipalContext {
        PrincipalContext::system()
    }

    fn extract_uuid_bytes(lsid_doc: &bson::Document) -> &[u8] {
        match lsid_doc.get("id").unwrap() {
            bson::Bson::Binary(bin) => &bin.bytes,
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn start_session_returns_lsid() {
        let mut conn = test_conn();
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        let result = start_session(&body, &mut conn).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        let id = result.get_document("id").unwrap();
        let bin = extract_uuid_bytes(id);
        assert_eq!(bin.len(), 16);
        assert_eq!(bin[6] >> 4, 4);
        assert_eq!(bin[8] >> 6, 2);
    }

    #[test]
    fn start_session_creates_unique_ids() {
        let mut conn = test_conn();
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        let r1 = start_session(&body, &mut conn).unwrap();
        let r2 = start_session(&body, &mut conn).unwrap();
        let id1 = extract_uuid_bytes(r1.get_document("id").unwrap());
        let id2 = extract_uuid_bytes(r2.get_document("id").unwrap());
        assert_ne!(id1, id2);
        assert_eq!(conn.session_store.session_count(), 2);
    }

    #[test]
    fn start_session_rejects_new_session_at_connection_cap() {
        let mut conn = test_conn();
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        for _ in 0..MAX_SESSIONS_PER_CONNECTION {
            start_session(&body, &mut conn).expect("session under cap should start");
        }

        let error =
            start_session(&body, &mut conn).expect_err("session over cap should be rejected");

        match error {
            MongoError::Command {
                code,
                code_name,
                message,
            } => {
                assert_eq!(code, TOO_MANY_LOGICAL_SESSIONS.code);
                assert_eq!(code_name, TOO_MANY_LOGICAL_SESSIONS.code_name);
                assert!(message.contains("too many logical sessions"));
            }
            other => panic!("expected Command, got {other:?}"),
        }
        assert_eq!(
            conn.session_store.session_count(),
            MAX_SESSIONS_PER_CONNECTION
        );
    }

    #[test]
    fn end_sessions_removes_sessions() {
        let mut conn = test_conn();
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        let r = start_session(&body, &mut conn).unwrap();
        assert_eq!(conn.session_store.session_count(), 1);

        let lsid = r.get_document("id").unwrap().clone();
        let end_body = bson::doc! { "endSessions": [lsid], "$db": "admin" };
        let fixture = nimbus_testing::EngineFixture::new(|path| Engine::new(path));
        let result =
            end_sessions(&end_body, &mut conn, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        assert_eq!(conn.session_store.session_count(), 0);
    }

    #[test]
    fn end_sessions_ignores_unknown_ids() {
        let mut conn = test_conn();
        let fake_lsid = bson::doc! {
            "id": bson::Binary {
                subtype: bson::spec::BinarySubtype::Uuid,
                bytes: vec![0u8; 16],
            }
        };
        let end_body = bson::doc! { "endSessions": [fake_lsid], "$db": "admin" };
        let fixture = nimbus_testing::EngineFixture::new(|path| Engine::new(path));
        let result =
            end_sessions(&end_body, &mut conn, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn extract_lsid_from_command_body() {
        let uuid_bytes = generate_uuid_v4();
        let body = bson::doc! {
            "find": "users",
            "$db": "testdb",
            "lsid": {
                "id": bson::Binary {
                    subtype: bson::spec::BinarySubtype::Uuid,
                    bytes: uuid_bytes.clone(),
                }
            }
        };
        let extracted = SessionStore::extract_lsid(&body);
        assert_eq!(extracted.unwrap(), uuid_bytes);
    }

    #[test]
    fn extract_lsid_returns_none_when_missing() {
        let body = bson::doc! { "find": "users", "$db": "testdb" };
        assert!(SessionStore::extract_lsid(&body).is_none());
    }

    #[test]
    fn refresh_sessions_returns_ok() {
        let mut conn = test_conn();
        let body = bson::doc! {
            "refreshSessions": [{
                "id": bson::Binary {
                    subtype: bson::spec::BinarySubtype::Uuid,
                    bytes: vec![0u8; 16],
                }
            }],
            "$db": "admin"
        };
        let result = refresh_sessions(&body, &mut conn).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn session_store_get_session_mut() {
        let mut conn = test_conn();
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        let r = start_session(&body, &mut conn).unwrap();
        let id_doc = r.get_document("id").unwrap();
        let uuid = extract_uuid_bytes(id_doc).to_vec();

        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(session.transaction_token.is_none());
        assert!(!session.transaction_started);
    }

    use nimbus_testing::EngineFixture;

    fn create_session_lsid(conn: &mut ConnectionState) -> bson::Document {
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        let r = start_session(&body, conn).unwrap();
        r.get_document("id").unwrap().clone()
    }

    fn lsid_field(lsid: &bson::Document) -> bson::Bson {
        bson::Bson::Document(lsid.clone())
    }

    fn setup_tenant(fixture: &EngineFixture<Engine>) {
        let tenant_id = TenantId::new("testdb").expect("tenant id should be valid");
        fixture.engine().create_tenant(tenant_id).unwrap_or(());
    }

    #[test]
    fn start_transaction_begins_engine_session() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&body, &mut conn, &fixture.engine(), &test_principal()).unwrap();

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(session.transaction_started);
        assert!(session.transaction_token.is_some());
    }

    #[test]
    fn commit_transaction_succeeds() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let start_body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&start_body, &mut conn, &fixture.engine(), &test_principal())
            .unwrap();

        let commit_body = bson::doc! {
            "commitTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let result = commit_transaction(
            &commit_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
        assert!(session.transaction_token.is_none());
    }

    #[test]
    fn abort_transaction_succeeds() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let start_body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&start_body, &mut conn, &fixture.engine(), &test_principal())
            .unwrap();

        let abort_body = bson::doc! {
            "abortTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let result =
            abort_transaction(&abort_body, &mut conn, &fixture.engine(), &test_principal())
                .unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
        assert!(session.transaction_token.is_none());
    }

    #[test]
    fn commit_without_transaction_returns_error() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let body = bson::doc! {
            "commitTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let err =
            commit_transaction(&body, &mut conn, &fixture.engine(), &test_principal()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, NO_SUCH_TRANSACTION),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn abort_without_transaction_returns_error() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let body = bson::doc! {
            "abortTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let err =
            abort_transaction(&body, &mut conn, &fixture.engine(), &test_principal()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, NO_SUCH_TRANSACTION),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn commit_missing_lsid_returns_error() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut conn = test_conn();
        let body = bson::doc! { "commitTransaction": 1, "$db": "admin" };
        let err =
            commit_transaction(&body, &mut conn, &fixture.engine(), &test_principal()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn start_transaction_without_lsid_returns_error() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut conn = test_conn();
        let body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "startTransaction": true,
        };
        let err = handle_start_transaction(&body, &mut conn, &fixture.engine(), &test_principal())
            .unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn start_transaction_without_flag_is_noop() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&body, &mut conn, &fixture.engine(), &test_principal()).unwrap();

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
    }

    #[test]
    fn transaction_stages_writes_in_engine_session_and_flushes_on_commit() {
        use crate::commands::crud;

        let fixture = EngineFixture::new(|path| Engine::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();

        let seed_body = bson::doc! {
            "insert": "txitems",
            "$db": "testdb",
            "documents": [{ "_id": "seed", "val": 0 }],
        };
        crud::insert(&seed_body, &mut conn, &fixture.engine(), &test_principal()).unwrap();

        let lsid = create_session_lsid(&mut conn);

        let start_body = bson::doc! {
            "insert": "txitems",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&start_body, &mut conn, &fixture.engine(), &test_principal())
            .unwrap();

        let insert_body = bson::doc! {
            "insert": "txitems",
            "$db": "testdb",
            "lsid": lsid_field(&lsid),
            "documents": [{ "_id": "tx1", "val": 42 }],
        };
        let result = crud::insert(
            &insert_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        assert_eq!(result.get_i32("n").unwrap(), 1);

        let transaction_find_body =
            bson::doc! { "find": "txitems", "$db": "testdb", "lsid": lsid_field(&lsid) };
        let transaction_found = crud::find(
            &transaction_find_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        let transaction_batch = transaction_found
            .get_document("cursor")
            .unwrap()
            .get_array("firstBatch")
            .unwrap();
        assert_eq!(
            transaction_batch.len(),
            2,
            "transaction reads should include staged writes"
        );

        let outside_find_body = bson::doc! { "find": "txitems", "$db": "testdb" };
        let outside_before_commit = crud::find(
            &outside_find_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        let outside_before_batch = outside_before_commit
            .get_document("cursor")
            .unwrap()
            .get_array("firstBatch")
            .unwrap();
        assert_eq!(
            outside_before_batch.len(),
            1,
            "outside reads must not see uncommitted transaction writes"
        );

        let commit_body = bson::doc! {
            "commitTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        commit_transaction(
            &commit_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
        assert!(session.transaction_token.is_none());

        let found = crud::find(
            &outside_find_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        let cursor = found.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn transaction_abort_discards_engine_staged_writes() {
        use crate::commands::crud;

        let fixture = EngineFixture::new(|path| Engine::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();

        let seed_body = bson::doc! {
            "insert": "abortitems",
            "$db": "testdb",
            "documents": [{ "_id": "seed", "val": 0 }],
        };
        crud::insert(&seed_body, &mut conn, &fixture.engine(), &test_principal()).unwrap();

        let lsid = create_session_lsid(&mut conn);

        let start_body = bson::doc! {
            "insert": "abortitems",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&start_body, &mut conn, &fixture.engine(), &test_principal())
            .unwrap();

        let insert_body = bson::doc! {
            "insert": "abortitems",
            "$db": "testdb",
            "lsid": lsid_field(&lsid),
            "documents": [{ "_id": "a1", "val": 99 }],
        };
        crud::insert(
            &insert_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();

        let transaction_find_body =
            bson::doc! { "find": "abortitems", "$db": "testdb", "lsid": lsid_field(&lsid) };
        let transaction_found = crud::find(
            &transaction_find_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        let transaction_batch = transaction_found
            .get_document("cursor")
            .unwrap()
            .get_array("firstBatch")
            .unwrap();
        assert_eq!(transaction_batch.len(), 2);

        let outside_find_body = bson::doc! { "find": "abortitems", "$db": "testdb" };
        let outside_before_abort = crud::find(
            &outside_find_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        assert_eq!(
            outside_before_abort
                .get_document("cursor")
                .unwrap()
                .get_array("firstBatch")
                .unwrap()
                .len(),
            1
        );

        let abort_body = bson::doc! {
            "abortTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        abort_transaction(&abort_body, &mut conn, &fixture.engine(), &test_principal()).unwrap();

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
        assert!(session.transaction_token.is_none());
        let outside_after_abort = crud::find(
            &outside_find_body,
            &mut conn,
            &fixture.engine(),
            &test_principal(),
        )
        .unwrap();
        assert_eq!(
            outside_after_abort
                .get_document("cursor")
                .unwrap()
                .get_array("firstBatch")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn end_session_aborts_active_transaction() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let start_body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&start_body, &mut conn, &fixture.engine(), &test_principal())
            .unwrap();

        let end_body = bson::doc! { "endSessions": [lsid], "$db": "admin" };
        end_sessions(&end_body, &mut conn, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(conn.session_store.session_count(), 0);
    }
}
