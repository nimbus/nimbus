use super::*;

#[derive(Clone)]
struct MockOidcState {
    issuer: String,
    jwks: Arc<Mutex<serde_json::Value>>,
    discovery_requests: Arc<AtomicUsize>,
    jwks_requests: Arc<AtomicUsize>,
}

pub(super) struct MockOidcProvider {
    issuer: String,
    jwks: Arc<Mutex<serde_json::Value>>,
    discovery_requests: Arc<AtomicUsize>,
    jwks_requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl MockOidcProvider {
    pub(super) fn issuer(&self) -> &str {
        &self.issuer
    }

    pub(super) fn set_jwks(&self, jwks: serde_json::Value) {
        *self
            .jwks
            .lock()
            .expect("mock oidc jwks lock should not be poisoned") = jwks;
    }

    pub(super) fn discovery_request_count(&self) -> usize {
        self.discovery_requests.load(Ordering::SeqCst)
    }

    pub(super) fn jwks_request_count(&self) -> usize {
        self.jwks_requests.load(Ordering::SeqCst)
    }
}

impl Drop for MockOidcProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock_oidc_discovery(State(state): State<MockOidcState>) -> Json<serde_json::Value> {
    state.discovery_requests.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "issuer": state.issuer,
        "jwks_uri": format!("{}/jwks", state.issuer),
    }))
}

async fn mock_oidc_jwks(State(state): State<MockOidcState>) -> Json<serde_json::Value> {
    state.jwks_requests.fetch_add(1, Ordering::SeqCst);
    Json(
        state
            .jwks
            .lock()
            .expect("mock oidc jwks lock should not be poisoned")
            .clone(),
    )
}

async fn start_mock_oidc_provider(initial_jwks: serde_json::Value) -> MockOidcProvider {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock OIDC listener should bind");
    let issuer = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("mock OIDC listener should expose a local address")
    );
    let state = MockOidcState {
        issuer: issuer.clone(),
        jwks: Arc::new(Mutex::new(initial_jwks)),
        discovery_requests: Arc::new(AtomicUsize::new(0)),
        jwks_requests: Arc::new(AtomicUsize::new(0)),
    };
    let router = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(mock_oidc_discovery),
        )
        .route("/jwks", get(mock_oidc_jwks))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("mock OIDC server should run");
    });
    MockOidcProvider {
        issuer,
        jwks: state.jwks,
        discovery_requests: state.discovery_requests,
        jwks_requests: state.jwks_requests,
        task,
    }
}

pub(super) async fn mock_oidc_provider_with_token(
    audience: serde_json::Value,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (MockOidcProvider, String, serde_json::Value) {
    let placeholder_jwks = json!({ "keys": [] });
    let provider = start_mock_oidc_provider(placeholder_jwks).await;
    let (token, jwks) = issue_eddsa_test_token(provider.issuer(), audience, subject, extra_claims);
    provider.set_jwks(jwks.clone());
    (provider, token, jwks)
}

pub(super) fn runtime_auth_bundle_source() -> &'static str {
    r#"
const definitions = new Map([
  ["auth:whoami", {
    name: "auth:whoami",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => await ctx.auth.getUserIdentity()",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#
}

pub(super) fn runtime_verified_auth_bundle_source() -> &'static str {
    r#"
const definitions = new Map([
  ["auth:whoami", {
    name: "auth:whoami",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => ({ user: await ctx.auth.getUserIdentity(), verified: await ctx.auth.getVerifiedIdentity() })",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#
}

pub(super) fn runtime_auth_subscription_bundle_source() -> &'static str {
    r#"
const definitions = new Map([
  ["auth:watchIdentity", {
    name: "auth:watchIdentity",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#
}

pub(super) fn issue_es256_test_token(
    issuer: &str,
    application_id: &str,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (String, String) {
    issue_es256_test_token_with_audience(issuer, json!(application_id), subject, extra_claims)
}

pub(super) fn issue_es256_test_token_with_audience(
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

pub(super) fn issue_eddsa_test_token(
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
