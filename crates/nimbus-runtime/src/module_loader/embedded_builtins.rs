pub(super) const NODE_FS_SPECIFIER: &str = "node:fs";
pub(super) const NIMBUS_NODE_FS_SPECIFIER: &str = "nimbus:node/fs";
pub(super) const NODE_FS_PROMISES_SPECIFIER: &str = "node:fs/promises";
pub(super) const NIMBUS_NODE_FS_PROMISES_SPECIFIER: &str = "nimbus:node/fs/promises";
pub(super) const NODE_MODULE_SPECIFIER: &str = "node:module";
pub(super) const NIMBUS_NODE_MODULE_SPECIFIER: &str = "node:nimbus/module";
pub(super) const INTERNAL_READLINE_UTILS_SPECIFIER: &str = "internal/readline/utils";
pub(super) const NIMBUS_INTERNAL_READLINE_UTILS_SPECIFIER: &str = "nimbus:internal/readline/utils";

const NODE_PERF_HOOKS_SPECIFIER: &str = "node:perf_hooks";
const NODE_TLS_SPECIFIER: &str = "node:tls";
const NODE_PERF_HOOKS_MODULE_SOURCE: &str = include_str!("builtins/perf_hooks.js");
const NODE_TLS_MODULE_SOURCE: &str = include_str!("builtins/tls.js");
const INTERNAL_READLINE_UTILS_MODULE_SOURCE: &str =
    include_str!("builtins/internal_readline_utils.js");
const NODE_FS_MODULE_SOURCE: &str = include_str!("builtins/fs.js");
const NODE_FS_PROMISES_MODULE_SOURCE: &str = include_str!("builtins/fs_promises.js");
const NODE_MODULE_MODULE_SOURCE: &str = concat!(
    include_str!("builtins/module_prelude.js"),
    include_str!("builtins/module_fs_helpers.js"),
    include_str!("builtins/module_fs_modules.js"),
    include_str!("builtins/module_wiring.js"),
);

pub(super) fn source_for_supported_node_builtin(
    specifier: &str,
    node_compat_enabled: bool,
) -> Option<&'static str> {
    if !node_compat_enabled {
        return None;
    }
    match specifier {
        NODE_FS_SPECIFIER | NIMBUS_NODE_FS_SPECIFIER => Some(NODE_FS_MODULE_SOURCE),
        NODE_TLS_SPECIFIER => Some(NODE_TLS_MODULE_SOURCE),
        NODE_MODULE_SPECIFIER | NIMBUS_NODE_MODULE_SPECIFIER => Some(NODE_MODULE_MODULE_SOURCE),
        NODE_FS_PROMISES_SPECIFIER | NIMBUS_NODE_FS_PROMISES_SPECIFIER => {
            Some(NODE_FS_PROMISES_MODULE_SOURCE)
        }
        INTERNAL_READLINE_UTILS_SPECIFIER | NIMBUS_INTERNAL_READLINE_UTILS_SPECIFIER => {
            Some(INTERNAL_READLINE_UTILS_MODULE_SOURCE)
        }
        NODE_PERF_HOOKS_SPECIFIER => Some(NODE_PERF_HOOKS_MODULE_SOURCE),
        _ => None,
    }
}

pub(super) fn supports_extension_backed_node_builtin(
    specifier: &str,
    node_compat_enabled: bool,
) -> bool {
    if !node_compat_enabled {
        return false;
    }
    matches!(
        specifier,
        "node:assert"
            | "node:assert/strict"
            | "node:buffer"
            | "node:console"
            | "node:events"
            | "node:path"
            | "node:path/posix"
            | "node:path/win32"
            | "node:punycode"
            | "node:querystring"
            | "node:string_decoder"
            | "node:test"
            | "node:test/reporters"
            | "node:url"
            | "node:process"
            | "node:timers"
            | "node:timers/promises"
            | "node:util"
            | "node:diagnostics_channel"
            | "node:perf_hooks"
            | "node:os"
            | "node:tty"
            | "node:stream"
            | "node:stream/consumers"
            | "node:stream/promises"
            | "node:stream/web"
            | "node:dns"
            | "node:net"
            | "node:dgram"
            | "node:tls"
            | "node:http"
            | "node:https"
            | "node:http2"
            | "node:child_process"
            | "node:crypto"
            | "node:worker_threads"
    )
}
