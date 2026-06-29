use std::sync::OnceLock;

use deno_error::JsErrorBox;
use deno_node::ops::module_hooks::LoaderHookRegistry;
use deno_permissions::PermissionsContainer;
use deno_web::InMemoryBroadcastChannel;
use sys_traits::impls::RealSys;

use crate::backends::v8::embedder::Extension;
use crate::egress::{EgressRequest, RuntimeEgressGatewayBinding};
use crate::limits::{RuntimeCompatibilityTarget, RuntimeLimits};
use crate::node_compat::{
    ScopedInNpmPackageChecker, ScopedNodeModulesResolver, build_node_init_services,
};
use crate::runtime_capabilities::RuntimePathPolicy;

use super::node22_runtime::node22_runtime_bootstrap_extension;
#[cfg(test)]
use super::ops::runtime_test_extension;
use super::ops::{runtime_extension, service_extension};
use super::state::{
    InstalledRuntimeEgressGateway, RuntimeInvocationHostCallBinding,
    install_missing_deno_extension_state,
};
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
    file_system: deno_fs::FileSystemRc,
) -> Vec<Extension> {
    RuntimeBootstrapExtensionRegistry::execution_extensions(
        target,
        path_policy,
        loader_hook_registry,
        limits,
        file_system,
    )
}

impl RuntimeBootstrapExtensionRegistry {
    /// Single source of truth for WHICH bootstrap slots a target receives and IN WHAT ORDER:
    /// every web-standard slot on all V8 targets, Node-only slots on Node targets, then (for
    /// non-Node targets) the WebStandard bootstrap entry point. Parameterized by how to render
    /// each entry, so the snapshot path, the execution path, and the test label registry share
    /// ONE gating + ordering and cannot drift on which extensions a profile carries — the exact
    /// property the cross-profile cage fix turns on. The per-path tail (`nimbus_runtime`, the
    /// test marker, optional `service`) differs per caller and is appended by each.
    fn selected_bootstrap_entries<T>(
        target: RuntimeCompatibilityTarget,
        mut render_slot: impl FnMut(NodeBootstrapExtensionSlot) -> T,
        render_web_standard_bootstrap: impl FnOnce() -> T,
    ) -> Vec<T> {
        let node = target.is_node();
        let mut entries = Vec::new();
        // Web standards on ALL V8 targets (preserve slot order so deps stay satisfied);
        // Node-only slots gated on is_node. Egress capability for fetch/WebSocket stays gated at
        // call time by deno_permissions (presence != capability).
        for slot in NODE_BOOTSTRAP_EXTENSION_SLOTS.iter().copied() {
            if node || slot.is_web_standard() {
                entries.push(render_slot(slot));
            }
        }
        // Non-Node V8 targets wire the web globals via the WebStandard bootstrap entry point;
        // Node targets get them from the node22 bootstrap slot above.
        if !node {
            entries.push(render_web_standard_bootstrap());
        }
        entries
    }

    fn snapshot_extensions(
        target: RuntimeCompatibilityTarget,
        service_extension_enabled: bool,
    ) -> Vec<Extension> {
        install_rustls_default_provider_once();
        let mut extensions = Self::selected_bootstrap_entries(
            target,
            NodeBootstrapExtensionSlot::snapshot_extension,
            web_standard_runtime_bootstrap_extension,
        );
        if target.is_node() {
            extensions.push(loader_hook_registry_extension(None));
        }
        extensions.push(deno_extension_state_extension());
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
        file_system: deno_fs::FileSystemRc,
    ) -> Vec<Extension> {
        install_rustls_default_provider_once();
        // Context is consumed only by Node-only slots; web-standard slots ignore it.
        let context = NodeExecutionExtensionContext {
            path_policy,
            limits,
            fs: file_system,
        };
        let mut extensions = Self::selected_bootstrap_entries(
            target,
            |slot| slot.execution_extension(&context),
            web_standard_runtime_bootstrap_extension,
        );
        if let Some(registry) = loader_hook_registry {
            extensions.push(loader_hook_registry_extension(Some(registry)));
        }
        extensions.push(deno_extension_state_extension());
        extensions.push(runtime_extension());
        #[cfg(test)]
        extensions.push(runtime_test_extension());
        if limits.service_capability_enabled && limits.grants.has_service_grants() {
            extensions.push(service_extension());
        }
        extensions
    }

    #[cfg(test)]
    fn snapshot_extension_labels(
        target: RuntimeCompatibilityTarget,
        service_extension_enabled: bool,
    ) -> Vec<&'static str> {
        // Shares selected_bootstrap_entries — the SAME gating + ordering snapshot_extensions()
        // uses — so this registry cannot drift from what production installs. (The prior
        // reconstruction gated on is_node() alone and falsely reported WebStandard as "lean"
        // after the cage fix added fetch/net/websocket to its snapshot.)
        let mut labels =
            Self::selected_bootstrap_entries(target, NodeBootstrapExtensionSlot::label, || {
                web_standard_runtime_bootstrap_extension().name
            });
        if target.is_node() {
            labels.push("nimbus_node_loader_hook_registry_ext");
        }
        labels.push("nimbus_deno_extension_state_ext");
        labels.push("nimbus_runtime");
        labels.push("nimbus_runtime_test");
        if service_extension_enabled {
            labels.push(service_extension().name);
        }
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
            Self::Fetch => deno_fetch::deno_fetch::init(egress_fetch_options()),
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

fn loader_hook_registry_extension(registry: Option<LoaderHookRegistry>) -> Extension {
    Extension {
        name: "nimbus_node_loader_hook_registry_ext",
        op_state_fn: registry.map(|registry| {
            Box::new(move |state: &mut deno_core::OpState| {
                state.put(registry.clone());
            }) as Box<dyn FnOnce(&mut deno_core::OpState)>
        }),
        ..Default::default()
    }
}

fn egress_fetch_options() -> deno_fetch::Options {
    deno_fetch::Options {
        egress_gateway_hook: Some(
            nimbus_fetch_egress_gateway_hook as deno_fetch::FetchEgressGatewayHook,
        ),
        ..Default::default()
    }
}

fn nimbus_fetch_egress_gateway_hook(
    state: &mut deno_core::OpState,
    request: deno_fetch::FetchEgressGatewayRequest<'_>,
) -> Result<deno_fetch::FetchEgressGatewayAuthorization, JsErrorBox> {
    let binding = state
        .try_borrow::<InstalledRuntimeEgressGateway>()
        .map(|installed| installed.binding.clone())
        .unwrap_or(RuntimeEgressGatewayBinding::Missing);
    match binding {
        RuntimeEgressGatewayBinding::CoarsePermissions => state
            .borrow_mut::<PermissionsContainer>()
            .check_net_url(request.url, "fetch()")
            .map(|_| deno_fetch::FetchEgressGatewayAuthorization::use_deno_permissions())
            .map_err(|error| JsErrorBox::generic(error.to_string())),
        RuntimeEgressGatewayBinding::Gateway(gateway) => {
            let invocation = state
                .try_borrow::<RuntimeInvocationHostCallBinding>()
                .cloned();
            let egress_request = EgressRequest::from_fetch_url_with_context(
                request.method.as_str(),
                request.url.as_str(),
                request.client_rid.is_some(),
                invocation
                    .as_ref()
                    .and_then(RuntimeInvocationHostCallBinding::tenant_label)
                    .map(str::to_owned),
                invocation
                    .as_ref()
                    .and_then(RuntimeInvocationHostCallBinding::session_id)
                    .map(str::to_owned),
                invocation
                    .as_ref()
                    .and_then(RuntimeInvocationHostCallBinding::invocation_id),
            )
            .map_err(|error| JsErrorBox::generic(error.to_string()))?;
            let authorization = gateway.authorize(&egress_request);
            if authorization.is_allowed() {
                Ok(deno_fetch::FetchEgressGatewayAuthorization::bypass_deno_permissions())
            } else {
                Err(JsErrorBox::generic(format!(
                    "fetch egress denied: {}",
                    authorization.reason()
                )))
            }
        }
        RuntimeEgressGatewayBinding::Missing => {
            Err(JsErrorBox::generic("fetch egress gateway is not installed"))
        }
    }
}

fn deno_extension_state_extension() -> Extension {
    Extension {
        name: "nimbus_deno_extension_state_ext",
        op_state_fn: Some(Box::new(install_missing_deno_extension_state)),
        ..Default::default()
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
    fn web_standard_snapshot_registry_carries_web_extensions_not_node_internals() {
        let labels = RuntimeBootstrapExtensionRegistry::snapshot_extension_labels(
            RuntimeCompatibilityTarget::WebStandardIsolate,
            false,
        );
        // WebStandard's snapshot is NOT "lean": it carries the full web-standard slot set
        // (including the egress-capable fetch/websocket/net/tls), the WebStandard bootstrap, and
        // the nimbus runtime tail — exactly what snapshot_extensions() installs. Presence is
        // required; egress CAPABILITY stays gated at call time by deno_permissions
        // (deny-by-default). Asserting the real set keeps the security signal honest (the prior
        // assertion claimed "lean runtime only", which became false and silently stayed green).
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
                "nimbus_web_standard_runtime_bootstrap_ext",
                "nimbus_deno_extension_state_ext",
                "nimbus_runtime",
                "nimbus_runtime_test",
            ]
        );
        // The web-standard profile must NOT carry Node-only host-access internals.
        for node_only in [
            "napi",
            "http",
            "io",
            "fs",
            "os",
            "process",
            "node",
            "node_runtime_bootstrap",
        ] {
            assert!(
                !labels.contains(&node_only),
                "WebStandard snapshot must not carry Node-only `{node_only}`; got {labels:?}"
            );
        }
    }

    #[test]
    fn node_snapshot_registry_extends_ordered_node_slots() {
        let labels = RuntimeBootstrapExtensionRegistry::snapshot_extension_labels(
            RuntimeCompatibilityTarget::Node22,
            false,
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
            [
                "nimbus_node_loader_hook_registry_ext",
                "nimbus_deno_extension_state_ext",
                "nimbus_runtime",
                "nimbus_runtime_test"
            ]
        );
    }

    #[test]
    fn fetch_execution_extension_installs_egress_gateway_hook() {
        let options = egress_fetch_options();

        assert!(
            options.egress_gateway_hook.is_some(),
            "fetch must consult the runtime EgressGateway hook before falling back to net permissions"
        );
    }
}
