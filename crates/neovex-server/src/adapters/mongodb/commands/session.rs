use std::collections::HashMap;
use std::sync::Arc;

use neovex_core::{
    AtomicWrite, AtomicWriteBatch, PrincipalContext, TenantId, TransactionSessionMode,
    TransactionSessionToken,
};
use neovex_engine::Service;

use super::super::connection::ConnectionState;
use super::super::error::{BAD_VALUE, MongoError, WRITE_CONFLICT};

const NO_SUCH_TRANSACTION: i32 = 251;
const NO_SUCH_TRANSACTION_NAME: &str = "NoSuchTransaction";
const TRANSACTION_COMMITTED: i32 = 256;

pub fn start_session(
    body: &bson::Document,
    conn: &mut ConnectionState,
) -> Result<bson::Document, MongoError> {
    let _ = body;
    let lsid = conn.session_store.create_session();
    Ok(bson::doc! {
        "id": lsid,
        "ok": 1.0,
    })
}

pub fn end_sessions(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let ids = body
        .get_array("endSessions")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "endSessions requires an array of session IDs".into(),
        })?;

    for id_bson in ids {
        if let Some(doc) = id_bson.as_document() {
            if let Some(uuid) = extract_session_uuid(doc) {
                conn.session_store.end_session(&uuid, service);
            }
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
    service: &Arc<Service>,
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
    let tenant_id = session.tenant_id.clone().unwrap_or_else(default_tenant_id);
    let buffered = std::mem::take(&mut session.buffered_writes);

    let batch = if buffered.is_empty() {
        None
    } else {
        Some(AtomicWriteBatch { writes: buffered })
    };

    service
        .commit_transaction_session(&tenant_id, &token, &PrincipalContext::system(), batch)
        .map_err(|e| match e {
            neovex_core::Error::Conflict(_) => MongoError::Command {
                code: WRITE_CONFLICT.code,
                code_name: WRITE_CONFLICT.code_name.into(),
                message: e.to_string(),
            },
            _ => MongoError::from(e),
        })?;

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn abort_transaction(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
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
    session.buffered_writes.clear();
    let tenant_id = session.tenant_id.clone().unwrap_or_else(default_tenant_id);

    let _ = service.rollback_transaction_session(&tenant_id, &token, &PrincipalContext::system());

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn handle_start_transaction(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
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
    let tenant_id = TenantId::new(db_name).map_err(MongoError::from)?;

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

    let txn_session = service.begin_transaction_session(
        tenant_id.clone(),
        PrincipalContext::system(),
        TransactionSessionMode::ReadWrite,
    )?;

    session.transaction_token = Some(txn_session.token);
    session.transaction_started = true;
    session.tenant_id = Some(tenant_id);

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

fn default_tenant_id() -> TenantId {
    TenantId::new("default").expect("default tenant id should be valid")
}

#[derive(Default)]
pub struct SessionStore {
    sessions: HashMap<Vec<u8>, SessionState>,
}

pub struct SessionState {
    pub transaction_token: Option<TransactionSessionToken>,
    pub transaction_started: bool,
    pub tenant_id: Option<TenantId>,
    pub buffered_writes: Vec<AtomicWrite>,
}

impl SessionStore {
    pub fn create_session(&mut self) -> bson::Document {
        let uuid_bytes = generate_uuid_v4();
        self.sessions.insert(
            uuid_bytes.clone(),
            SessionState {
                transaction_token: None,
                transaction_started: false,
                tenant_id: None,
                buffered_writes: Vec::new(),
            },
        );
        bson::doc! {
            "id": bson::Binary {
                subtype: bson::spec::BinarySubtype::Uuid,
                bytes: uuid_bytes,
            }
        }
    }

    pub fn get_session_mut(&mut self, uuid: &[u8]) -> Option<&mut SessionState> {
        self.sessions.get_mut(uuid)
    }

    pub fn end_session(&mut self, uuid: &[u8], service: &Arc<Service>) {
        if let Some(session) = self.sessions.remove(uuid) {
            if let Some(token) = session.transaction_token {
                let tenant_id = session.tenant_id.unwrap_or_else(default_tenant_id);
                let _ = service.rollback_transaction_session(
                    &tenant_id,
                    &token,
                    &PrincipalContext::system(),
                );
            }
        }
    }

    pub fn extract_lsid(body: &bson::Document) -> Option<Vec<u8>> {
        let lsid_doc = body.get_document("lsid").ok()?;
        extract_session_uuid(lsid_doc)
    }

    pub fn buffer_writes_if_in_transaction(
        &mut self,
        body: &bson::Document,
        writes: Vec<AtomicWrite>,
    ) -> Option<()> {
        let lsid = Self::extract_lsid(body)?;
        let session = self.sessions.get_mut(&lsid)?;
        if session.transaction_started && session.transaction_token.is_some() {
            session.buffered_writes.extend(writes);
            Some(())
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

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
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
    fn end_sessions_removes_sessions() {
        let mut conn = test_conn();
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        let r = start_session(&body, &mut conn).unwrap();
        assert_eq!(conn.session_store.session_count(), 1);

        let lsid = r.get_document("id").unwrap().clone();
        let end_body = bson::doc! { "endSessions": [lsid], "$db": "admin" };
        let fixture = neovex_testing::ServiceFixture::new(|path| Service::new(path));
        let result = end_sessions(&end_body, &mut conn, &fixture.service()).unwrap();
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
        let fixture = neovex_testing::ServiceFixture::new(|path| Service::new(path));
        let result = end_sessions(&end_body, &mut conn, &fixture.service()).unwrap();
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

    use neovex_testing::ServiceFixture;

    fn create_session_lsid(conn: &mut ConnectionState) -> bson::Document {
        let body = bson::doc! { "startSession": 1, "$db": "admin" };
        let r = start_session(&body, conn).unwrap();
        r.get_document("id").unwrap().clone()
    }

    fn lsid_field(lsid: &bson::Document) -> bson::Bson {
        bson::Bson::Document(lsid.clone())
    }

    fn setup_tenant(fixture: &ServiceFixture<Service>) {
        let tenant_id = TenantId::new("testdb").expect("tenant id should be valid");
        fixture.service().create_tenant(tenant_id).unwrap_or(());
    }

    #[test]
    fn start_transaction_begins_engine_session() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
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
        handle_start_transaction(&body, &mut conn, &fixture.service()).unwrap();

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(session.transaction_started);
        assert!(session.transaction_token.is_some());
    }

    #[test]
    fn commit_transaction_succeeds() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
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
        handle_start_transaction(&start_body, &mut conn, &fixture.service()).unwrap();

        let commit_body = bson::doc! {
            "commitTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let result = commit_transaction(&commit_body, &mut conn, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
        assert!(session.transaction_token.is_none());
    }

    #[test]
    fn abort_transaction_succeeds() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
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
        handle_start_transaction(&start_body, &mut conn, &fixture.service()).unwrap();

        let abort_body = bson::doc! {
            "abortTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let result = abort_transaction(&abort_body, &mut conn, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
        assert!(session.transaction_token.is_none());
    }

    #[test]
    fn commit_without_transaction_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let body = bson::doc! {
            "commitTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let err = commit_transaction(&body, &mut conn, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, NO_SUCH_TRANSACTION),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn abort_without_transaction_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let body = bson::doc! {
            "abortTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        let err = abort_transaction(&body, &mut conn, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, NO_SUCH_TRANSACTION),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn commit_missing_lsid_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let mut conn = test_conn();
        let body = bson::doc! { "commitTransaction": 1, "$db": "admin" };
        let err = commit_transaction(&body, &mut conn, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn start_transaction_without_lsid_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let mut conn = test_conn();
        let body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "startTransaction": true,
        };
        let err = handle_start_transaction(&body, &mut conn, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn start_transaction_without_flag_is_noop() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let mut conn = test_conn();
        let lsid = create_session_lsid(&mut conn);

        let body = bson::doc! {
            "insert": "users",
            "$db": "testdb",
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&body, &mut conn, &fixture.service()).unwrap();

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(!session.transaction_started);
    }

    #[test]
    fn transaction_buffers_writes_and_flushes_on_commit() {
        use crate::adapters::mongodb::commands::crud;

        let fixture = ServiceFixture::new(|path| Service::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();

        let seed_body = bson::doc! {
            "insert": "txitems",
            "$db": "testdb",
            "documents": [{ "_id": "seed", "val": 0 }],
        };
        crud::insert(&seed_body, &mut conn, &fixture.service()).unwrap();

        let lsid = create_session_lsid(&mut conn);

        let start_body = bson::doc! {
            "insert": "txitems",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&start_body, &mut conn, &fixture.service()).unwrap();

        let insert_body = bson::doc! {
            "insert": "txitems",
            "$db": "testdb",
            "lsid": lsid_field(&lsid),
            "documents": [{ "_id": "tx1", "val": 42 }],
        };
        let result = crud::insert(&insert_body, &mut conn, &fixture.service()).unwrap();
        assert_eq!(result.get_i32("n").unwrap(), 1);

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert_eq!(session.buffered_writes.len(), 1);

        let commit_body = bson::doc! {
            "commitTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        commit_transaction(&commit_body, &mut conn, &fixture.service()).unwrap();

        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(session.buffered_writes.is_empty());

        let find_body = bson::doc! { "find": "txitems", "$db": "testdb" };
        let found = crud::find(&find_body, &mut conn, &fixture.service()).unwrap();
        let cursor = found.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn transaction_abort_discards_buffered_writes() {
        use crate::adapters::mongodb::commands::crud;

        let fixture = ServiceFixture::new(|path| Service::new(path));
        setup_tenant(&fixture);
        let mut conn = test_conn();

        let seed_body = bson::doc! {
            "insert": "abortitems",
            "$db": "testdb",
            "documents": [{ "_id": "seed", "val": 0 }],
        };
        crud::insert(&seed_body, &mut conn, &fixture.service()).unwrap();

        let lsid = create_session_lsid(&mut conn);

        let start_body = bson::doc! {
            "insert": "abortitems",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": lsid_field(&lsid),
            "documents": [],
        };
        handle_start_transaction(&start_body, &mut conn, &fixture.service()).unwrap();

        let insert_body = bson::doc! {
            "insert": "abortitems",
            "$db": "testdb",
            "lsid": lsid_field(&lsid),
            "documents": [{ "_id": "a1", "val": 99 }],
        };
        crud::insert(&insert_body, &mut conn, &fixture.service()).unwrap();

        let uuid = extract_uuid_bytes(&lsid).to_vec();
        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert_eq!(session.buffered_writes.len(), 1);

        let abort_body = bson::doc! {
            "abortTransaction": 1,
            "$db": "admin",
            "lsid": lsid_field(&lsid),
        };
        abort_transaction(&abort_body, &mut conn, &fixture.service()).unwrap();

        let session = conn.session_store.get_session_mut(&uuid).unwrap();
        assert!(session.buffered_writes.is_empty());
    }

    #[test]
    fn end_session_aborts_active_transaction() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
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
        handle_start_transaction(&start_body, &mut conn, &fixture.service()).unwrap();

        let end_body = bson::doc! { "endSessions": [lsid], "$db": "admin" };
        end_sessions(&end_body, &mut conn, &fixture.service()).unwrap();
        assert_eq!(conn.session_store.session_count(), 0);
    }
}
