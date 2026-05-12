use crate::backends::v8::embedder::{Extension, extension};

extension!(
    neovex_node22_runtime_bootstrap_ext,
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
    esm_entry_point = "ext:neovex_node22/runtime_bootstrap.js",
    esm = [
        "ext:runtime/01_errors.js" = "src/runtime/bootstrap/js/01_errors.js",
        "ext:runtime/98_global_scope_shared.js" =
            "src/runtime/bootstrap/js/98_global_scope_shared.js",
        "ext:neovex_node22/perf_hooks_impl.js" = "src/runtime/bootstrap/js/perf_hooks.js",
        "ext:neovex_node22/internal_bootstrap.js" =
            "src/runtime/bootstrap/js/node22_internal_bootstrap.js",
        "ext:neovex_node22/runtime_bootstrap.js" =
            "src/runtime/bootstrap/js/node22_runtime_bootstrap.js"
    ],
);

pub(crate) fn node22_runtime_bootstrap_extension() -> Extension {
    neovex_node22_runtime_bootstrap_ext::init()
}
