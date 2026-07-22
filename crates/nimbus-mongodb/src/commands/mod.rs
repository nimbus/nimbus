mod admin;
mod aggregation;
mod collection;
pub(crate) mod crud;
pub(crate) mod cursor;
mod handshake;
mod index;
pub(crate) mod session;
pub(crate) mod tenant;

use std::sync::Arc;

use nimbus_core::PrincipalContext;
use nimbus_engine::Engine;

use super::AuthConfig;
use super::auth;
use super::connection::ConnectionState;
use super::credential_registry::MongoAuth;
use super::error::{MongoError, UNAUTHORIZED, ok_doc};

/// Dispatch a command authenticated by the single tenant-agnostic credential.
///
/// Thin wrapper over [`dispatch_authed`] in unbound mode, preserving the
/// `&AuthConfig` signature `nimbus-server` calls. Bound (per-tenant) callers use
/// [`dispatch_authed`] with [`MongoAuth::Bound`] directly.
pub async fn dispatch(
    command_name: &str,
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
    auth: &AuthConfig,
) -> Result<bson::Document, MongoError> {
    dispatch_authed(command_name, body, conn, engine, &MongoAuth::Unbound(auth)).await
}

/// Dispatch a command under an explicit auth mode.
///
/// In [`MongoAuth::Bound`] mode authentication decides the tenant: a successful
/// SCRAM handshake fixes the connection's tenant, and tenant resolution refuses
/// any wire `$db` naming a different tenant.
pub async fn dispatch_authed(
    command_name: &str,
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
    auth: &MongoAuth<'_>,
) -> Result<bson::Document, MongoError> {
    let principal = if requires_authentication(command_name) {
        Some(authenticated_principal(conn)?)
    } else {
        None
    };
    // Fail-closed bound-mode invariant. A bound (per-tenant) connection that has
    // authenticated MUST carry the tenant authentication resolved; if it ever does
    // not, refuse here — never let tenant resolution fall through to the wire `$db`
    // (the M9 hole this change closes). In correct operation this never fires: a
    // bound SCRAM success always binds a tenant (an unknown username never
    // authenticates). This converts the bound invariant from "always set" into
    // "enforced regardless of path", so the `$db`-deriving branch of
    // `resolve_tenant_context` is unreachable while bound — even if a future code
    // path were to authenticate in bound mode without binding a tenant.
    if auth.is_tenant_bound() && conn.authenticated && conn.authenticated_tenant.is_none() {
        return Err(MongoError::Command {
            code: UNAUTHORIZED.code,
            code_name: UNAUTHORIZED.code_name.into(),
            message: "bound-mode connection authenticated without a tenant binding; refusing to \
                      resolve the tenant from the wire $db"
                .into(),
        });
    }
    if body.get_bool("startTransaction").unwrap_or(false) && principal.is_none() {
        return Err(unauthorized(command_name));
    }
    if let Some(principal) = principal.as_ref() {
        let db_name = body.get_str("$db").unwrap_or(tenant::DEFAULT_TENANT);
        let tenant_context =
            tenant::resolve_tenant_context(db_name, "mongodb command admission", principal)?;
        tenant::ensure_tenant_async(engine, &tenant_context).await?;
        session::handle_start_transaction(body, conn, engine, principal)?;
    }

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
        "insert" => crud::insert(body, conn, engine, required_principal(principal.as_ref())?),
        "find" => crud::find(body, conn, engine, required_principal(principal.as_ref())?),
        "update" => crud::update(body, conn, engine, required_principal(principal.as_ref())?),
        "delete" => crud::delete(body, conn, engine, required_principal(principal.as_ref())?),
        "findAndModify" | "findandmodify" => {
            crud::find_and_modify(body, conn, engine, required_principal(principal.as_ref())?)
        }
        "count" => crud::count(body, engine, required_principal(principal.as_ref())?),
        "distinct" => crud::distinct(body, engine, required_principal(principal.as_ref())?),
        "aggregate" => {
            aggregation::aggregate(body, conn, engine, required_principal(principal.as_ref())?)
        }
        "create" => collection::create(body, engine, required_principal(principal.as_ref())?),
        "drop" => {
            collection::drop_collection(body, engine, required_principal(principal.as_ref())?)
        }
        "listCollections" => {
            collection::list_collections(body, engine, required_principal(principal.as_ref())?)
        }
        "listDatabases" => {
            collection::list_databases(body, engine, required_principal(principal.as_ref())?)
        }
        "createIndexes" | "createindexes" => {
            index::create_indexes(body, engine, required_principal(principal.as_ref())?)
        }
        "dropIndexes" | "dropindexes" => {
            index::drop_indexes(body, engine, required_principal(principal.as_ref())?)
        }
        "listIndexes" | "listindexes" => {
            index::list_indexes(body, engine, required_principal(principal.as_ref())?)
        }
        "getMore" => cursor::get_more(body, conn),
        "killCursors" => cursor::kill_cursors(body, conn),
        "startSession" => session::start_session(body, conn),
        "endSessions" => {
            session::end_sessions(body, conn, engine, required_principal(principal.as_ref())?)
        }
        "refreshSessions" => session::refresh_sessions(body, conn),
        "commitTransaction" => {
            session::commit_transaction(body, conn, engine, required_principal(principal.as_ref())?)
        }
        "abortTransaction" => {
            session::abort_transaction(body, conn, engine, required_principal(principal.as_ref())?)
        }
        _ => Err(MongoError::command_not_found(command_name)),
    }
}

fn requires_authentication(command_name: &str) -> bool {
    !matches!(
        command_name,
        "hello"
            | "isMaster"
            | "ismaster"
            | "buildInfo"
            | "buildinfo"
            | "ping"
            | "whatsmyuri"
            | "getParameter"
            | "serverStatus"
            | "connectionStatus"
            | "getCmdLineOpts"
            | "getFreeMonitoringStatus"
            | "getLog"
            | "saslStart"
            | "saslContinue"
    )
}

fn authenticated_principal(conn: &ConnectionState) -> Result<PrincipalContext, MongoError> {
    conn.authenticated_principal()
        .ok_or_else(|| unauthorized("command"))
}

fn required_principal(
    principal: Option<&PrincipalContext>,
) -> Result<&PrincipalContext, MongoError> {
    principal.ok_or_else(|| unauthorized("command"))
}

fn unauthorized(command_name: &str) -> MongoError {
    MongoError::Command {
        code: UNAUTHORIZED.code,
        code_name: UNAUTHORIZED.code_name.into(),
        message: format!("command '{command_name}' requires authentication"),
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
        AuthConfig::new("admin".into(), "admin".into())
    }

    fn authenticated_conn() -> ConnectionState {
        let mut conn = test_conn();
        conn.authenticated = true;
        conn.auth_user = Some("admin".to_string());
        conn
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
    async fn bound_mode_refuses_authenticated_connection_without_a_tenant_binding() {
        // Fail-closed guard (defense-in-depth on the bound invariant): even if a
        // connection reaches bound-mode dispatch authenticated but WITHOUT an
        // authenticated tenant — an invariant violation that a real bound SCRAM
        // handshake never produces (unknown usernames never authenticate) — tenant
        // resolution must NOT fall through to the wire `$db`. The command is refused.
        use crate::credential_registry::{CredentialRegistry, MongoAuth};
        use nimbus_core::TenantId;

        let registry = CredentialRegistry::new().bind(
            "user-a",
            TenantId::new("tenant-a").unwrap(),
            "secret-a",
        );
        let auth = MongoAuth::Bound(&registry);
        let fixture = EngineFixture::new(|path| Engine::new(path));

        // The invariant-violation state a buggy future auth path could leave:
        // authenticated in bound mode, but no tenant bound.
        let mut conn = test_conn();
        conn.authenticated = true;
        conn.auth_user = Some("user-a".to_string());
        conn.authenticated_tenant = None;

        let doc = bson::doc! { "find": "users", "$db": "tenant-a", "filter": {} };
        let err = dispatch_authed("find", &doc, &mut conn, &fixture.engine(), &auth)
            .await
            .expect_err(
                "bound + authenticated + no tenant must be refused, never resolved from $db",
            );
        match err {
            MongoError::Command { code, message, .. } => {
                assert_eq!(code, UNAUTHORIZED.code);
                assert!(
                    message.contains("without a tenant binding"),
                    "refusal must name the fail-closed reason: {message}"
                );
            }
            other => panic!("expected Command error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_unknown_returns_command_not_found() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let doc = bson::doc! { "foobar": 1 };
        let err = dispatch(
            "foobar",
            &doc,
            &mut authenticated_conn(),
            &fixture.engine(),
            &auth,
        )
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

    #[tokio::test]
    async fn dispatch_rejects_data_command_before_authentication() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let auth = test_auth();
        let body = bson::doc! {
            "find": "users",
            "$db": "testdb",
            "filter": {},
        };

        let err = dispatch("find", &body, &mut test_conn(), &fixture.engine(), &auth)
            .await
            .unwrap_err();

        match err {
            MongoError::Command {
                code, code_name, ..
            } => {
                assert_eq!(code, UNAUTHORIZED.code);
                assert_eq!(code_name, UNAUTHORIZED.code_name);
            }
            other => panic!("expected unauthorized command error, got: {other:?}"),
        }
    }
}
