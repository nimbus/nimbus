use super::*;

pub(crate) fn issue_es256_test_token(
    issuer: &str,
    application_id: &str,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (String, String) {
    issue_es256_test_token_with_audience(issuer, json!(application_id), subject, extra_claims)
}

pub(crate) fn issue_es256_test_token_with_audience(
    issuer: &str,
    audience: serde_json::Value,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (String, String) {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
        .expect("test key should generate");
    let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
        .expect("test key should parse");
    let header = json!({
        "alg": "ES256",
        "kid": "test-key",
        "typ": "JWT"
    });
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs();
    let mut claims = serde_json::Map::new();
    claims.insert("iss".to_string(), json!(issuer));
    claims.insert("sub".to_string(), json!(subject));
    claims.insert("aud".to_string(), audience);
    claims.insert("exp".to_string(), json!(now + 300));
    claims.insert("iat".to_string(), json!(now));
    if let serde_json::Value::Object(extra) = extra_claims {
        claims.extend(extra);
    }

    let header_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("jwt header should serialize"));
    let claims_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).expect("jwt claims should serialize"));
    let signing_input = format!("{header_segment}.{claims_segment}");
    let signature = key_pair
        .sign(&rng, signing_input.as_bytes())
        .expect("jwt signature should sign");
    let token = format!(
        "{signing_input}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.as_ref())
    );

    let public_key = key_pair.public_key().as_ref();
    let jwks = json!({
        "keys": [
            {
                "kid": "test-key",
                "kty": "EC",
                "crv": "P-256",
                "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_key[1..33]),
                "y": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_key[33..65]),
                "alg": "ES256",
                "use": "sig"
            }
        ]
    });
    let jwks_data_url = format!(
        "data:application/json;base64,{}",
        base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_vec(&jwks).expect("jwks should serialize"))
    );

    (token, jwks_data_url)
}

pub(crate) fn issue_eddsa_test_token(
    issuer: &str,
    audience: serde_json::Value,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (String, serde_json::Value) {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("test key should generate");
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("test key should parse");
    let header = json!({
        "alg": "EdDSA",
        "kid": "test-key",
        "typ": "JWT"
    });
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs();
    let mut claims = serde_json::Map::new();
    claims.insert("iss".to_string(), json!(issuer));
    claims.insert("sub".to_string(), json!(subject));
    claims.insert("aud".to_string(), audience);
    claims.insert("exp".to_string(), json!(now + 300));
    claims.insert("iat".to_string(), json!(now));
    if let serde_json::Value::Object(extra) = extra_claims {
        claims.extend(extra);
    }

    let header_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("jwt header should serialize"));
    let claims_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).expect("jwt claims should serialize"));
    let signing_input = format!("{header_segment}.{claims_segment}");
    let signature = key_pair.sign(signing_input.as_bytes());
    let token = format!(
        "{signing_input}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.as_ref())
    );

    let jwks = json!({
        "keys": [
            {
                "kid": "test-key",
                "kty": "OKP",
                "crv": "Ed25519",
                "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key_pair.public_key().as_ref()),
                "alg": "EdDSA",
                "use": "sig"
            }
        ]
    });

    (token, jwks)
}
