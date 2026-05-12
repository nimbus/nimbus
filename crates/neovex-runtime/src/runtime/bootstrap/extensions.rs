use std::sync::OnceLock;

use deno_fs::sync::MaybeArc;
use deno_web::InMemoryBroadcastChannel;
use sys_traits::impls::RealSys;

use crate::backends::v8::embedder::Extension;
use crate::limits::RuntimeCompatibilityTarget;
use crate::node_compat::{
    ScopedInNpmPackageChecker, ScopedNodeModulesResolver, build_node_init_services,
};
use crate::runtime_capabilities::RuntimePathPolicy;

use super::node22_runtime::node22_runtime_bootstrap_extension;
use super::ops::runtime_extension;
#[cfg(test)]
use super::ops::runtime_test_extension;

fn install_rustls_default_provider_once() {
    static RUSTLS_PROVIDER: OnceLock<()> = OnceLock::new();
    RUSTLS_PROVIDER.get_or_init(|| {
        deno_tls::rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Node22 runtime should install the rustls CryptoProvider once");
    });
}

pub(crate) fn snapshot_extensions(target: RuntimeCompatibilityTarget) -> Vec<Extension> {
    let mut extensions = Vec::new();
    if matches!(target, RuntimeCompatibilityTarget::Node22) {
        install_rustls_default_provider_once();
        extensions.extend([
            deno_webidl::deno_webidl::lazy_init(),
            deno_web::deno_web::lazy_init(),
            deno_crypto::deno_crypto::lazy_init(),
            deno_fetch::deno_fetch::lazy_init(),
            deno_websocket::deno_websocket::lazy_init(),
            deno_telemetry::deno_telemetry::lazy_init(),
            deno_net::deno_net::lazy_init(),
            deno_tls::deno_tls::lazy_init(),
            deno_napi::deno_napi::lazy_init(),
            deno_http::deno_http::lazy_init(),
            deno_io::deno_io::lazy_init(),
            deno_fs::deno_fs::lazy_init(),
            deno_os::deno_os::lazy_init(),
            deno_process::deno_process::lazy_init(),
            deno_node_crypto::deno_node_crypto::lazy_init(),
            deno_node_sqlite::deno_node_sqlite::lazy_init(),
            deno_node::deno_node::lazy_init::<
                ScopedInNpmPackageChecker,
                ScopedNodeModulesResolver,
                RealSys,
            >(),
            node22_runtime_bootstrap_extension(),
        ]);
    }
    extensions.push(runtime_extension());
    #[cfg(test)]
    extensions.push(runtime_test_extension());
    extensions
}

pub(crate) fn execution_extensions(
    target: RuntimeCompatibilityTarget,
    path_policy: &RuntimePathPolicy,
) -> Vec<Extension> {
    let mut extensions = Vec::new();
    if matches!(target, RuntimeCompatibilityTarget::Node22) {
        install_rustls_default_provider_once();
        let fs: deno_fs::FileSystemRc = MaybeArc::new(deno_fs::RealFs);
        extensions.extend([
            deno_webidl::deno_webidl::init(),
            deno_web::deno_web::init(
                Default::default(),
                Default::default(),
                InMemoryBroadcastChannel::default(),
            ),
            deno_crypto::deno_crypto::init(None),
            deno_fetch::deno_fetch::init(Default::default()),
            deno_websocket::deno_websocket::init(),
            deno_telemetry::deno_telemetry::init(),
            deno_net::deno_net::init(None, None),
            deno_tls::deno_tls::init(),
            deno_napi::deno_napi::init(None),
            deno_http::deno_http::init(deno_http::Options::default()),
            deno_io::deno_io::init(Some(Default::default())),
            deno_fs::deno_fs::init(fs.clone()),
            deno_os::deno_os::init(Some(deno_os::ExitCode::default())),
            deno_process::deno_process::init(Default::default()),
            deno_node_crypto::deno_node_crypto::init(),
            deno_node_sqlite::deno_node_sqlite::init(),
            deno_node::deno_node::init::<
                ScopedInNpmPackageChecker,
                ScopedNodeModulesResolver,
                RealSys,
            >(Some(build_node_init_services(path_policy)), fs),
            node22_runtime_bootstrap_extension(),
        ]);
    }
    extensions.push(runtime_extension());
    #[cfg(test)]
    extensions.push(runtime_test_extension());
    extensions
}
