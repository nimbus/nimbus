use super::*;

#[derive(Clone)]
struct MockOidcState {
    issuer: String,
    jwks: Arc<Mutex<serde_json::Value>>,
    discovery_requests: Arc<AtomicUsize>,
    jwks_requests: Arc<AtomicUsize>,
}

pub(crate) struct MockOidcProvider {
    issuer: String,
    jwks: Arc<Mutex<serde_json::Value>>,
    discovery_requests: Arc<AtomicUsize>,
    jwks_requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl MockOidcProvider {
    pub(crate) fn issuer(&self) -> &str {
        &self.issuer
    }

    pub(crate) fn set_jwks(&self, jwks: serde_json::Value) {
        *self
            .jwks
            .lock()
            .expect("mock oidc jwks lock should not be poisoned") = jwks;
    }

    pub(crate) fn discovery_request_count(&self) -> usize {
        self.discovery_requests.load(Ordering::SeqCst)
    }

    pub(crate) fn jwks_request_count(&self) -> usize {
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

pub(crate) async fn mock_oidc_provider_with_token(
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
