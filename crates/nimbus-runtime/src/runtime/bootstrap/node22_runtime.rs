use std::borrow::Cow;

use crate::backends::v8::embedder::{Extension, ExtensionFileSource, ascii_str_include, extension};

extension!(
    nimbus_node22_runtime_bootstrap_ext,
    deps = [
        deno_webidl,
        deno_web,
        deno_crypto,
        deno_fetch,
        deno_websocket,
        deno_net,
        deno_tls,
        deno_napi,
        deno_http,
        deno_io,
        deno_fs,
        deno_os,
        deno_process,
        deno_node_crypto,
        deno_node
    ],
    esm_entry_point = "ext:nimbus_node22/runtime_bootstrap.js",
    esm = [
        "ext:runtime/01_errors.js" = "src/runtime/bootstrap/js/01_errors.js",
        "ext:runtime/98_global_scope_shared.js" =
            "src/runtime/bootstrap/js/98_global_scope_shared.js",
        "ext:nimbus_node22/internal_bootstrap.js" =
            "src/runtime/bootstrap/js/node22_internal_bootstrap.js",
        "ext:nimbus_node22/runtime_bootstrap.js" =
            "src/runtime/bootstrap/js/node22_runtime_bootstrap.js"
    ],
    lazy_loaded_esm =
        ["ext:nimbus_node22/perf_hooks_impl.js" = "src/runtime/bootstrap/js/perf_hooks.js"],
);

const NODE22_RUNTIME_BOOTSTRAP_ESM: &[ExtensionFileSource] = &[
    ExtensionFileSource::new(
        "ext:runtime/01_errors.js",
        ascii_str_include!("js/01_errors.js"),
    ),
    ExtensionFileSource::new(
        "ext:runtime/98_global_scope_shared.js",
        ascii_str_include!("js/98_global_scope_shared.js"),
    ),
    ExtensionFileSource::new(
        "ext:nimbus_node22/internal_bootstrap.js",
        ascii_str_include!("js/node22_internal_bootstrap.js"),
    ),
    ExtensionFileSource::new(
        "ext:nimbus_node22/runtime_bootstrap.js",
        ascii_str_include!("js/node22_runtime_bootstrap.js"),
    ),
];

pub(crate) fn node22_runtime_bootstrap_extension() -> Extension {
    let mut extension = nimbus_node22_runtime_bootstrap_ext::init();
    extension.esm_files = Cow::Borrowed(NODE22_RUNTIME_BOOTSTRAP_ESM);
    extension
}
