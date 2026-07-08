use hmac::{Hmac, Mac};
use nimbus_core::TenantId;
use nimbus_core::{base64_decode_standard, base64_encode_standard, base64_encode_url_safe_no_pad};
use ring::rand::{SecureRandom, SystemRandom};
use sha2::Sha256;

use super::connection::{ConnectionState, ScramState};
use super::credential_registry::MongoAuth;
use super::error::{AUTHENTICATION_FAILED, BAD_VALUE, MongoError};

type HmacSha256 = Hmac<Sha256>;

/// The credential material resolved for one SCRAM handshake, plus the tenant (if
/// any) that authenticating this credential binds.
struct ResolvedCredential {
    /// The raw (SCRAM-unescaped) authenticated username.
    username: String,
    password: String,
    salt: Vec<u8>,
    iterations: u32,
    /// `Some` in bound mode (authentication decides the tenant), `None` in
    /// tenant-agnostic unbound mode.
    tenant: Option<TenantId>,
}

/// Resolve the per-username credential material for the active auth mode.
///
/// - Unbound: the username must equal the configured username; the material is
///   the single config credential and no tenant is bound.
/// - Bound: the username must resolve in the registry; the material is that
///   binding's, and authenticating it binds the binding's tenant.
fn resolve_credential(
    auth: &MongoAuth<'_>,
    raw_username: &str,
) -> Result<ResolvedCredential, MongoError> {
    match auth {
        MongoAuth::Unbound(config) => {
            if raw_username.is_empty() || raw_username != config.username {
                return Err(authentication_failed("authentication failed"));
            }
            Ok(ResolvedCredential {
                username: config.username.clone(),
                password: config.password.clone(),
                salt: config.salt.to_vec(),
                iterations: config.iterations,
                tenant: None,
            })
        }
        MongoAuth::Bound(registry) => {
            if raw_username.is_empty() {
                return Err(authentication_failed("authentication failed"));
            }
            let binding = registry.resolve(raw_username)?;
            Ok(ResolvedCredential {
                username: raw_username.to_string(),
                password: binding.password.clone(),
                salt: binding.salt.to_vec(),
                iterations: binding.iterations,
                tenant: Some(binding.tenant.clone()),
            })
        }
    }
}

pub fn sasl_start(
    body: &bson::Document,
    conn: &mut ConnectionState,
    auth: &MongoAuth,
) -> Result<bson::Document, MongoError> {
    let mechanism = body.get_str("mechanism").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing mechanism field".into(),
    })?;

    if mechanism != "SCRAM-SHA-256" {
        return Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: format!("unsupported mechanism: {mechanism}"),
        });
    }

    let payload = body
        .get_binary_generic("payload")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing payload field".into(),
        })?;

    let client_first =
        std::str::from_utf8(payload.as_slice()).map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "invalid UTF-8 in SCRAM payload".into(),
        })?;

    let (client_nonce, username_wire) = parse_client_first(client_first)?;
    let raw_username = scram_unescape_username(&username_wire);
    let credential = resolve_credential(auth, &raw_username)?;

    let mut server_nonce_suffix = [0u8; 18];
    fill_secure_random(&mut server_nonce_suffix)?;
    let server_nonce = format!(
        "{client_nonce}{}",
        base64_encode_url_safe_no_pad(server_nonce_suffix)
    );
    let salt_b64 = base64_encode_standard(&credential.salt);

    let server_first = format!("r={server_nonce},s={salt_b64},i={}", credential.iterations);

    let client_first_bare = strip_gs2_header(client_first);
    let auth_message = format!("{client_first_bare},{server_first}");

    let server_key = compute_server_key(
        &credential.password,
        &credential.salt,
        credential.iterations,
    );

    let conversation_id = conn.connection_id as i32;
    conn.scram_state = Some(ScramState {
        conversation_id,
        username: credential.username,
        client_nonce,
        server_nonce,
        salt: credential.salt,
        iterations: credential.iterations,
        auth_message,
        server_key,
    });

    let payload_bytes = server_first.into_bytes();

    Ok(bson::doc! {
        "conversationId": conversation_id,
        "done": false,
        "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: payload_bytes },
        "ok": 1.0,
    })
}

pub fn sasl_continue(
    body: &bson::Document,
    conn: &mut ConnectionState,
    auth: &MongoAuth,
) -> Result<bson::Document, MongoError> {
    let scram = conn.scram_state.take().ok_or_else(|| MongoError::Command {
        code: AUTHENTICATION_FAILED.code,
        code_name: AUTHENTICATION_FAILED.code_name.into(),
        message: "no SCRAM conversation in progress".into(),
    })?;

    let payload = body
        .get_binary_generic("payload")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing payload field".into(),
        })?;

    let client_final =
        std::str::from_utf8(payload.as_slice()).map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "invalid UTF-8 in SCRAM payload".into(),
        })?;

    if client_final.is_empty() {
        return Ok(bson::doc! {
            "conversationId": scram.conversation_id,
            "done": true,
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: vec![] },
            "ok": 1.0,
        });
    }

    let (channel_binding, nonce, proof_b64) = parse_client_final(client_final)?;

    if nonce != scram.server_nonce {
        return Err(MongoError::Command {
            code: AUTHENTICATION_FAILED.code,
            code_name: AUTHENTICATION_FAILED.code_name.into(),
            message: "nonce mismatch".into(),
        });
    }

    // Re-resolve the same credential by the username carried from `saslStart`,
    // so the proof is checked against — and the tenant is bound from — exactly
    // the authenticated credential.
    let credential = resolve_credential(auth, &scram.username)?;

    let client_final_without_proof = format!("c={channel_binding},r={nonce}");
    let full_auth_message = format!("{},{client_final_without_proof}", scram.auth_message);

    let salted_password =
        derive_salted_password(&credential.password, &scram.salt, scram.iterations);
    let client_key = compute_hmac(&salted_password, b"Client Key");
    let stored_key = sha256_hash(&client_key);
    let client_signature = compute_hmac(&stored_key, full_auth_message.as_bytes());

    let mut client_proof = client_key.clone();
    for (i, b) in client_signature.iter().enumerate() {
        client_proof[i] ^= b;
    }

    let proof = base64_decode_standard(proof_b64.as_bytes()).map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "invalid base64 proof in client-final-message".into(),
    })?;
    if !constant_time_eq(&proof, &client_proof) {
        return Err(authentication_failed("authentication failed"));
    }

    let server_signature = compute_hmac(&scram.server_key, full_auth_message.as_bytes());
    let server_final = format!("v={}", base64_encode_standard(&server_signature));

    conn.authenticated = true;
    conn.auth_user = Some(credential.username.clone());
    // Fail-closed: in bound mode a successful handshake always binds a tenant
    // (an unknown username never reaches here); in unbound mode this stays
    // `None` and the connection remains tenant-agnostic.
    conn.authenticated_tenant = credential.tenant;

    conn.scram_state = Some(ScramState {
        conversation_id: scram.conversation_id,
        username: String::new(),
        client_nonce: String::new(),
        server_nonce: String::new(),
        salt: vec![],
        iterations: 0,
        auth_message: String::new(),
        server_key: vec![],
    });

    Ok(bson::doc! {
        "conversationId": scram.conversation_id,
        "done": true,
        "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: server_final.into_bytes() },
        "ok": 1.0,
    })
}

fn parse_client_first(msg: &str) -> Result<(String, String), MongoError> {
    let bare = strip_gs2_header(msg);
    let mut username = String::new();
    let mut nonce = String::new();

    for part in bare.split(',') {
        if let Some(val) = part.strip_prefix("n=") {
            username = val.to_string();
        } else if let Some(val) = part.strip_prefix("r=") {
            nonce = val.to_string();
        }
    }

    if nonce.is_empty() {
        return Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing nonce in client-first-message".into(),
        });
    }

    Ok((nonce, username))
}

/// Reverse SCRAM username escaping (`=2C` → `,`, `=3D` → `=`) so the raw
/// username can be compared and looked up. Inverse of the client's escaping;
/// the order undoes `=`→`=3D` then `,`→`=2C`.
fn scram_unescape_username(username: &str) -> String {
    username.replace("=2C", ",").replace("=3D", "=")
}

fn fill_secure_random(bytes: &mut [u8]) -> Result<(), MongoError> {
    SystemRandom::new()
        .fill(bytes)
        .map_err(|_| authentication_failed("authentication failed"))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}

fn authentication_failed(message: impl Into<String>) -> MongoError {
    MongoError::Command {
        code: AUTHENTICATION_FAILED.code,
        code_name: AUTHENTICATION_FAILED.code_name.into(),
        message: message.into(),
    }
}

fn strip_gs2_header(msg: &str) -> &str {
    let mut count = 0;
    for (i, c) in msg.char_indices() {
        if c == ',' {
            count += 1;
            if count == 2 {
                return &msg[i + 1..];
            }
        }
    }
    msg
}

fn parse_client_final(msg: &str) -> Result<(String, String, String), MongoError> {
    let mut channel_binding = String::new();
    let mut nonce = String::new();
    let mut proof = String::new();

    for part in msg.split(',') {
        if let Some(val) = part.strip_prefix("c=") {
            channel_binding = val.to_string();
        } else if let Some(val) = part.strip_prefix("r=") {
            nonce = val.to_string();
        } else if let Some(val) = part.strip_prefix("p=") {
            proof = val.to_string();
        }
    }

    if nonce.is_empty() || proof.is_empty() {
        return Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing nonce or proof in client-final-message".into(),
        });
    }

    Ok((channel_binding, nonce, proof))
}

fn derive_salted_password(password: &str, salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut salted = vec![0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut salted);
    salted
}

fn compute_hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn compute_server_key(password: &str, salt: &[u8], iterations: u32) -> Vec<u8> {
    let salted_password = derive_salted_password(password, salt, iterations);
    compute_hmac(&salted_password, b"Server Key")
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthConfig;
    use crate::error::UNAUTHORIZED;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    fn test_auth() -> AuthConfig {
        AuthConfig::new("admin".into(), "admin".into())
    }

    /// Drive a full SCRAM-SHA-256 handshake as `username`/`password` against the
    /// given auth mode, through the real `sasl_start`/`sasl_continue` flow, and
    /// return the final `saslContinue` reply. The username must need no SCRAM
    /// escaping (true for every test username here).
    fn scram_authenticate(
        conn: &mut ConnectionState,
        auth: &MongoAuth,
        username: &str,
        password: &str,
    ) -> bson::Document {
        let client_nonce = "Y2xpZW50LW5vbmNl";
        let client_first = format!("n,,n={username},r={client_nonce}");
        let start_body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };
        let step1 = sasl_start(&start_body, conn, auth).expect("saslStart should succeed");
        assert!(!step1.get_bool("done").unwrap());

        let server_first_payload = step1.get_binary_generic("payload").unwrap();
        let server_first = std::str::from_utf8(server_first_payload.as_slice()).unwrap();

        let mut server_nonce = String::new();
        let mut salt_b64 = String::new();
        let mut iterations = 0u32;
        for part in server_first.split(',') {
            if let Some(v) = part.strip_prefix("r=") {
                server_nonce = v.to_string();
            } else if let Some(v) = part.strip_prefix("s=") {
                salt_b64 = v.to_string();
            } else if let Some(v) = part.strip_prefix("i=") {
                iterations = v.parse().unwrap();
            }
        }

        let salt = base64_decode_standard(&salt_b64).unwrap();
        let salted_password = derive_salted_password(password, &salt, iterations);
        let client_key = compute_hmac(&salted_password, b"Client Key");
        let stored_key = sha256_hash(&client_key);

        let client_first_bare = format!("n={username},r={client_nonce}");
        let channel_binding = base64_encode_standard(b"n,,");
        let client_final_without_proof = format!("c={channel_binding},r={server_nonce}");
        let auth_message =
            format!("{client_first_bare},{server_first},{client_final_without_proof}");
        let client_signature = compute_hmac(&stored_key, auth_message.as_bytes());
        let mut proof = client_key;
        for (i, b) in client_signature.iter().enumerate() {
            proof[i] ^= b;
        }
        let proof_b64 = base64_encode_standard(&proof);
        let client_final = format!("{client_final_without_proof},p={proof_b64}");

        let conversation_id = conn.scram_state.as_ref().unwrap().conversation_id;
        let continue_body = bson::doc! {
            "saslContinue": 1,
            "conversationId": conversation_id,
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_final.as_bytes().to_vec() },
        };
        sasl_continue(&continue_body, conn, auth).expect("saslContinue should succeed")
    }

    #[test]
    fn sasl_start_rejects_unsupported_mechanism() {
        let mut conn = test_conn();
        let auth = test_auth();
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-1",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: vec![] },
        };
        let err = sasl_start(&body, &mut conn, &MongoAuth::Unbound(&auth)).unwrap_err();
        match err {
            MongoError::Command { message, .. } => {
                assert!(message.contains("unsupported mechanism"));
            }
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn sasl_start_returns_server_first() {
        let mut conn = test_conn();
        let auth = test_auth();
        let client_first = "n,,n=admin,r=clientnonce123";
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };
        let doc = sasl_start(&body, &mut conn, &MongoAuth::Unbound(&auth)).unwrap();

        assert!(!doc.get_bool("done").unwrap());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
        assert!(doc.get_i32("conversationId").is_ok());

        let payload = doc.get_binary_generic("payload").unwrap();
        let server_first = std::str::from_utf8(payload.as_slice()).unwrap();
        assert!(server_first.starts_with("r=clientnonce123"));
        assert!(server_first.contains(",s="));
        assert!(server_first.contains(",i=4096"));
        assert!(conn.scram_state.is_some());
    }

    #[test]
    fn sasl_start_rejects_wrong_username() {
        let mut conn = test_conn();
        let auth = test_auth();
        let client_first = "n,,n=intruder,r=clientnonce123";
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };

        let err = sasl_start(&body, &mut conn, &MongoAuth::Unbound(&auth)).unwrap_err();

        match err {
            MongoError::Command { code, .. } => {
                assert_eq!(code, AUTHENTICATION_FAILED.code);
            }
            other => panic!("expected Command, got {:?}", other),
        }
        assert!(conn.scram_state.is_none());
    }

    #[test]
    fn sasl_start_accepts_scram_escaped_configured_username() {
        let mut conn = test_conn();
        let auth = AuthConfig::new("name,with=chars".into(), "admin".into());
        let client_first = "n,,n=name=2Cwith=3Dchars,r=clientnonce123";
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };

        let doc = sasl_start(&body, &mut conn, &MongoAuth::Unbound(&auth)).unwrap();

        assert!(!doc.get_bool("done").unwrap());
        assert!(conn.scram_state.is_some());
    }

    #[test]
    fn full_scram_exchange() {
        let mut conn = test_conn();
        let auth = test_auth();
        let step2 = scram_authenticate(
            &mut conn,
            &MongoAuth::Unbound(&auth),
            &auth.username,
            &auth.password,
        );
        assert!(step2.get_bool("done").unwrap());
        assert_eq!(step2.get_f64("ok").unwrap(), 1.0);
        assert!(conn.authenticated);

        let server_final_payload = step2.get_binary_generic("payload").unwrap();
        let server_final = std::str::from_utf8(server_final_payload.as_slice()).unwrap();
        assert!(server_final.starts_with("v="));

        // Unbound mode is tenant-agnostic: authentication binds no tenant.
        assert!(conn.authenticated_tenant().is_none());
    }

    #[test]
    fn sasl_continue_rejects_bad_proof() {
        let mut conn = test_conn();
        let auth = test_auth();
        let client_first = "n,,n=admin,r=testnonce";
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };
        let step1 = sasl_start(&body, &mut conn, &MongoAuth::Unbound(&auth)).unwrap();
        let server_first_payload = step1.get_binary_generic("payload").unwrap();
        let server_first = std::str::from_utf8(server_first_payload.as_slice()).unwrap();
        let mut server_nonce = String::new();
        for part in server_first.split(',') {
            if let Some(v) = part.strip_prefix("r=") {
                server_nonce = v.to_string();
            }
        }

        let bad_proof = base64_encode_standard(b"this-is-wrong-proof-data-xxxxx!");
        let channel_binding = base64_encode_standard(b"n,,");
        let client_final = format!("c={channel_binding},r={server_nonce},p={bad_proof}");

        let body2 = bson::doc! {
            "saslContinue": 1,
            "conversationId": conn.scram_state.as_ref().unwrap().conversation_id,
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_final.as_bytes().to_vec() },
        };
        let err = sasl_continue(&body2, &mut conn, &MongoAuth::Unbound(&auth)).unwrap_err();
        match err {
            MongoError::Command { code, .. } => {
                assert_eq!(code, AUTHENTICATION_FAILED.code);
            }
            other => panic!("expected Command, got {:?}", other),
        }
        assert!(!conn.authenticated);
    }

    #[test]
    fn sasl_continue_without_start_fails() {
        let mut conn = test_conn();
        let auth = test_auth();
        let body = bson::doc! {
            "saslContinue": 1,
            "conversationId": 1,
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: vec![] },
        };
        let err = sasl_continue(&body, &mut conn, &MongoAuth::Unbound(&auth)).unwrap_err();
        match err {
            MongoError::Command { code, .. } => {
                assert_eq!(code, AUTHENTICATION_FAILED.code);
            }
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn full_scram_with_custom_credentials() {
        let auth = AuthConfig::new("myuser".into(), "secretpass".into());
        let mut conn = test_conn();
        let step2 = scram_authenticate(
            &mut conn,
            &MongoAuth::Unbound(&auth),
            &auth.username,
            &auth.password,
        );
        assert!(step2.get_bool("done").unwrap());
        assert!(conn.authenticated);
        assert_eq!(conn.auth_user.as_deref(), Some("myuser"));
        assert!(conn.authenticated_tenant().is_none());
    }

    #[test]
    fn each_auth_config_gets_unique_salt() {
        let a1 = AuthConfig::new("user".into(), "pass".into());
        let a2 = AuthConfig::new("user".into(), "pass".into());
        assert_ne!(a1.salt, a2.salt);
    }

    #[test]
    fn strip_gs2_header_works() {
        assert_eq!(strip_gs2_header("n,,n=user,r=nonce"), "n=user,r=nonce");
        assert_eq!(strip_gs2_header("p=tls,,n=user,r=nonce"), "n=user,r=nonce");
    }

    #[test]
    fn bound_unknown_username_is_authentication_failed() {
        // Strict-by-default: an unknown username never starts a handshake in
        // bound mode, so a bound connection can only ever authenticate a
        // credential that resolves to a tenant (the fail-closed invariant).
        use crate::credential_registry::CredentialRegistry;
        use nimbus_core::TenantId;

        let registry = CredentialRegistry::new().bind(
            "user-a",
            TenantId::new("tenant-a").unwrap(),
            "secret-a",
        );
        let auth = MongoAuth::Bound(&registry);
        let mut conn = test_conn();

        let client_first = "n,,n=nobody,r=clientnonce123";
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };
        let err = sasl_start(&body, &mut conn, &auth).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, AUTHENTICATION_FAILED.code),
            other => panic!("expected Command, got {other:?}"),
        }
        assert!(conn.scram_state.is_none());
        assert!(conn.authenticated_tenant().is_none());
    }

    /// The acceptance bar: through the real auth + command flow, a bound
    /// credential's tenant is fixed by authentication and the wire `$db` cannot
    /// reach another tenant — observed as REFUSED on a CRUD path (`find`) AND on
    /// the session/transaction path, with the same-tenant case allowed.
    #[tokio::test]
    async fn bound_credential_refuses_cross_tenant_on_crud_and_session_paths() {
        use crate::commands::dispatch_authed;
        use crate::credential_registry::CredentialRegistry;
        use nimbus_core::TenantId;
        use nimbus_engine::Engine;
        use nimbus_testing::EngineFixture;

        let registry = CredentialRegistry::new()
            .bind("user-a", TenantId::new("tenant-a").unwrap(), "secret-a")
            .bind("user-b", TenantId::new("tenant-b").unwrap(), "secret-b");
        let auth = MongoAuth::Bound(&registry);

        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        engine
            .create_tenant(TenantId::new("tenant-a").unwrap())
            .expect("create tenant-a");

        // Real SCRAM handshake as user-a; authentication binds tenant-a.
        let mut conn = test_conn();
        let final_reply = scram_authenticate(&mut conn, &auth, "user-a", "secret-a");
        assert!(final_reply.get_bool("done").unwrap());
        assert!(conn.authenticated);
        assert_eq!(
            conn.authenticated_tenant().map(|t| t.as_str()),
            Some("tenant-a"),
            "authentication, not the wire $db, must decide the tenant"
        );

        // CRUD path: cross-tenant find ($db = tenant-b) is REFUSED.
        let cross_find = bson::doc! { "find": "users", "$db": "tenant-b", "filter": {} };
        let refused = dispatch_authed("find", &cross_find, &mut conn, &engine, &auth)
            .await
            .expect_err("cross-tenant find must be refused");
        match refused {
            MongoError::Command { code, .. } => assert_eq!(code, UNAUTHORIZED.code),
            other => panic!("expected command error, got {other:?}"),
        }

        // CRUD path: same-tenant insert + find ($db = tenant-a) is allowed.
        let same_insert = bson::doc! {
            "insert": "users",
            "$db": "tenant-a",
            "documents": [ { "_id": "a1", "name": "alice" } ],
        };
        let inserted = dispatch_authed("insert", &same_insert, &mut conn, &engine, &auth)
            .await
            .expect("same-tenant insert must be allowed");
        assert_eq!(inserted.get_i32("n").unwrap(), 1);

        let same_find = bson::doc! { "find": "users", "$db": "tenant-a", "filter": {} };
        let allowed = dispatch_authed("find", &same_find, &mut conn, &engine, &auth)
            .await
            .expect("same-tenant find must be allowed");
        assert_eq!(allowed.get_f64("ok").unwrap(), 1.0);
        assert_eq!(
            allowed
                .get_document("cursor")
                .unwrap()
                .get_array("firstBatch")
                .unwrap()
                .len(),
            1,
            "same-tenant find should see the inserted document"
        );

        // Session/transaction path: the FIRST tenant selection in a session is
        // auth-bound, not $db-bound. A cross-tenant startTransaction ($db =
        // tenant-b) after authenticating as user-a is REFUSED.
        let session = dispatch_authed(
            "startSession",
            &bson::doc! { "startSession": 1 },
            &mut conn,
            &engine,
            &auth,
        )
        .await
        .expect("startSession");
        let lsid = bson::Bson::Document(session.get_document("id").unwrap().clone());

        let cross_txn = bson::doc! {
            "find": "users",
            "$db": "tenant-b",
            "filter": {},
            "startTransaction": true,
            "lsid": lsid.clone(),
        };
        let refused_txn = dispatch_authed("find", &cross_txn, &mut conn, &engine, &auth)
            .await
            .expect_err("cross-tenant transaction must be refused");
        match refused_txn {
            MongoError::Command { code, .. } => assert_eq!(code, UNAUTHORIZED.code),
            other => panic!("expected command error, got {other:?}"),
        }

        // Session/transaction path: same-tenant startTransaction is allowed.
        let same_txn = bson::doc! {
            "find": "users",
            "$db": "tenant-a",
            "filter": {},
            "startTransaction": true,
            "lsid": lsid,
        };
        let allowed_txn = dispatch_authed("find", &same_txn, &mut conn, &engine, &auth)
            .await
            .expect("same-tenant transaction must be allowed");
        assert_eq!(allowed_txn.get_f64("ok").unwrap(), 1.0);
    }
}
