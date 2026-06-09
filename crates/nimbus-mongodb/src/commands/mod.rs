mod admin;
mod aggregation;
mod collection;
pub(crate) mod crud;
pub(crate) mod cursor;
mod handshake;
mod index;
pub(crate) mod session;
mod tenant;

use std::sync::Arc;

use nimbus_engine::Engine;

use super::AuthConfig;
use super::auth;
use super::connection::ConnectionState;
use super::error::{MongoError, ok_doc};

pub async fn dispatch(
    command_name: &str,
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
    auth: &AuthConfig,
) -> Result<bson::Document, MongoError> {
    session::handle_start_transaction(body, conn, engine)?;

    match command_name {
        "hello" => handshake::hello(body, conn),
        "isMaster" | "ismaster" => handshake::is_master(body, conn),
        "buildInfo" | "buildinfo" => handshake::build_info(),
        "ping" => Ok(ok_doc()),
        "whatsmyuri" => admin::whatsmyuri(conn),
        "getParameter" => admin::get_parameter(body),
        "serverStatus" => admin::server_status(),
        "connectionStatus" => admin::connection_status(conn),
        "getCmdLineOpts" => admin::get_cmd_line_opts(),
        "getFreeMonitoringStatus" => admin::get_free_monitoring_status(),
        "getLog" => admin::get_log(body),
        "saslStart" => auth::sasl_start(body, conn, auth),
        "saslContinue" => auth::sasl_continue(body, conn, auth),
        "insert" => crud::insert(body, conn, engine),
        "find" => crud::find(body, conn, engine),
        "update" => crud::update(body, conn, engine),
        "delete" => crud::delete(body, conn, engine),
        "findAndModify" | "findandmodify" => crud::find_and_modify(body, conn, engine),
        "count" => crud::count(body, engine),
        "distinct" => crud::distinct(body, engine),
        "aggregate" => aggregation::aggregate(body, conn, engine),
        "create" => collection::create(body, engine),
        "drop" => collection::drop_collection(body, engine),
        "listCollections" => collection::list_collections(body, engine),
        "listDatabases" => collection::list_databases(body, engine),
        "createIndexes" | "createindexes" => index::create_indexes(body, engine),
        "dropIndexes" | "dropindexes" => index::drop_indexes(body, engine),
        "listIndexes" | "listindexes" => index::list_indexes(body, engine),
        "getMore" => cursor::get_more(body, conn),
        "killCursors" => cursor::kill_cursors(body, conn),
        "startSession" => session::start_session(body, conn),
        "endSessions" => session::end_sessions(body, conn, engine),
        "refreshSessions" => session::refresh_sessions(body, conn),
        "commitTransaction" => session::commit_transaction(body, conn, engine),
        "abortTransaction" => session::abort_transaction(body, conn, engine),
        _ => Err(MongoError::command_not_found(command_name)),
    }
}

pub fn extract_command_name(doc: &bson::Document) -> Option<String> {
    doc.keys().next().map(|k| k.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_testing::EngineFixture;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    fn test_auth() -> AuthConfig {
        AuthConfig::default()
    }

    #[test]
    fn extract_command_name_from_doc() {
        let doc = bson::doc! { "ping": 1 };
        assert_eq!(extract_command_name(&doc), Some("ping".into()));
    }

    #[test]
    fn extract_command_name_empty_doc() {
        let doc = bson::Document::new();
        assert_eq!(extract_command_name(&doc), None);
    }

    #[tokio::test]
    async fn dispatch_ping_returns_ok() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "ping": 1 };
        let result = dispatch("ping", &doc, &mut test_conn(), &fixture.engine(), &auth)
            .await
            .unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[tokio::test]
    async fn dispatch_unknown_returns_command_not_found() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "foobar": 1 };
        let err = dispatch("foobar", &doc, &mut test_conn(), &fixture.engine(), &auth)
            .await
            .unwrap_err();
        match err {
            MongoError::Command {
                code, code_name, ..
            } => {
                assert_eq!(code, 59);
                assert_eq!(code_name, "CommandNotFound");
            }
            other => panic!("expected Command error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn dispatch_hello_returns_writable_primary() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "hello": 1 };
        let result = dispatch("hello", &doc, &mut test_conn(), &fixture.engine(), &auth)
            .await
            .unwrap();
        assert!(result.get_bool("isWritablePrimary").unwrap());
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[tokio::test]
    async fn dispatch_ismaster_case_insensitive() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let mut conn = test_conn();
        let doc1 = bson::doc! { "isMaster": 1 };
        let doc2 = bson::doc! { "ismaster": 1 };
        let r1 = dispatch("isMaster", &doc1, &mut conn, &fixture.engine(), &auth)
            .await
            .unwrap();
        let r2 = dispatch("ismaster", &doc2, &mut conn, &fixture.engine(), &auth)
            .await
            .unwrap();
        assert!(r1.get_bool("ismaster").unwrap());
        assert!(r2.get_bool("ismaster").unwrap());
    }

    #[tokio::test]
    async fn dispatch_build_info() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "buildInfo": 1 };
        let result = dispatch(
            "buildInfo",
            &doc,
            &mut test_conn(),
            &fixture.engine(),
            &auth,
        )
        .await
        .unwrap();
        assert_eq!(result.get_str("version").unwrap(), "7.0.0");
    }

    #[tokio::test]
    async fn dispatch_sasl_start() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let mut conn = test_conn();
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: b"n,,n=admin,r=nonce123".to_vec() },
        };
        let result = dispatch("saslStart", &body, &mut conn, &fixture.engine(), &auth)
            .await
            .unwrap();
        assert!(!result.get_bool("done").unwrap());
        assert!(conn.scram_state.is_some());
    }
}
