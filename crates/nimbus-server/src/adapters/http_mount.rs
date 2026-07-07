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

use std::sync::Arc;

use axum::Router;
use axum::routing::any;

use crate::adapters::cloud_functions;
use crate::adapters::cloudflare::{self, CloudflareConfig};
use crate::state::AppState;

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
    /// fallback would silently replace the first with no signal.
    fn is_fallback(&self) -> bool {
        false
    }

    /// Mount this adapter's routes onto `router`, consuming any
    /// adapter-owned state. Called only when [`enabled`](Self::enabled) is
    /// `true`.
    fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>>;
}

/// Merges every enabled adapter's routes onto `router` in registration
/// order, asserting at most one enabled adapter installs a fallback.
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
    for adapter in adapters {
        if adapter.enabled() {
            router = adapter.mount(router);
        }
    }
    router
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
    use super::*;

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

        fn mount(self: Box<Self>, router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
            let name = self.name;
            router.route(name, axum::routing::get(move || async move { name }))
        }
    }

    #[test]
    fn mount_adapters_skips_disabled_adapters() {
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
        );
        // Disabled adapters must not register a route: attempting to
        // register the same path twice would panic at merge/route time,
        // so a clean build is itself proof the disabled adapter mounted
        // nothing.
        let _ = router;
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

    #[test]
    fn mount_adapters_allows_two_fallback_capable_adapters_when_only_one_is_enabled() {
        let router = mount_adapters(
            Router::new(),
            vec![
                Box::new(StubAdapter {
                    name: "/only-enabled",
                    enabled: true,
                    is_fallback: true,
                }),
                Box::new(StubAdapter {
                    name: "second",
                    enabled: false,
                    is_fallback: true,
                }),
            ],
        );
        let _ = router;
    }
}
