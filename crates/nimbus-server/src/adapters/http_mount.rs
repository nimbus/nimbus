//! Registration seam for HTTP-mounted protocol surfaces.
//!
//! Adapters that answer requests on the main HTTP listener (Convex,
//! Firebase, Cloudflare, the Cloud Functions fallback) share one `Router`
//! and one `Arc<AppState>`. [`mount_adapters`] merges every enabled
//! adapter's routes in a fixed registration order, so adding an adapter
//! means implementing [`HttpProtocolAdapter`] — not growing another `if`
//! chain in `router::RouterBuildConfig::build`. Contrast with
//! [`super::wire::WireProtocolAdapter`], which registers sibling
//! listeners on their own ports rather than routes on this shared router.
//!
//! The chain always ends at [`route_not_found`]. "No route matches this
//! path" is the router's own answer, so it reads the same on every build
//! whatever set of adapters happens to be mounted, and an adapter that
//! claims the fallback slot declines a path through [`no_route`] rather than
//! describing the request in its own vocabulary.

use std::sync::Arc;

use axum::Router;
use axum::extract::OriginalUri;
use axum::http::Method;
use axum::routing::any;

use crate::adapters::cloud_functions;
use crate::adapters::cloudflare::{self, CloudflareConfig};
use crate::state::{AppError, AppState};

/// The one answer to a request that matched no route on this listener.
///
/// A fallback adapter that inspects a path and does not own it returns this
/// too: from the client's side the two are the same fact, and which internal
/// surface looked last is server detail.
pub(crate) fn no_route(method: &Method, path: &str) -> AppError {
    AppError::route_not_found(format!("no route matches {method} {path}"))
}

/// Terminates the fallback chain. Installed by [`mount_adapters`] whenever no
/// enabled adapter claimed the slot, so an unknown path is answered in the
/// server's error envelope on every build.
async fn route_not_found(method: Method, OriginalUri(uri): OriginalUri) -> AppError {
    no_route(&method, uri.path())
}

/// One HTTP-mounted protocol surface, registered into the router build in a
/// fixed order.
pub(crate) trait HttpProtocolAdapter {
    /// Stable adapter name, used only for the fallback-uniqueness assertion
    /// in [`mount_adapters`].
    fn name(&self) -> &'static str;

    /// Whether this adapter is configured on for this build. [`mount`](Self::mount)
    /// is called only when this returns `true`.
    fn enabled(&self) -> bool;

    /// Whether this adapter installs a router-wide fallback instead of
    /// merging routes. [`mount_adapters`] asserts at most one enabled
    /// adapter in a registration list reports `true` here — a second
    /// fallback would silently replace the first with no signal — and skips
    /// its own [`route_not_found`] terminator when one claims the slot. An
    /// adapter that claims it therefore owns the end of the chain, and must
    /// answer a path it does not handle with [`no_route`].
    fn is_fallback(&self) -> bool {
        false
    }

    /// Mount this adapter's routes onto `router`, consuming any
    /// adapter-owned state. Called only when [`enabled`](Self::enabled) is
    /// `true`.
    fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>>;
}

/// Merges every enabled adapter's routes onto `router` in registration
/// order, asserting at most one enabled adapter installs a fallback, and
/// terminating the chain with [`route_not_found`] when none did.
pub(crate) fn mount_adapters(
    mut router: Router<Arc<AppState>>,
    adapters: Vec<Box<dyn HttpProtocolAdapter>>,
) -> Router<Arc<AppState>> {
    let fallback_adapters: Vec<&'static str> = adapters
        .iter()
        .filter(|adapter| adapter.enabled() && adapter.is_fallback())
        .map(|adapter| adapter.name())
        .collect();
    assert!(
        fallback_adapters.len() <= 1,
        "at most one enabled HTTP adapter may install a router fallback, got {fallback_adapters:?}"
    );
    let mut fallback_claimed = false;
    for adapter in adapters {
        if adapter.enabled() {
            fallback_claimed |= adapter.is_fallback();
            router = adapter.mount(router);
        }
    }
    if fallback_claimed {
        router
    } else {
        router.fallback(route_not_found)
    }
}

/// Convex is mounted unconditionally: its routes exist on every build
/// regardless of whether a tenant deployment has published Convex
/// functions, matching the pre-seam `build_convex_router()` merge.
///
/// The #43 network-bind stopgap (a route-layer refusing the convex surface
/// on non-loopback) is gone: the #41 team-binding gate in the convex
/// admission funnel (`registry_and_auth` + `dispatch.rs`) now refuses
/// cross-team selection on every bind, superseding it.
pub(crate) struct ConvexHttpAdapter;

impl HttpProtocolAdapter for ConvexHttpAdapter {
    fn name(&self) -> &'static str {
        "convex"
    }

    fn enabled(&self) -> bool {
        true
    }

    fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
        router.merge(crate::router::build_convex_router())
    }
}

pub(crate) struct FirebaseHttpAdapter {
    enabled: bool,
    state: Arc<AppState>,
}

impl FirebaseHttpAdapter {
    pub(crate) fn new(enabled: bool, state: Arc<AppState>) -> Self {
        Self { enabled, state }
    }
}

impl HttpProtocolAdapter for FirebaseHttpAdapter {
    fn name(&self) -> &'static str {
        "firebase"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
        router.merge(crate::router::build_firebase_router(self.state))
    }
}

pub(crate) struct CloudflareHttpAdapter {
    config: Option<Arc<CloudflareConfig>>,
}

impl CloudflareHttpAdapter {
    pub(crate) fn new(config: Option<Arc<CloudflareConfig>>) -> Self {
        Self { config }
    }
}

impl HttpProtocolAdapter for CloudflareHttpAdapter {
    fn name(&self) -> &'static str {
        "cloudflare"
    }

    fn enabled(&self) -> bool {
        self.config.is_some()
    }

    fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
        let config = self
            .config
            .expect("mount is only called when enabled() is true");
        router.merge(cloudflare::build_cloudflare_router(config))
    }
}

/// Claims the fallback slot so a deployed Cloud Function can answer a path
/// that is not in the static routing table. It is therefore also the end of
/// the chain: a path it does not resolve to a target is a path no route
/// matches, and it says so with [`no_route`].
pub(crate) struct CloudFunctionsHttpAdapter {
    enabled: bool,
}

impl CloudFunctionsHttpAdapter {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl HttpProtocolAdapter for CloudFunctionsHttpAdapter {
    fn name(&self) -> &'static str {
        "cloud_functions"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn is_fallback(&self) -> bool {
        true
    }

    fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
        router.fallback(any(cloud_functions::http_handler))
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, Bytes, to_bytes};
    use axum::http::{Request, StatusCode};
    use nimbus_compute::config::control_plane::ControlPlaneConfig;
    use nimbus_compute::config::deployment::DeploymentConfig;
    use nimbus_compute::config::node_services::NodeServicesConfig;
    use nimbus_compute::config::runtime::RuntimeGovernorConfig;
    use nimbus_engine::Engine;
    use nimbus_testing::EngineFixture;
    use tower::ServiceExt;

    use super::*;
    use crate::config::transport::TransportConfig;
    use crate::state::AppStateConfig;

    struct StubAdapter {
        name: &'static str,
        enabled: bool,
        is_fallback: bool,
    }

    impl HttpProtocolAdapter for StubAdapter {
        fn name(&self) -> &'static str {
            self.name
        }

        fn enabled(&self) -> bool {
            self.enabled
        }

        fn is_fallback(&self) -> bool {
            self.is_fallback
        }

        // Mirrors the production adapters: an `is_fallback` stub installs a
        // router-wide fallback (like `CloudFunctionsHttpAdapter`) instead of
        // merging a route, so the fallback tests below can observe which
        // adapter actually won the fallback slot.
        fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
            let Self {
                name, is_fallback, ..
            } = *self;
            if is_fallback {
                router.fallback(axum::routing::any(move || async move { name }))
            } else {
                router.route(name, axum::routing::get(move || async move { name }))
            }
        }
    }

    /// Minimal `AppState` for driving requests through a `mount_adapters`
    /// router: the stub adapters' handlers never read state, so every field
    /// is a default. The `EngineFixture` must outlive the router.
    fn test_state() -> (Arc<AppState>, EngineFixture<Engine>) {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let state = Arc::new(AppState::from_config(AppStateConfig {
            workload: crate::workload_composition::ServerWorkloadProfile::protocol_only(
                fixture.engine(),
            ),
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::default(),
            transport: TransportConfig::default(),
            runtime: RuntimeGovernorConfig::default(),
            object_storage: nimbus_object_storage::ObjectStorageConfig::default(),
        }));
        (state, fixture)
    }

    async fn get(router: &Router, path: &str) -> (StatusCode, Bytes) {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("route should respond");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should collect");
        (status, body)
    }

    #[tokio::test]
    async fn mount_adapters_skips_disabled_adapters() {
        let (state, _fixture) = test_state();
        let router = mount_adapters(
            Router::new(),
            vec![
                Box::new(StubAdapter {
                    name: "/enabled",
                    enabled: true,
                    is_fallback: false,
                }),
                Box::new(StubAdapter {
                    name: "/disabled",
                    enabled: false,
                    is_fallback: false,
                }),
            ],
        )
        .with_state(state);

        let (status, body) = get(&router, "/enabled").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the enabled adapter's route must be mounted and reachable"
        );
        assert_eq!(
            &body[..],
            b"/enabled",
            "the enabled adapter's own handler must answer its route"
        );

        let (status, _) = get(&router, "/disabled").await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a disabled adapter must not have its route mounted, so its path 404s"
        );
    }

    #[tokio::test]
    async fn an_unmatched_path_answers_the_route_not_found_envelope_without_a_fallback_adapter() {
        let (state, _fixture) = test_state();
        let router = mount_adapters(
            Router::new(),
            vec![Box::new(StubAdapter {
                name: "/mounted",
                enabled: true,
                is_fallback: false,
            })],
        )
        .with_state(state);

        let (status, body) = get(&router, "/nothing-here").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let envelope: serde_json::Value = serde_json::from_slice(&body)
            .expect("the terminator must answer in the error envelope");
        assert_eq!(
            envelope["error"]["code"], "service.route_not_found",
            "a build with no fallback adapter must still name the routing table as the reason"
        );
        assert_eq!(
            envelope["error"]["message"],
            "no route matches GET /nothing-here"
        );
    }

    /// The Cloud Functions adapter owns the fallback slot on a normal build.
    /// A deployment with no Cloud Functions must still answer an unknown path
    /// exactly as the terminator would: the client asked about a route, and
    /// which internal surface looked last is not part of the answer.
    #[tokio::test]
    async fn the_cloud_functions_fallback_declines_an_unrouted_path_as_the_terminator_would() {
        let (state, _fixture) = test_state();
        let with_adapter = mount_adapters(
            Router::new(),
            vec![Box::new(CloudFunctionsHttpAdapter::new(true))],
        )
        .with_state(state.clone());
        let without_adapter = mount_adapters(Router::new(), Vec::new()).with_state(state);

        let (declined_status, declined_body) = get(&with_adapter, "/nothing-here").await;
        let (terminator_status, terminator_body) = get(&without_adapter, "/nothing-here").await;

        assert_eq!(declined_status, terminator_status);
        let declined: serde_json::Value = serde_json::from_slice(&declined_body).expect("envelope");
        let terminator: serde_json::Value =
            serde_json::from_slice(&terminator_body).expect("envelope");
        assert_eq!(declined["error"]["code"], terminator["error"]["code"]);
        assert_eq!(declined["error"]["message"], terminator["error"]["message"]);
        assert_eq!(
            declined["error"]["message"], "no route matches GET /nothing-here",
            "declining must not describe Cloud Functions or the deployment's registry"
        );
    }

    #[test]
    #[should_panic(expected = "at most one enabled HTTP adapter may install a router fallback")]
    fn mount_adapters_rejects_two_enabled_fallbacks() {
        let _ = mount_adapters(
            Router::new(),
            vec![
                Box::new(StubAdapter {
                    name: "first",
                    enabled: true,
                    is_fallback: true,
                }),
                Box::new(StubAdapter {
                    name: "second",
                    enabled: true,
                    is_fallback: true,
                }),
            ],
        );
    }

    #[tokio::test]
    async fn mount_adapters_routes_unmatched_paths_to_the_enabled_fallback_when_only_one_is_enabled()
     {
        let (state, _fixture) = test_state();
        let router = mount_adapters(
            Router::new(),
            vec![
                Box::new(StubAdapter {
                    name: "/only-enabled-fallback",
                    enabled: true,
                    is_fallback: true,
                }),
                Box::new(StubAdapter {
                    name: "/disabled-fallback",
                    enabled: false,
                    is_fallback: true,
                }),
            ],
        )
        .with_state(state);

        let (status, body) = get(&router, "/this-path-matches-no-adapter-route").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the enabled fallback-capable adapter must install a router-wide fallback"
        );
        assert_eq!(
            &body[..],
            b"/only-enabled-fallback",
            "an unmatched path must be answered by the enabled fallback adapter, proving the \
             disabled fallback-capable adapter did not (silently) win the fallback slot"
        );
    }
}
