mod admin;
mod aggregation;
pub(crate) mod change_stream;
mod collection;
pub(crate) mod crud;
pub(crate) mod cursor;
mod handshake;
mod index;
pub(crate) mod session;
mod tenant;

use std::sync::Arc;

use nimbus_engine::Service;

use super::AuthConfig;
use super::auth;
use super::connection::ConnectionState;
use super::error::{MongoError, ok_doc};

pub async fn dispatch(
    command_name: &str,
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    auth: &AuthConfig,
) -> Result<bson::Document, MongoError> {
    session::handle_start_transaction(body, conn, service)?;

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
        "insert" => crud::insert(body, conn, service),
        "find" => crud::find(body, conn, service),
        "update" => crud::update(body, conn, service),
        "delete" => crud::delete(body, conn, service),
        "findAndModify" | "findandmodify" => crud::find_and_modify(body, conn, service),
        "count" => crud::count(body, service),
        "distinct" => crud::distinct(body, service),
        "aggregate" => aggregation::aggregate(body, conn, service),
        "create" => collection::create(body, service),
        "drop" => collection::drop_collection(body, service),
        "listCollections" => collection::list_collections(body, service),
        "listDatabases" => collection::list_databases(body, service),
        "createIndexes" | "createindexes" => index::create_indexes(body, service),
        "dropIndexes" | "dropindexes" => index::drop_indexes(body, service),
        "listIndexes" | "listindexes" => index::list_indexes(body, service),
        "getMore" => get_more_with_change_stream(body, conn).await,
        "killCursors" => kill_cursors_with_change_stream(body, conn),
        "startSession" => session::start_session(body, conn),
        "endSessions" => session::end_sessions(body, conn, service),
        "refreshSessions" => session::refresh_sessions(body, conn),
        "commitTransaction" => session::commit_transaction(body, conn, service),
        "abortTransaction" => session::abort_transaction(body, conn, service),
        _ => Err(MongoError::command_not_found(command_name)),
    }
}

async fn get_more_with_change_stream(
    body: &bson::Document,
    conn: &mut ConnectionState,
) -> Result<bson::Document, MongoError> {
    let cursor_id = body.get_i64("getMore").map_err(|_| MongoError::Command {
        code: super::error::BAD_VALUE.code,
        code_name: super::error::BAD_VALUE.code_name.into(),
        message: "missing cursor id in getMore command".into(),
    })?;

    if conn.change_stream_store.contains(cursor_id) {
        let max_await_ms = match body.get("maxAwaitTimeMS") {
            Some(bson::Bson::Int32(n)) => Some(*n as u64),
            Some(bson::Bson::Int64(n)) => Some(*n as u64),
            _ => None,
        };
        let timeout = std::time::Duration::from_millis(max_await_ms.unwrap_or(1000));

        let cursor = conn
            .change_stream_store
            .get_mut(cursor_id)
            .expect("cursor existence was checked");

        let mut events = change_stream::collect_change_events(cursor);

        if events.is_empty() {
            match tokio::time::timeout(timeout, cursor.receiver.recv()).await {
                Ok(Some(nimbus_engine::SubscriptionUpdate::Result { snapshot, .. })) => {
                    let ns = cursor.ns.clone();
                    let new_events = change_stream::snapshot_to_change_events_pub(
                        &ns,
                        cursor.last_snapshot.as_ref(),
                        &snapshot,
                    );
                    events.extend(new_events);
                    cursor.last_snapshot = Some(snapshot);

                    if let Some(ref token) = cursor.resume_after {
                        events = change_stream::filter_events_after_resume(events, token);
                        if !events.is_empty() {
                            cursor.resume_after = None;
                        }
                    }
                }
                Ok(Some(nimbus_engine::SubscriptionUpdate::Error { .. })) => {}
                Ok(None) => {
                    conn.change_stream_store.remove(cursor_id);
                    return Ok(bson::doc! {
                        "cursor": {
                            "nextBatch": Vec::<bson::Bson>::new(),
                            "id": 0_i64,
                            "ns": "",
                        },
                        "ok": 1.0,
                    });
                }
                Err(_) => {}
            }
        }

        let ns = conn
            .change_stream_store
            .get_mut(cursor_id)
            .map(|c| c.ns.clone())
            .unwrap_or_default();

        let next_batch: Vec<bson::Bson> = events.into_iter().map(bson::Bson::Document).collect();

        Ok(bson::doc! {
            "cursor": {
                "nextBatch": next_batch,
                "id": cursor_id,
                "ns": &ns,
            },
            "ok": 1.0,
        })
    } else {
        cursor::get_more(body, conn)
    }
}

fn kill_cursors_with_change_stream(
    body: &bson::Document,
    conn: &mut ConnectionState,
) -> Result<bson::Document, MongoError> {
    if let Ok(cursor_ids) = body.get_array("cursors") {
        for id_bson in cursor_ids {
            if let Some(id) = id_bson.as_i64() {
                conn.change_stream_store.remove(id);
            }
        }
    }
    cursor::kill_cursors(body, conn)
}

pub fn extract_command_name(doc: &bson::Document) -> Option<String> {
    doc.keys().next().map(|k| k.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_testing::ServiceFixture;

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
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "ping": 1 };
        let result = dispatch("ping", &doc, &mut test_conn(), &fixture.service(), &auth)
            .await
            .unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[tokio::test]
    async fn dispatch_unknown_returns_command_not_found() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "foobar": 1 };
        let err = dispatch("foobar", &doc, &mut test_conn(), &fixture.service(), &auth)
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
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "hello": 1 };
        let result = dispatch("hello", &doc, &mut test_conn(), &fixture.service(), &auth)
            .await
            .unwrap();
        assert!(result.get_bool("isWritablePrimary").unwrap());
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[tokio::test]
    async fn dispatch_ismaster_case_insensitive() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let auth = test_auth();
        let mut conn = test_conn();
        let doc1 = bson::doc! { "isMaster": 1 };
        let doc2 = bson::doc! { "ismaster": 1 };
        let r1 = dispatch("isMaster", &doc1, &mut conn, &fixture.service(), &auth)
            .await
            .unwrap();
        let r2 = dispatch("ismaster", &doc2, &mut conn, &fixture.service(), &auth)
            .await
            .unwrap();
        assert!(r1.get_bool("ismaster").unwrap());
        assert!(r2.get_bool("ismaster").unwrap());
    }

    #[tokio::test]
    async fn dispatch_build_info() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "buildInfo": 1 };
        let result = dispatch(
            "buildInfo",
            &doc,
            &mut test_conn(),
            &fixture.service(),
            &auth,
        )
        .await
        .unwrap();
        assert_eq!(result.get_str("version").unwrap(), "7.0.0");
    }

    #[tokio::test]
    async fn dispatch_sasl_start() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let auth = test_auth();
        let mut conn = test_conn();
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: b"n,,n=admin,r=nonce123".to_vec() },
        };
        let result = dispatch("saslStart", &body, &mut conn, &fixture.service(), &auth)
            .await
            .unwrap();
        assert!(!result.get_bool("done").unwrap());
        assert!(conn.scram_state.is_some());
    }
}
