use std::sync::OnceLock;

use deno_fs::sync::MaybeArc;
use deno_node::ops::module_hooks::LoaderHookRegistry;
use deno_web::InMemoryBroadcastChannel;
use sys_traits::impls::RealSys;

use crate::backends::v8::embedder::Extension;
use crate::limits::{RuntimeCompatibilityTarget, RuntimeLimits};
use crate::node_compat::{
    ScopedInNpmPackageChecker, ScopedNodeModulesResolver, build_node_init_services,
};
use crate::runtime_capabilities::RuntimePathPolicy;

use super::node22_runtime::node22_runtime_bootstrap_extension;
#[cfg(test)]
use super::ops::runtime_test_extension;
use super::ops::{runtime_extension, service_extension};
use super::web_standard_runtime::web_standard_runtime_bootstrap_extension;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeBootstrapExtensionSlot {
    Telemetry,
    WebIdl,
    Web,
    Crypto,
    Fetch,
    WebSocket,
    Net,
    Tls,
    Napi,
    Http,
    Io,
    Fs,
    Os,
    Process,
    NodeCrypto,
    NodeSqlite,
    Node,
    NodeRuntimeBootstrap,
}

const NODE_BOOTSTRAP_EXTENSION_SLOTS: &[NodeBootstrapExtensionSlot] = &[
    NodeBootstrapExtensionSlot::Telemetry,
    NodeBootstrapExtensionSlot::WebIdl,
    NodeBootstrapExtensionSlot::Web,
    NodeBootstrapExtensionSlot::Crypto,
    NodeBootstrapExtensionSlot::Fetch,
    NodeBootstrapExtensionSlot::WebSocket,
    NodeBootstrapExtensionSlot::Net,
    NodeBootstrapExtensionSlot::Tls,
    NodeBootstrapExtensionSlot::Napi,
    NodeBootstrapExtensionSlot::Http,
    NodeBootstrapExtensionSlot::Io,
    NodeBootstrapExtensionSlot::Fs,
    NodeBootstrapExtensionSlot::Os,
    NodeBootstrapExtensionSlot::Process,
    NodeBootstrapExtensionSlot::NodeCrypto,
    NodeBootstrapExtensionSlot::NodeSqlite,
    NodeBootstrapExtensionSlot::Node,
    NodeBootstrapExtensionSlot::NodeRuntimeBootstrap,
];

struct RuntimeBootstrapExtensionRegistry;

struct NodeExecutionExtensionContext<'a> {
    path_policy: &'a RuntimePathPolicy,
    loader_hook_registry: Option<LoaderHookRegistry>,
    limits: &'a RuntimeLimits,
    fs: deno_fs::FileSystemRc,
}

fn install_rustls_default_provider_once() {
    static RUSTLS_PROVIDER: OnceLock<()> = OnceLock::new();
    RUSTLS_PROVIDER.get_or_init(|| {
        deno_tls::rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Node-compatible runtime should install the rustls CryptoProvider once");
    });
}

pub(crate) fn snapshot_extensions(
    target: RuntimeCompatibilityTarget,
    service_extension_enabled: bool,
) -> Vec<Extension> {
    RuntimeBootstrapExtensionRegistry::snapshot_extensions(target, service_extension_enabled)
}

pub(crate) fn execution_extensions(
    target: RuntimeCompatibilityTarget,
    path_policy: &RuntimePathPolicy,
    loader_hook_registry: Option<LoaderHookRegistry>,
    limits: &RuntimeLimits,
) -> Vec<Extension> {
    RuntimeBootstrapExtensionRegistry::execution_extensions(
        target,
        path_policy,
        loader_hook_registry,
        limits,
    )
}

impl RuntimeBootstrapExtensionRegistry {
    fn snapshot_extensions(
        target: RuntimeCompatibilityTarget,
        service_extension_enabled: bool,
    ) -> Vec<Extension> {
        let node = target.is_node();
        install_rustls_default_provider_once();
        let mut extensions = Vec::new();
        // Web standards on ALL V8 targets (preserve slot order so deps stay satisfied);
        // Node-only slots gated on is_node.
        for slot in NODE_BOOTSTRAP_EXTENSION_SLOTS.iter().copied() {
            if node || slot.is_web_standard() {
                extensions.push(slot.snapshot_extension());
            }
        }
        if !node {
            extensions.push(web_standard_runtime_bootstrap_extension());
        }
        extensions.push(runtime_extension());
        #[cfg(test)]
        extensions.push(runtime_test_extension());
        if service_extension_enabled {
            extensions.push(service_extension());
        }
        extensions
    }

    fn execution_extensions(
        target: RuntimeCompatibilityTarget,
        path_policy: &RuntimePathPolicy,
        loader_hook_registry: Option<LoaderHookRegistry>,
        limits: &RuntimeLimits,
    ) -> Vec<Extension> {
        let node = target.is_node();
        install_rustls_default_provider_once();
        // Context is consumed only by Node-only slots; web-standard slots ignore it.
        let context = NodeExecutionExtensionContext {
            path_policy,
            loader_hook_registry,
            limits,
            fs: MaybeArc::new(deno_fs::RealFs),
        };
        let mut extensions = Vec::new();
        // Web standards on ALL V8 targets (preserve slot order so deps stay satisfied);
        // Node-only slots gated on is_node. Egress capability for fetch/WebSocket stays
        // gated at call time by deno_permissions (presence != capability).
        for slot in NODE_BOOTSTRAP_EXTENSION_SLOTS.iter().copied() {
            if node || slot.is_web_standard() {
                extensions.push(slot.execution_extension(&context));
            }
        }
        // Non-Node V8 targets wire the web globals (TextEncoder/URL/Streams/WebCrypto/Fetch)
        // via the WebStandard bootstrap entry point; Node targets get them from node22.
        if !node {
            extensions.push(web_standard_runtime_bootstrap_extension());
        }
        extensions.push(runtime_extension());
        #[cfg(test)]
        extensions.push(runtime_test_extension());
        if limits.service_capability_enabled && limits.grants.has_service_grants() {
            extensions.push(service_extension());
        }
        extensions
    }

    #[cfg(test)]
    fn snapshot_extension_labels(target: RuntimeCompatibilityTarget) -> Vec<&'static str> {
        let mut labels = Vec::new();
        if target.is_node() {
            labels.extend(
                NODE_BOOTSTRAP_EXTENSION_SLOTS
                    .iter()
                    .copied()
                    .map(NodeBootstrapExtensionSlot::label),
            );
        }
        labels.push("nimbus_runtime");
        labels.push("nimbus_runtime_test");
        labels
    }
}

impl NodeBootstrapExtensionSlot {
    fn snapshot_extension(self) -> Extension {
        match self {
            Self::Telemetry => deno_telemetry::deno_telemetry::lazy_init(),
            Self::WebIdl => deno_webidl::deno_webidl::lazy_init(),
            Self::Web => deno_web::deno_web::lazy_init(),
            Self::Crypto => deno_crypto::deno_crypto::lazy_init(),
            Self::Fetch => deno_fetch::deno_fetch::lazy_init(),
            Self::WebSocket => deno_websocket::deno_websocket::lazy_init(),
            Self::Net => deno_net::deno_net::lazy_init(),
            Self::Tls => deno_tls::deno_tls::lazy_init(),
            Self::Napi => deno_napi::deno_napi::lazy_init(),
            Self::Http => deno_http::deno_http::lazy_init(),
            Self::Io => deno_io::deno_io::lazy_init(),
            Self::Fs => deno_fs::deno_fs::lazy_init(),
            Self::Os => deno_os::deno_os::lazy_init(),
            Self::Process => deno_process::deno_process::lazy_init(),
            Self::NodeCrypto => deno_node_crypto::deno_node_crypto::lazy_init(),
            Self::NodeSqlite => deno_node_sqlite::deno_node_sqlite::lazy_init(),
            Self::Node => deno_node::deno_node::lazy_init::<
                ScopedInNpmPackageChecker,
                ScopedNodeModulesResolver,
                RealSys,
            >(),
            Self::NodeRuntimeBootstrap => node22_runtime_bootstrap_extension(),
        }
    }

    fn execution_extension(self, context: &NodeExecutionExtensionContext<'_>) -> Extension {
        match self {
            Self::Telemetry => deno_telemetry::deno_telemetry::init(),
            Self::WebIdl => deno_webidl::deno_webidl::init(),
            Self::Web => deno_web::deno_web::init(
                deno_web::BlobStore::default_arc(),
                Default::default(),
                false,
                InMemoryBroadcastChannel::default(),
            ),
            Self::Crypto => deno_crypto::deno_crypto::init(None),
            Self::Fetch => deno_fetch::deno_fetch::init(Default::default()),
            Self::WebSocket => deno_websocket::deno_websocket::init(),
            Self::Net => deno_net::deno_net::init(None, None),
            Self::Tls => deno_tls::deno_tls::init(),
            Self::Napi => deno_napi::deno_napi::init(None),
            Self::Http => deno_http::deno_http::init(deno_http::Options::default()),
            Self::Io => deno_io::deno_io::init(Some(Default::default())),
            Self::Fs => deno_fs::deno_fs::init(context.fs.clone()),
            Self::Os => deno_os::deno_os::init(Some(deno_os::ExitCode::default())),
            Self::Process => deno_process::deno_process::init(Default::default()),
            Self::NodeCrypto => deno_node_crypto::deno_node_crypto::init(),
            Self::NodeSqlite => deno_node_sqlite::deno_node_sqlite::init(),
            Self::Node => deno_node::deno_node::init::<
                ScopedInNpmPackageChecker,
                ScopedNodeModulesResolver,
                RealSys,
            >(
                Some(build_node_init_services(
                    context.path_policy,
                    context.loader_hook_registry.clone(),
                    &context.limits.node_conditions,
                )),
                context.fs.clone(),
            ),
            Self::NodeRuntimeBootstrap => node22_runtime_bootstrap_extension(),
        }
    }

    /// W3C/WHATWG STANDARDS extensions (Encoding/TextEncoder, URL, Streams, WebCrypto,
    /// Fetch, WebSocket) — present on ALL V8 targets, not just Node. These are not
    /// Node-specific; gating them on `is_node()` left `WebStandardIsolate` with no
    /// web-standard APIs (confirmed bug). PRESENCE only — egress-bearing surfaces
    /// (fetch/WebSocket) come up present and are gated at call time by the EXISTING
    /// deno_permissions path (deny-by-default), identical to Node. Deps are self-contained
    /// (webidl <- web <- crypto/fetch/websocket; fetch deps = [webidl, web], no net/tls).
    fn is_web_standard(self) -> bool {
        matches!(
            self,
            // Telemetry is not itself a web standard, but the shared global-scope module
            // (98_global_scope_shared.js) loads ext:deno_telemetry/{telemetry,util}.ts, so it
            // is required for any V8 target that evaluates that scope. Net+Tls are fetch's
            // transport (deno_fetch/22_http_client.js loads ext:deno_net/02_tls.js); fetch is
            // present-and-deny-by-default, so the transport is permission-gated, not granted.
            Self::Telemetry
                | Self::WebIdl
                | Self::Web
                | Self::Crypto
                | Self::Fetch
                | Self::WebSocket
                | Self::Net
                | Self::Tls
        )
    }

    #[cfg(test)]
    fn label(self) -> &'static str {
        match self {
            Self::Telemetry => "telemetry",
            Self::WebIdl => "webidl",
            Self::Web => "web",
            Self::Crypto => "crypto",
            Self::Fetch => "fetch",
            Self::WebSocket => "websocket",
            Self::Net => "net",
            Self::Tls => "tls",
            Self::Napi => "napi",
            Self::Http => "http",
            Self::Io => "io",
            Self::Fs => "fs",
            Self::Os => "os",
            Self::Process => "process",
            Self::NodeCrypto => "node_crypto",
            Self::NodeSqlite => "node_sqlite",
            Self::Node => "node",
            Self::NodeRuntimeBootstrap => "node_runtime_bootstrap",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_extension_registry_is_single_ordered_source() {
        let labels = NODE_BOOTSTRAP_EXTENSION_SLOTS
            .iter()
            .map(|slot| slot.label())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "telemetry",
                "webidl",
                "web",
                "crypto",
                "fetch",
                "websocket",
                "net",
                "tls",
                "napi",
                "http",
                "io",
                "fs",
                "os",
                "process",
                "node_crypto",
                "node_sqlite",
                "node",
                "node_runtime_bootstrap",
            ]
        );
    }

    #[test]
    fn web_standard_snapshot_registry_is_lean_runtime_only() {
        assert_eq!(
            RuntimeBootstrapExtensionRegistry::snapshot_extension_labels(
                RuntimeCompatibilityTarget::WebStandardIsolate
            ),
            vec!["nimbus_runtime", "nimbus_runtime_test"]
        );
    }

    #[test]
    fn node_snapshot_registry_extends_ordered_node_slots() {
        let labels = RuntimeBootstrapExtensionRegistry::snapshot_extension_labels(
            RuntimeCompatibilityTarget::Node22,
        );
        assert_eq!(
            &labels[..NODE_BOOTSTRAP_EXTENSION_SLOTS.len()],
            NODE_BOOTSTRAP_EXTENSION_SLOTS
                .iter()
                .map(|slot| slot.label())
                .collect::<Vec<_>>()
                .as_slice()
        );
        assert_eq!(
            &labels[NODE_BOOTSTRAP_EXTENSION_SLOTS.len()..],
            ["nimbus_runtime", "nimbus_runtime_test"]
        );
    }
}
