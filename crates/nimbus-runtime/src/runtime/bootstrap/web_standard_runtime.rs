use std::borrow::Cow;

use crate::backends::v8::embedder::{Extension, ExtensionFileSource, ascii_str_include, extension};

extension!(
    nimbus_web_standard_runtime_bootstrap_ext,
    deps = [
        deno_telemetry,
        deno_webidl,
        deno_web,
        deno_crypto,
        deno_fetch,
        deno_websocket,
        deno_net,
        deno_tls
    ],
    esm_entry_point = "ext:nimbus_web/runtime_bootstrap.js",
    esm = [
        "ext:runtime/98_global_scope_shared.js" =
            "src/runtime/bootstrap/js/98_global_scope_shared.js",
        "ext:nimbus_web/runtime_bootstrap.js" = "src/runtime/bootstrap/js/web_runtime_bootstrap.js"
    ],
);

const WEB_STANDARD_RUNTIME_BOOTSTRAP_ESM: &[ExtensionFileSource] = &[
    ExtensionFileSource::new(
        "ext:runtime/98_global_scope_shared.js",
        ascii_str_include!("js/98_global_scope_shared.js"),
    ),
    ExtensionFileSource::new(
        "ext:nimbus_web/runtime_bootstrap.js",
        ascii_str_include!("js/web_runtime_bootstrap.js"),
    ),
];

/// WebStandard bootstrap: evaluates the shared WindowOrWorkerGlobalScope mixin (the web
/// standards: Encoding/URL/Streams/WebCrypto/Fetch/timers) on a non-Node V8 target, with no
/// Node setup. Deps are the web-standards extension slots only.
pub(crate) fn web_standard_runtime_bootstrap_extension() -> Extension {
    let mut extension = nimbus_web_standard_runtime_bootstrap_ext::init();
    extension.esm_files = Cow::Borrowed(WEB_STANDARD_RUNTIME_BOOTSTRAP_ESM);
    extension
}
