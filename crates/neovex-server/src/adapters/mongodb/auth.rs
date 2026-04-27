use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::AuthConfig;
use super::connection::{ConnectionState, ScramState};
use super::error::{AUTHENTICATION_FAILED, BAD_VALUE, MongoError};

type HmacSha256 = Hmac<Sha256>;

pub fn sasl_start(
    body: &bson::Document,
    conn: &mut ConnectionState,
    auth: &AuthConfig,
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

    let (client_nonce, _username) = parse_client_first(client_first)?;

    let salt = auth.salt;

    let server_nonce_suffix: u64 = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        conn.connection_id.hash(&mut hasher);
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        hasher.finish()
    };

    let server_nonce = format!("{client_nonce}{server_nonce_suffix:016x}");
    let salt_b64 = BASE64.encode(salt);

    let server_first = format!("r={server_nonce},s={salt_b64},i={}", auth.iterations);

    let client_first_bare = strip_gs2_header(client_first);
    let auth_message = format!("{client_first_bare},{server_first}");

    let server_key = compute_server_key(&auth.password, &salt, auth.iterations);

    let conversation_id = conn.connection_id as i32;
    conn.scram_state = Some(ScramState {
        conversation_id,
        client_nonce,
        server_nonce,
        salt: salt.to_vec(),
        iterations: auth.iterations,
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
    auth: &AuthConfig,
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

    let client_final_without_proof = format!("c={channel_binding},r={nonce}");
    let full_auth_message = format!("{},{client_final_without_proof}", scram.auth_message);

    let salted_password = derive_salted_password(&auth.password, &scram.salt, scram.iterations);
    let client_key = compute_hmac(&salted_password, b"Client Key");
    let stored_key = sha256_hash(&client_key);
    let client_signature = compute_hmac(&stored_key, full_auth_message.as_bytes());

    let mut client_proof = client_key.clone();
    for (i, b) in client_signature.iter().enumerate() {
        client_proof[i] ^= b;
    }

    let expected_proof = BASE64.encode(&client_proof);
    if proof_b64 != expected_proof {
        return Err(MongoError::Command {
            code: AUTHENTICATION_FAILED.code,
            code_name: AUTHENTICATION_FAILED.code_name.into(),
            message: "authentication failed".into(),
        });
    }

    let server_signature = compute_hmac(&scram.server_key, full_auth_message.as_bytes());
    let server_final = format!("v={}", BASE64.encode(&server_signature));

    conn.authenticated = true;
    conn.auth_user = Some(auth.username.clone());

    conn.scram_state = Some(ScramState {
        conversation_id: scram.conversation_id,
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

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    fn test_auth() -> AuthConfig {
        AuthConfig::new("admin".into(), "admin".into())
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
        let err = sasl_start(&body, &mut conn, &auth).unwrap_err();
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
        let doc = sasl_start(&body, &mut conn, &auth).unwrap();

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
    fn full_scram_exchange() {
        let mut conn = test_conn();
        let auth = test_auth();
        let client_nonce = "ODcyMTQ2NDk=";
        let client_first = format!("n,,n={},r={client_nonce}", auth.username);
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };
        let step1 = sasl_start(&body, &mut conn, &auth).unwrap();
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

        let salt = BASE64.decode(&salt_b64).unwrap();
        let salted_password = derive_salted_password(&auth.password, &salt, iterations);
        let client_key = compute_hmac(&salted_password, b"Client Key");
        let stored_key = sha256_hash(&client_key);

        let client_first_bare = format!("n={},r={client_nonce}", auth.username);
        let channel_binding = BASE64.encode(b"n,,");
        let client_final_without_proof = format!("c={channel_binding},r={server_nonce}");

        let auth_message =
            format!("{client_first_bare},{server_first},{client_final_without_proof}");
        let client_signature = compute_hmac(&stored_key, auth_message.as_bytes());
        let mut proof = client_key;
        for (i, b) in client_signature.iter().enumerate() {
            proof[i] ^= b;
        }
        let proof_b64 = BASE64.encode(&proof);
        let client_final = format!("{client_final_without_proof},p={proof_b64}");

        let body2 = bson::doc! {
            "saslContinue": 1,
            "conversationId": conn.scram_state.as_ref().unwrap().conversation_id,
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_final.as_bytes().to_vec() },
        };
        let step2 = sasl_continue(&body2, &mut conn, &auth).unwrap();
        assert!(step2.get_bool("done").unwrap());
        assert_eq!(step2.get_f64("ok").unwrap(), 1.0);
        assert!(conn.authenticated);

        let server_final_payload = step2.get_binary_generic("payload").unwrap();
        let server_final = std::str::from_utf8(server_final_payload.as_slice()).unwrap();
        assert!(server_final.starts_with("v="));
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
        let step1 = sasl_start(&body, &mut conn, &auth).unwrap();
        let server_first_payload = step1.get_binary_generic("payload").unwrap();
        let server_first = std::str::from_utf8(server_first_payload.as_slice()).unwrap();
        let mut server_nonce = String::new();
        for part in server_first.split(',') {
            if let Some(v) = part.strip_prefix("r=") {
                server_nonce = v.to_string();
            }
        }

        let bad_proof = BASE64.encode(b"this-is-wrong-proof-data-xxxxx!");
        let channel_binding = BASE64.encode(b"n,,");
        let client_final = format!("c={channel_binding},r={server_nonce},p={bad_proof}");

        let body2 = bson::doc! {
            "saslContinue": 1,
            "conversationId": conn.scram_state.as_ref().unwrap().conversation_id,
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_final.as_bytes().to_vec() },
        };
        let err = sasl_continue(&body2, &mut conn, &auth).unwrap_err();
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
        let err = sasl_continue(&body, &mut conn, &auth).unwrap_err();
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
        let client_nonce = "Y3VzdG9tX25vbmNl";
        let client_first = format!("n,,n={},r={client_nonce}", auth.username);
        let body = bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_first.as_bytes().to_vec() },
        };
        let step1 = sasl_start(&body, &mut conn, &auth).unwrap();
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

        let salt = BASE64.decode(&salt_b64).unwrap();
        let salted_password = derive_salted_password(&auth.password, &salt, iterations);
        let client_key = compute_hmac(&salted_password, b"Client Key");
        let stored_key = sha256_hash(&client_key);

        let client_first_bare = format!("n={},r={client_nonce}", auth.username);
        let channel_binding = BASE64.encode(b"n,,");
        let client_final_without_proof = format!("c={channel_binding},r={server_nonce}");

        let auth_message =
            format!("{client_first_bare},{server_first},{client_final_without_proof}");
        let client_signature = compute_hmac(&stored_key, auth_message.as_bytes());
        let mut proof = client_key;
        for (i, b) in client_signature.iter().enumerate() {
            proof[i] ^= b;
        }
        let proof_b64 = BASE64.encode(&proof);
        let client_final = format!("{client_final_without_proof},p={proof_b64}");

        let body2 = bson::doc! {
            "saslContinue": 1,
            "conversationId": conn.scram_state.as_ref().unwrap().conversation_id,
            "payload": bson::Binary { subtype: bson::spec::BinarySubtype::Generic, bytes: client_final.as_bytes().to_vec() },
        };
        let step2 = sasl_continue(&body2, &mut conn, &auth).unwrap();
        assert!(step2.get_bool("done").unwrap());
        assert!(conn.authenticated);
        assert_eq!(conn.auth_user.as_deref(), Some("myuser"));
    }

    #[test]
    fn each_auth_config_gets_unique_salt() {
        let a1 = AuthConfig::new("user1".into(), "pass1".into());
        let a2 = AuthConfig::new("user2".into(), "pass2".into());
        assert_ne!(a1.salt, a2.salt);
    }

    #[test]
    fn strip_gs2_header_works() {
        assert_eq!(strip_gs2_header("n,,n=user,r=nonce"), "n=user,r=nonce");
        assert_eq!(strip_gs2_header("p=tls,,n=user,r=nonce"), "n=user,r=nonce");
    }
}
