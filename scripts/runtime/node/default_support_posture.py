#!/usr/bin/env python3
"""Generate the Node default-support posture overlay for NDS.

The existing lane classification catalogs intentionally describe "what is not
green yet" for the broad official fixture corpus. NDS needs a second,
default-support-specific denominator that explains which of those gaps are
required V8-isolate support, optional V8-isolate support, diagnostic non-isolate
behavior, harness-only, or upstream/platform boundary.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any


DENOMINATORS = [
    "v8_isolate_required",
    "v8_isolate_optional",
    "diagnostic_only_non_isolate",
    "test_harness_only",
    "upstream_or_platform_boundary",
]

SHIM_CLASSES = [
    "native_isolate",
    "compatibility_shim",
    "isolate_emulation",
    "test_harness_emulation",
    "diagnostic_stub",
    "unsupported",
]

CATEGORY_KEYWORDS = {
    "v8_isolate_required": [
        "assert",
        "async",
        "async-hooks",
        "abort",
        "blob",
        "buffer",
        "console",
        "constants",
        "crypto",
        "diagnostics",
        "domain",
        "encoding",
        "errors",
        "event",
        "fs-promises",
        "global",
        "http",
        "https",
        "module",
        "path",
        "perf",
        "process",
        "promise",
        "querystring",
        "stream",
        "string",
        "timers",
        "tls",
        "trace",
        "url",
        "util",
        "v8",
        "vm",
        "webcrypto",
        "whatwg",
        "zlib",
    ],
    "diagnostic_only_non_isolate": [
        "child-process",
        "cluster",
        "debugger",
        "dgram",
        "fs-watch",
        "inspector",
        "native",
        "net",
        "pipe",
        "signal",
        "socket",
        "udp",
        "unix",
        "worker",
    ],
    "test_harness_only": [
        "benchmark",
        "fixtures",
        "node-test",
        "pummel",
        "repl",
        "report",
        "test-runner",
        "tty",
        "wpt",
    ],
}

HOST_PROCESS_CONTROL_PATHS = {
    "test/abort/test-process-abort-exitcode.js",
    "test/parallel/test-process-dlopen-error-message-crash.js",
    "test/parallel/test-process-dlopen-undefined-exports.js",
    "test/parallel/test-process-euid-egid.js",
    "test/parallel/test-process-external-stdio-close-spawn.js",
    "test/parallel/test-process-external-stdio-close.js",
    "test/parallel/test-process-getgroups.js",
    "test/parallel/test-process-initgroups.js",
    "test/parallel/test-process-kill-null.js",
    "test/parallel/test-process-kill-pid.js",
    "test/parallel/test-process-raw-debug.js",
    "test/parallel/test-process-really-exit.js",
    "test/parallel/test-process-redirect-warnings-env.js",
    "test/parallel/test-process-redirect-warnings.js",
    "test/parallel/test-process-setgroups.js",
    "test/parallel/test-process-title-cli.js",
    "test/parallel/test-process-uid-gid.js",
    "test/parallel/test-windows-abort-exitcode.js",
}

HOST_PROCESS_CONTROL_PREFIXES = (
    "test/abort/",
    "test/parallel/test-process-execve",
)

NODE_CLI_TOPOLOGY_PATHS = {
    "test/client-proxy/test-use-env-proxy-cli-http.mjs",
    "test/client-proxy/test-use-env-proxy-cli-https.mjs",
    "test/parallel/test-cli-eval-event.js",
    "test/parallel/test-cli-print-promise.mjs",
    "test/parallel/test-debug-process.js",
    "test/parallel/test-preload-print-process-argv.js",
    "test/parallel/test-set-process-debug-port.js",
    "test/parallel/test-stream-preprocess.js",
    "test/parallel/test-tick-processor-arguments.js",
    "test/parallel/test-tick-processor-version-check.js",
}

NODE_CLI_TOPOLOGY_PREFIXES = (
    "test/tick-processor/",
)

# NDS3 post-2000 required-surface denominator cleanup. The keyword catch-all in
# classify_entry lands any path containing "process", "module", "util", "async",
# etc. in v8_isolate_required, which over-counts fixtures that are categorically
# not public Application API. Each set below was confirmed by reading the fixture
# source: node-api fixtures load build/<type>/*.node native addons; test-eslint-*
# drive tools/eslint-rules RuleTester via skipIfEslintMissing; test-snapshot-*
# build a V8 startup snapshot through the common/snapshot CLI subprocess;
# test-bootstrap-modules asserts Node's exact internal moduleLoadList; node:sqlite
# is a native-backed builtin; and the expose-internals set carries
# `// Flags: --expose-internals` + require('internal/*'). test-internal-process-
# binding.js is deliberately NOT listed: it has no --expose-internals flag and
# asserts public process.binding() throw behavior, so it stays a promotable
# v8_isolate_required gap rather than a private-internals reclassification.
NATIVE_ADDON_NODE_API_PREFIXES = (
    "test/node-api/",
    "test/js-native-api/",
)

NODE_LINT_RULE_HARNESS_PREFIX = "test/parallel/test-eslint-"

STARTUP_SNAPSHOT_CLI_PATHS = {
    "test/parallel/test-snapshot-console.js",
    "test/parallel/test-snapshot-dns-lookup-localhost-promise.js",
    "test/parallel/test-snapshot-dns-resolve-localhost-promise.js",
    "test/parallel/test-snapshot-stack-trace-limit-mutation.js",
    "test/parallel/test-snapshot-stack-trace-limit.js",
}

INTERNAL_BOOTSTRAP_TOPOLOGY_PATHS = {
    "test/parallel/test-bootstrap-modules.js",
}

NATIVE_BACKED_OPTIONAL_BUILTIN_PATHS = {
    "test/parallel/test-sqlite.js",
}

EXPOSE_INTERNALS_PRIVATE_MODULE_PATHS = {
    "test/parallel/test-internal-assert.js",
    "test/parallel/test-internal-async-context-frame-disable.js",
    "test/parallel/test-internal-async-context-frame-enabled.js",
    "test/parallel/test-internal-encoding-binding.js",
    "test/parallel/test-internal-errors.js",
    "test/parallel/test-internal-fs-syncwritestream.js",
    "test/parallel/test-internal-module-require.js",
    "test/parallel/test-internal-module-wrap.js",
    "test/parallel/test-internal-util-assertCrypto.js",
    "test/parallel/test-internal-util-classwrapper.js",
    "test/parallel/test-internal-util-construct-sab.js",
    "test/parallel/test-internal-util-decorate-error-stack.js",
    "test/parallel/test-internal-util-getCIDR.js",
    "test/parallel/test-internal-util-helpers.js",
    "test/parallel/test-internal-util-isinsidenodemodules.js",
    "test/parallel/test-internal-util-objects.js",
    "test/parallel/test-internal-webidl-buffer-source.js",
}

# NDS3 second denominator cleanup wave (workflow wf_54971f1d-74d): per-fixture
# evidence-classified, adversarially verified, and lead spot-checked. Each path
# below carries a confirmed BLOCKING signal that prevents the whole fixture from
# running as default-isolate public Application API (top-level
# require('internal/*'), spawned execPath child, --permission gate,
# --allow-natives-syntax/platform skip, native addon, SEA, or doc harness). Some
# are MIXED fixtures (e.g. test-process-env, test-blob, test-eventtarget,
# test-esm-import-meta-resolve) that also contain public-API assertions; reclassi-
# fying the whole fixture is defensible only because the blocking tail gates the
# file and the related public surfaces remain visible elsewhere as required gaps.
# Keyed by (support_denominator, reason_code, shim_classification).
NDS3_WAVE2_RECLASSIFICATIONS = {
    ('diagnostic_only_non_isolate', 'child_process_host_output_topology', 'diagnostic_stub'): frozenset({
        'test/parallel/test-domain-top-level-error-handler-throw.js',
        'test/parallel/test-domain-uncaught-exception.js',
        'test/parallel/test-trace-atomic-deprecation.js',
        'test/parallel/test-trace-atomics-wait.js',
        'test/parallel/test-trace-events-vm.js',
        'test/parallel/test-v8-coverage.js',
        'test/parallel/test-v8-stop-coverage.js',
        'test/parallel/test-v8-take-coverage-noop.js',
        'test/parallel/test-v8-take-coverage.js',
        'test/parallel/test-vm-api-handles-getter-errors.js',
        'test/parallel/test-vm-cached-data.js',
        'test/parallel/test-vm-syntax-error-message.js',
        'test/parallel/test-vm-syntax-error-stderr.js',
        'test/v8-updates/test-trace-gc-flag.js',
    }),
    ('diagnostic_only_non_isolate', 'exact_host_process_control_surface', 'diagnostic_stub'): frozenset({
        'test/es-module/test-vm-main-context-default-loader-eval.js',
        'test/es-module/test-vm-main-context-default-loader.js',
        'test/parallel/test-domain-abort-on-uncaught.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-0.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-1.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-2.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-3.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-4.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-5.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-6.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-7.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-8.js',
        'test/parallel/test-domain-no-error-handler-abort-on-uncaught-9.js',
        'test/parallel/test-domain-throw-error-then-throw-from-uncaught-exception-handler.js',
        'test/parallel/test-domain-with-abort-on-uncaught-exception.js',
        'test/parallel/test-fs-write-sigxfsz.js',
        'test/parallel/test-os-homedir-no-envvar.js',
        'test/parallel/test-os-process-priority.js',
        'test/parallel/test-process-argv-0.js',
        'test/parallel/test-process-chdir.js',
        'test/parallel/test-process-env.js',
        'test/parallel/test-process-execpath.js',
        'test/parallel/test-process-exit-code.js',
        'test/parallel/test-process-getactivehandles.js',
        'test/parallel/test-process-getactiverequests.js',
        'test/parallel/test-process-ppid.js',
        'test/parallel/test-process-warnings.mjs',
        'test/parallel/test-vm-sigint-existing-handler.js',
        'test/parallel/test-vm-sigint.js',
    }),
    ('diagnostic_only_non_isolate', 'host_owned_non_isolate_harness', 'diagnostic_stub'): frozenset({
        'test/parallel/test-domain-dep0097.js',
        'test/parallel/test-module-print-timing.mjs',
    }),
    ('diagnostic_only_non_isolate', 'host_owned_permission_policy', 'diagnostic_stub'): frozenset({
        'test/es-module/test-cjs-legacyMainResolve-permission.js',
        'test/parallel/test-permission-fs-absolute-path.js',
        'test/parallel/test-permission-fs-internal-module-stat.js',
        'test/parallel/test-permission-fs-relative-path.js',
        'test/parallel/test-permission-fs-repeat-path.js',
        'test/parallel/test-permission-fs-traversal-path.js',
        'test/parallel/test-permission-fs-windows-path.js',
        'test/parallel/test-permission-fs-write-v8.js',
        'test/parallel/test-permission-processbinding.js',
    }),
    ('test_harness_only', 'exact_node_cli_or_tooling_topology', 'test_harness_emulation'): frozenset({
        'test/es-module/test-cjs-esm-warn.js',
        'test/es-module/test-esm-assertionless-json-import.js',
        'test/es-module/test-esm-cjs-load-error-note.mjs',
        'test/es-module/test-esm-detect-ambiguous.mjs',
        'test/es-module/test-esm-dynamic-import-mutating-fs.mjs',
        'test/es-module/test-esm-experimental-warnings.mjs',
        'test/es-module/test-esm-export-not-found.mjs',
        'test/es-module/test-esm-extension-lookup-deprecation.mjs',
        'test/es-module/test-esm-extensionless-esm-and-wasm.mjs',
        'test/es-module/test-esm-import-assertion-warning.mjs',
        'test/es-module/test-esm-import-flag.mjs',
        'test/es-module/test-esm-import-meta-main-eval.mjs',
        'test/es-module/test-esm-import-meta-resolve.mjs',
        'test/es-module/test-esm-initialization.mjs',
        'test/es-module/test-esm-invalid-pjson.js',
        'test/es-module/test-esm-loader-chaining.mjs',
        'test/es-module/test-esm-loader-custom-condition.mjs',
        'test/es-module/test-esm-loader-default-resolver.mjs',
        'test/es-module/test-esm-loader-entry-url.mjs',
        'test/es-module/test-esm-loader-hooks.mjs',
        'test/es-module/test-esm-loader-http-imports.mjs',
        'test/es-module/test-esm-loader-invalid-format.mjs',
        'test/es-module/test-esm-loader-invalid-url.mjs',
        'test/es-module/test-esm-loader-not-found.mjs',
        'test/es-module/test-esm-loader-programmatically.mjs',
        'test/es-module/test-esm-loader-resolve-type.mjs',
        'test/es-module/test-esm-loader-spawn-promisified.mjs',
        'test/es-module/test-esm-loader-stringify-text.mjs',
        'test/es-module/test-esm-loader-thenable.mjs',
        'test/es-module/test-esm-loader-with-source.mjs',
        'test/es-module/test-esm-loader-with-syntax-error.mjs',
        'test/es-module/test-esm-loader.mjs',
        'test/es-module/test-esm-module-not-found-commonjs-hint.mjs',
        'test/es-module/test-esm-named-exports.mjs',
        'test/es-module/test-esm-non-js.mjs',
        'test/es-module/test-esm-nowarn-exports.mjs',
        'test/es-module/test-esm-preserve-symlinks-main.js',
        'test/es-module/test-esm-preserve-symlinks-not-found-plain.mjs',
        'test/es-module/test-esm-preserve-symlinks-not-found.mjs',
        'test/es-module/test-esm-source-map.mjs',
        'test/es-module/test-esm-tla-syntax-errors-not-recognized-as-tla-error.mjs',
        'test/es-module/test-esm-tla-unfinished.mjs',
        'test/es-module/test-esm-type-field-errors.js',
        'test/es-module/test-esm-type-flag-cli-entry.mjs',
        'test/es-module/test-esm-type-flag-errors.mjs',
        'test/es-module/test-esm-type-flag-loose-files.mjs',
        'test/es-module/test-esm-type-flag-package-scopes.mjs',
        'test/es-module/test-esm-type-flag-string-input.mjs',
        'test/es-module/test-esm-unknown-extension.js',
        'test/es-module/test-esm-wasm-globals-all-types.mjs',
        'test/es-module/test-esm-wasm-js-string-builtins.mjs',
        'test/es-module/test-esm-wasm-module-instances-warning.mjs',
        'test/es-module/test-esm-wasm-no-code-injection.mjs',
        'test/es-module/test-esm-wasm-non-identifier-exports.mjs',
        'test/es-module/test-esm-wasm-reject-wasm-export-names.mjs',
        'test/es-module/test-esm-wasm-reject-wasm-import-names.mjs',
        'test/es-module/test-esm-wasm-reject-wasm-js-export-names.mjs',
        'test/es-module/test-esm-wasm-reject-wasm-js-import-module.mjs',
        'test/es-module/test-esm-wasm-reject-wasm-js-import-names.mjs',
        'test/es-module/test-esm-wasm-source-phase-dynamic.mjs',
        'test/es-module/test-esm-wasm-source-phase-identity.mjs',
        'test/es-module/test-esm-wasm-source-phase-no-execute-dynamic.mjs',
        'test/es-module/test-esm-wasm-source-phase-no-execute.mjs',
        'test/es-module/test-esm-wasm-source-phase-not-defined-dynamic.mjs',
        'test/es-module/test-esm-wasm-source-phase-not-defined-static.mjs',
        'test/es-module/test-esm-wasm-top-level-execution.mjs',
        'test/es-module/test-esm-wasm-vm-source-phase-dynamic.mjs',
        'test/es-module/test-esm-wasm-vm-source-phase-static.mjs',
        'test/es-module/test-esm-wasm.mjs',
        'test/es-module/test-loaders-unknown-builtin-module.mjs',
        'test/es-module/test-require-module-cycle-esm-cjs-esm-esm.js',
        'test/es-module/test-require-module-cycle-esm-cjs-esm.js',
        'test/es-module/test-require-module-cycle-esm-esm-cjs-esm-esm.js',
        'test/es-module/test-require-module-cycle-esm-esm-cjs-esm.js',
        'test/es-module/test-require-module-errors.js',
        'test/es-module/test-require-module-feature-detect.js',
        'test/es-module/test-require-module-tla-print-execution.js',
        'test/es-module/test-require-module-warning.js',
        'test/es-module/test-require-node-modules-warning.js',
        'test/es-module/test-typescript-commonjs.mjs',
        'test/es-module/test-typescript-eval.mjs',
        'test/es-module/test-typescript-module.mjs',
        'test/es-module/test-typescript-transform.mjs',
        'test/es-module/test-typescript.mjs',
        'test/module-hooks/test-module-hooks-load-async-and-sync.js',
        'test/module-hooks/test-module-hooks-preload.js',
        'test/module-hooks/test-module-hooks-require-esm.js',
        'test/parallel/test-fs-readfile-eof.js',
        'test/parallel/test-fs-syncwritestream.js',
        'test/parallel/test-global-customevent-disabled.js',
        'test/parallel/test-module-run-main-monkey-patch.js',
        'test/parallel/test-shadow-realm-preload-module.js',
        'test/sea/test-single-executable-blob-config-errors.js',
        'test/sea/test-single-executable-blob-config.js',
    }),
    ('test_harness_only', 'official_harness_or_support_file', 'test_harness_emulation'): frozenset({
        'test/parallel/test-node-output-v8-warning.mjs',
        'test/parallel/test-node-output-vm.mjs',
        'test/parallel/test-process-env-allowed-flags-are-documented.js',
    }),
    ('upstream_or_platform_boundary', 'upstream_or_platform_boundary', 'unsupported'): frozenset({
        'test/parallel/test-fs-long-path.js',
        'test/parallel/test-fs-promises-watch-ignore-function.mjs',
        'test/parallel/test-fs-promises-watch-ignore-glob.mjs',
        'test/parallel/test-fs-promises-watch-ignore-mixed.mjs',
        'test/parallel/test-fs-promises-watch-ignore-regexp.mjs',
        'test/parallel/test-fs-promises-watch-iterator.js',
        'test/parallel/test-fs-read-file-sync-hostname.js',
        'test/parallel/test-fs-readdir-buffer.js',
        'test/parallel/test-fs-readdir-ucs2.js',
        'test/parallel/test-fs-readfilesync-enoent.js',
        'test/parallel/test-fs-realpath-on-substed-drive.js',
        'test/parallel/test-fs-write-file-invalid-path.js',
        'test/parallel/test-fs-write.js',
        'test/parallel/test-module-readonly.js',
        'test/parallel/test-module-strip-types.js',
        'test/parallel/test-module-subpath-import-long-path.js',
        'test/parallel/test-os-fast.js',
        'test/parallel/test-process-hrtime-bigint.js',
        'test/parallel/test-process-hrtime.js',
        'test/parallel/test-process-versions.js',
        'test/parallel/test-strace-openat-openssl.js',
        'test/parallel/test-util-getcallsites.js',
        'test/parallel/test-util-types.js',
        'test/parallel/test-v8-flag-pool-size-0.js',
        'test/parallel/test-v8-flags.js',
        'test/parallel/test-whatwg-url-canparse.js',
        'test/v8-updates/test-linux-perf-logger.js',
        'test/v8-updates/test-linux-perf.js',
    }),
    ('v8_isolate_optional', 'expose_internals_private_module_surface', 'unsupported'): frozenset({
        'test/es-module/test-cjs-legacyMainResolve.js',
        'test/es-module/test-esm-import-attributes-validation.js',
        'test/es-module/test-esm-loader-modulemap.js',
        'test/es-module/test-esm-loader-search.js',
        'test/es-module/test-esm-long-path-win.js',
        'test/es-module/test-esm-resolve-type.mjs',
        'test/es-module/test-esm-url-extname.js',
        'test/parallel/test-abortcontroller-internal.js',
        'test/parallel/test-blob.js',
        'test/parallel/test-compression-decompression-stream.js',
        'test/parallel/test-data-url.js',
        'test/parallel/test-debug-v8-fast-api.js',
        'test/parallel/test-eventtarget-memoryleakwarning.js',
        'test/parallel/test-eventtarget.js',
        'test/parallel/test-fs-error-messages.js',
        'test/parallel/test-fs-existssync-memleak-longpath.js',
        'test/parallel/test-fs-filehandle.js',
        'test/parallel/test-fs-rm.js',
        'test/parallel/test-fs-utils-get-dirents.js',
        'test/parallel/test-global-customevent.js',
        'test/parallel/test-util-inspect-proxy.js',
        'test/parallel/test-util-inspect.js',
        'test/parallel/test-util-internal.js',
        'test/parallel/test-util-promisify.js',
        'test/parallel/test-util-sigint-watchdog.js',
        'test/parallel/test-util-sleep.js',
        'test/parallel/test-util.js',
        'test/parallel/test-v8-serdes.js',
        'test/parallel/test-webapi-sharedarraybuffer-rejection.js',
        'test/parallel/test-whatwg-encoding-custom-internals.js',
        'test/parallel/test-whatwg-encoding-custom-interop.js',
        'test/parallel/test-whatwg-encoding-custom-textdecoder.js',
        'test/parallel/test-whatwg-readablebytestream.js',
        'test/parallel/test-whatwg-readablestream.js',
        'test/parallel/test-whatwg-transformstream.js',
        'test/parallel/test-whatwg-webstreams-adapters-streambase.js',
        'test/parallel/test-whatwg-webstreams-adapters-to-readablewritablepair.js',
        'test/parallel/test-whatwg-webstreams-adapters-to-streamduplex.js',
        'test/parallel/test-whatwg-webstreams-adapters-to-streamreadable.js',
        'test/parallel/test-whatwg-webstreams-adapters-to-streamwritable.js',
        'test/parallel/test-whatwg-webstreams-adapters-to-writablestream.js',
        'test/parallel/test-whatwg-webstreams-coverage.js',
        'test/parallel/test-whatwg-webstreams-transfer.js',
        'test/parallel/test-whatwg-writablestream.js',
    }),
}

NDS3_WAVE2_PREFIXES = {
    ('test_harness_only', 'exact_node_cli_or_tooling_topology', 'test_harness_emulation'): (
        'test/module-hooks/test-async-loader-hooks-',
    ),
}

WAVE2_REASON_TEXT = {
    'child_process_host_output_topology': "fixture spawns a host child process (NODE_V8_COVERAGE, child stdout/stderr formatting, or cached-data round-trip) and asserts that child's host-owned output, which the V8 isolate does not produce",
    'exact_host_process_control_surface': 'fixture spawns or controls a host process (execPath child asserting exit code/env/argv/ppid, process.chdir, or raw host process state) and must fail closed inside the V8 isolate',
    'exact_node_cli_or_tooling_topology': 'fixture runs its assertion in a spawned Node CLI child process (custom loader hooks, --import/--require/--experimental-loader, SEA single-executable, WASM/TypeScript loader, or require(esm) subprocess topology) rather than in-isolate Application API behavior',
    'expose_internals_private_module_surface': "fixture is gated behind --expose-internals and exercises private require('internal/*') modules outside the public Application API surface; isolate-safe but intentionally not exposed, so it is a visible optional gap rather than required support",
    'host_owned_non_isolate_harness': 'fixture depends on a host-owned diagnostic surface (inspector debugging port or NODE_DEBUG child timing) and must fail closed unless a host-capable backend is selected',
    'host_owned_permission_policy': 'fixture is gated behind the host --permission model (--allow-fs-*/--allow-child-process) and asserts permission-model side effects the V8 isolate does not own',
    'official_harness_or_support_file': 'fixture exercises upstream Node harness or documentation-consistency topology rather than the Application runtime support contract',
    'upstream_or_platform_boundary': "fixture is gated by V8 native-syntax intrinsics, host-platform skips, host-specific filesystem/watch backends, or Node's exact native build/dependency composition, so it cannot run as public Application API behavior inside the Nimbus V8 isolate",
}


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def classify_entry(entry: dict[str, Any]) -> tuple[str, str, str, str]:
    source = entry.get("classification", "")
    test_path = entry.get("test_path", "")
    owner = entry.get("owner", "")
    haystack = f"{test_path} {owner}".lower().replace("_", "-")

    if source == "rust_watchpoint_expected_failure" and (
        test_path == "test/parallel/test-v8-serdes.js" and owner == "runtime/v8"
    ):
        return (
            "upstream_or_platform_boundary",
            "v8_serialization_wire_format_boundary",
            "unsupported",
            "fixture asserts Node's exact serialized-byte format, which is tied to Node's embedded V8 release rather than Nimbus's v8_deno_core compatibility contract",
        )
    if source == "rust_watchpoint_expected_failure":
        return (
            "v8_isolate_required",
            "watchpoint_required_surface",
            "unsupported",
            "ignored Rust watchpoint marks this as a measured required red path until fixed or explicitly reclassified",
        )
    if source in {
        "requires_native_addon_harness",
        "requires_pseudo_tty_host_harness",
    }:
        return (
            "diagnostic_only_non_isolate",
            "host_owned_non_isolate_harness",
            "diagnostic_stub",
            "fixture depends on host-owned native or terminal behavior and must fail closed unless a host-capable backend is selected",
        )
    if source in {
        "requires_pummel_stress_harness",
        "requires_sequential_host_state_harness",
        "requires_wpt_harness",
        "support_fixture_not_top_level_test",
    }:
        return (
            "test_harness_only",
            "official_harness_or_support_file",
            "test_harness_emulation",
            "fixture exercises upstream harness topology rather than the Application runtime support contract",
        )
    if source == "upstream_known_issue_or_platform_boundary":
        return (
            "upstream_or_platform_boundary",
            "upstream_or_platform_boundary",
            "unsupported",
            "fixture is blocked by upstream, version-specific, or host-platform behavior",
        )
    if source == "node26_current_broad_pre_run_residual":
        return (
            "v8_isolate_required",
            "node26_current_required_residual",
            "unsupported",
            "NDS4 Node26 Current broad pre-run recorded this official fixture as skipped or failing, so it remains a required-surface red path until focused Current-lane promotion proves it green",
        )
    if source == "vendored_non_official_placeholder":
        return (
            "test_harness_only",
            "vendored_placeholder",
            "test_harness_emulation",
            "vendored placeholder is not a top-level Application runtime API claim",
        )
    if source == "requires_unpromoted_node_surface":
        if test_path in HOST_PROCESS_CONTROL_PATHS or any(
            test_path.startswith(prefix) for prefix in HOST_PROCESS_CONTROL_PREFIXES
        ):
            return (
                "diagnostic_only_non_isolate",
                "exact_host_process_control_surface",
                "diagnostic_stub",
                "fixture requires host process replacement, abort/fatal-exit behavior, native dlopen, raw stdio, signal delivery, uid/gid/group mutation, or warning-file side effects and must fail closed inside the V8 isolate",
            )
        if test_path in NODE_CLI_TOPOLOGY_PATHS or any(
            test_path.startswith(prefix) for prefix in NODE_CLI_TOPOLOGY_PREFIXES
        ):
            return (
                "test_harness_only",
                "exact_node_cli_or_tooling_topology",
                "test_harness_emulation",
                "fixture exercises Node CLI, debug-port, preload-print, proxy CLI, tick-processor, or upstream tooling topology rather than Application runtime API support",
            )
        if any(
            test_path.startswith(prefix) for prefix in NATIVE_ADDON_NODE_API_PREFIXES
        ):
            return (
                "diagnostic_only_non_isolate",
                "native_addon_node_api_surface",
                "diagnostic_stub",
                "fixture loads a compiled Node-API native addon (build/<type>/*.node) through dlopen, which runs host-native machine code outside the V8 isolate and must fail closed unless a host-capable backend is selected",
            )
        if test_path in NATIVE_BACKED_OPTIONAL_BUILTIN_PATHS:
            return (
                "v8_isolate_optional",
                "non_required_native_backed_builtin",
                "unsupported",
                "fixture exercises the non-required node:sqlite native-backed builtin, which is isolate-safe-capable through a runtime-provided implementation but is not part of the default Application contract in this wave",
            )
        if test_path.startswith(NODE_LINT_RULE_HARNESS_PREFIX):
            return (
                "test_harness_only",
                "node_lint_rule_harness",
                "test_harness_emulation",
                "fixture drives Node's own ESLint custom-rule harness (tools/eslint-rules with RuleTester and skipIfEslintMissing) rather than Application runtime API behavior",
            )
        if test_path in STARTUP_SNAPSHOT_CLI_PATHS:
            return (
                "test_harness_only",
                "startup_snapshot_cli_topology",
                "test_harness_emulation",
                "fixture builds and restores a V8 startup snapshot through the common/snapshot CLI subprocess (--build-snapshot/--snapshot-blob) rather than in-isolate Application API behavior",
            )
        if test_path in INTERNAL_BOOTSTRAP_TOPOLOGY_PATHS:
            return (
                "test_harness_only",
                "internal_bootstrap_module_topology",
                "test_harness_emulation",
                "fixture asserts Node's exact internal bootstrap moduleLoadList rather than a public Application API contract",
            )
        if test_path in EXPOSE_INTERNALS_PRIVATE_MODULE_PATHS:
            return (
                "v8_isolate_optional",
                "expose_internals_private_module_surface",
                "unsupported",
                "fixture is gated behind --expose-internals and exercises private require('internal/*') modules outside the public Application API surface; isolate-safe but intentionally not exposed, so it is a visible optional gap rather than required support",
            )
        for _w2_key, _w2_paths in NDS3_WAVE2_RECLASSIFICATIONS.items():
            if test_path in _w2_paths:
                _w2_denom, _w2_reason, _w2_shim = _w2_key
                return (_w2_denom, _w2_reason, _w2_shim, WAVE2_REASON_TEXT[_w2_reason])
        for _w2_key, _w2_prefixes in NDS3_WAVE2_PREFIXES.items():
            if any(test_path.startswith(_p) for _p in _w2_prefixes):
                _w2_denom, _w2_reason, _w2_shim = _w2_key
                return (_w2_denom, _w2_reason, _w2_shim, WAVE2_REASON_TEXT[_w2_reason])
        for keyword in CATEGORY_KEYWORDS["diagnostic_only_non_isolate"]:
            if keyword in haystack:
                return (
                    "diagnostic_only_non_isolate",
                    "legacy_unpromoted_host_owned_surface",
                    "diagnostic_stub",
                    "legacy unpromoted fixture names host-owned process, socket, native, signal, or worker behavior",
                )
        for keyword in CATEGORY_KEYWORDS["test_harness_only"]:
            if keyword in haystack:
                return (
                    "test_harness_only",
                    "legacy_unpromoted_harness_surface",
                    "test_harness_emulation",
                    "legacy unpromoted fixture names harness, terminal, REPL, WPT, or test-runner topology",
                )
        for keyword in CATEGORY_KEYWORDS["v8_isolate_required"]:
            if keyword in haystack:
                return (
                    "v8_isolate_required",
                    "legacy_unpromoted_required_api_surface",
                    "unsupported",
                    "legacy unpromoted fixture names public JavaScript or V8-isolate-compatible Node API behavior",
                )
        return (
            "v8_isolate_optional",
            "legacy_unpromoted_optional_surface",
            "unsupported",
            "legacy unpromoted fixture is visible and promotable, but not required for the default Application contract in NDS1",
        )
    return (
        "v8_isolate_optional",
        "unknown_legacy_classification",
        "unsupported",
        f"unrecognized source classification {source!r}; kept visible as optional until triaged",
    )


def build_posture(repo: Path) -> dict[str, Any]:
    status_path = repo / "docs/architecture/runtime/node-compat-evidence/latest/status-summary.json"
    status = load_json(status_path)
    lanes: dict[str, Any] = {}

    for lane_summary in status["lane_summaries"]:
        lane = lane_summary["lane"]
        catalog = lane_summary["classification_catalog"]
        entries = []
        denominator_counts: Counter[str] = Counter()
        source_counts: Counter[str] = Counter(catalog.get("by_classification", {}))
        legacy_unpromoted_source_count = source_counts.get("requires_unpromoted_node_surface", 0)
        reclassified_legacy_count = 0

        for entry in catalog.get("entries", []):
            denominator, reason_code, shim_class, reason = classify_entry(entry)
            if entry.get("classification") == "requires_unpromoted_node_surface":
                reclassified_legacy_count += 1
            denominator_counts[denominator] += 1
            entries.append(
                {
                    "test_path": entry["test_path"],
                    "source_expectation": entry["expectation"],
                    "source_classification": entry["classification"],
                    "owner": entry["owner"],
                    "support_denominator": denominator,
                    "reason_code": reason_code,
                    "reason": reason,
                    "evidence_path": catalog["catalog_path"],
                    "docs_cross_check": docs_cross_check(denominator),
                    "shim_classification": shim_class,
                }
            )

        passed = lane_summary["documented_manifested_green_count"]
        required_gaps = denominator_counts["v8_isolate_required"]
        optional_gaps = denominator_counts["v8_isolate_optional"]
        reachable_ceiling = passed + required_gaps + optional_gaps
        required_total = passed + required_gaps
        required_pass_rate = round((passed / required_total) * 100, 2) if required_total else 100.0

        lanes[lane] = {
            "role": lane_summary["lane_role"],
            "upstream": lane_summary["upstream"],
            "full_official_fixture_corpus": lane_summary["vendored_test_file_count"],
            "current_passed": passed,
            "current_pass_rate": lane_summary["documented_manifested_green_ratio"],
            "source_classification_counts": dict(source_counts),
            "source_requires_unpromoted_node_surface_count": legacy_unpromoted_source_count,
            "reclassified_requires_unpromoted_node_surface_count": reclassified_legacy_count,
            "remaining_requires_unpromoted_node_surface_count": 0,
            "support_denominator_counts": dict(denominator_counts),
            "v8_isolate_required": {
                "passed": passed,
                "gaps": required_gaps,
                "total": required_total,
                "pass_rate_percent": required_pass_rate,
            },
            "v8_isolate_optional": {
                "gaps": optional_gaps,
            },
            "diagnostic_only_non_isolate": {
                "gaps": denominator_counts["diagnostic_only_non_isolate"],
            },
            "test_harness_only": {
                "gaps": denominator_counts["test_harness_only"],
            },
            "upstream_or_platform_boundary": {
                "gaps": denominator_counts["upstream_or_platform_boundary"],
            },
            "node24_2000_feasibility" if lane == "node24" else "feasibility": {
                "target_pass_count": 2000 if lane == "node24" else None,
                "current_passed": passed,
                "required_gap_count": required_gaps,
                "optional_promotable_gap_count": optional_gaps,
                "estimated_reachable_pass_ceiling": reachable_ceiling,
                "target_reachable_in_this_plan": reachable_ceiling >= 2000 if lane == "node24" else None,
            },
            "entries": entries,
        }

    return {
        "schema_version": 1,
        "report_kind": "node_default_support_posture",
        "generated_from": [
            "docs/architecture/runtime/node-compat-evidence/latest/status-summary.json",
            "tests/runtime/node/classifications/*.json",
            "docs/plans/node-default-runtime-support-hardening-plan.md",
        ],
        "denominator_vocabulary": DENOMINATORS,
        "shim_classification_vocabulary": SHIM_CLASSES,
        "reason_vocabulary": sorted(
            {
                "child_process_host_output_topology",
                "host_owned_non_isolate_harness",
                "host_owned_permission_policy",
                "exact_host_process_control_surface",
                "exact_node_cli_or_tooling_topology",
                "expose_internals_private_module_surface",
                "internal_bootstrap_module_topology",
                "legacy_unpromoted_harness_surface",
                "legacy_unpromoted_host_owned_surface",
                "legacy_unpromoted_optional_surface",
                "legacy_unpromoted_required_api_surface",
                "native_addon_node_api_surface",
                "node_lint_rule_harness",
                "non_required_native_backed_builtin",
                "official_harness_or_support_file",
                "startup_snapshot_cli_topology",
                "unknown_legacy_classification",
                "upstream_or_platform_boundary",
                "vendored_placeholder",
                "watchpoint_required_surface",
            }
        ),
        "lanes": lanes,
    }


def docs_cross_check(denominator: str) -> str:
    if denominator == "v8_isolate_required":
        return (
            "public Application support docs must either green this fixture or "
            "provide a per-fixture proof that it tests host-owned behavior"
        )
    if denominator == "v8_isolate_optional":
        return "visible optional gap; docs must not count it as required support"
    if denominator == "diagnostic_only_non_isolate":
        return "docs must describe fail-closed diagnostic or service/microVM route"
    if denominator == "test_harness_only":
        return "docs must not count upstream harness topology as runtime API support"
    return "docs must link upstream/platform rationale when excluding from support"


def render_markdown(posture: dict[str, Any]) -> str:
    lines = [
        "# Node Default Support Posture",
        "",
        "<!-- generated by scripts/runtime/node/default_support_posture.py; do not edit by hand -->",
        "",
        "This file is the NDS default-support denominator overlay. It does not hide the",
        "full official fixture corpus; it explains which classified gaps are required",
        "V8-isolate support, optional V8-isolate support, diagnostic non-isolate",
        "behavior, test-harness-only, or upstream/platform boundary.",
        "",
        "## Denominator Vocabulary",
        "",
    ]
    for denominator in posture["denominator_vocabulary"]:
        lines.append(f"- `{denominator}`")
    lines.extend(["", "## Lane Summary", ""])
    lines.append(
        "| Lane | Role | Full Corpus | Current Passed | Required Gaps | Optional Gaps | Diagnostic | Harness Only | Upstream/Platform | Source Unpromoted | Remaining Unpromoted |"
    )
    lines.append("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for lane, summary in posture["lanes"].items():
        counts = summary["support_denominator_counts"]
        lines.append(
            f"| `{lane}` | `{summary['role']}` | {summary['full_official_fixture_corpus']} | "
            f"{summary['current_passed']} | {counts.get('v8_isolate_required', 0)} | "
            f"{counts.get('v8_isolate_optional', 0)} | "
            f"{counts.get('diagnostic_only_non_isolate', 0)} | "
            f"{counts.get('test_harness_only', 0)} | "
            f"{counts.get('upstream_or_platform_boundary', 0)} | "
            f"{summary['source_requires_unpromoted_node_surface_count']} | "
            f"{summary['remaining_requires_unpromoted_node_surface_count']} |"
        )
    node24 = posture["lanes"].get("node24", {})
    feasibility = node24.get("node24_2000_feasibility", {})
    lines.extend(
        [
            "",
            "## Node24 Feasibility",
            "",
            f"- current passed: `{feasibility.get('current_passed')}`",
            f"- required gap count: `{feasibility.get('required_gap_count')}`",
            f"- optional promotable gap count: `{feasibility.get('optional_promotable_gap_count')}`",
            f"- estimated reachable pass ceiling: `{feasibility.get('estimated_reachable_pass_ceiling')}`",
            f"- target reachable in this plan: `{str(feasibility.get('target_reachable_in_this_plan')).lower()}`",
            "",
            "The ceiling is an NDS1 estimate, not a completion claim. NDS3 may re-enter",
            "the documented blocked path if implementation disproves the estimate.",
            "",
        ]
    )
    return "\n".join(lines)


def write_markdown(path: Path, posture: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_markdown(posture), encoding="utf-8")


def validate(posture: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if posture.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if posture.get("report_kind") != "node_default_support_posture":
        errors.append("report_kind must be node_default_support_posture")
    if posture.get("denominator_vocabulary") != DENOMINATORS:
        errors.append("denominator vocabulary mismatch")
    for lane, summary in posture.get("lanes", {}).items():
        if summary.get("remaining_requires_unpromoted_node_surface_count") != 0:
            errors.append(f"{lane} still has remaining unpromoted surface")
        seen = set()
        for entry in summary.get("entries", []):
            denominator = entry.get("support_denominator")
            if denominator not in DENOMINATORS:
                errors.append(f"{lane}:{entry.get('test_path')} invalid denominator {denominator}")
            if entry.get("shim_classification") not in SHIM_CLASSES:
                errors.append(f"{lane}:{entry.get('test_path')} invalid shim classification")
            if not entry.get("reason_code") or not entry.get("evidence_path") or not entry.get("docs_cross_check"):
                errors.append(f"{lane}:{entry.get('test_path')} missing reason/evidence/docs cross-check")
            test_path = entry.get("test_path")
            if test_path in seen:
                errors.append(f"{lane}:{test_path} duplicated")
            seen.add(test_path)
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="validate checked-in generated files")
    args = parser.parse_args()

    repo = repo_root()
    json_path = repo / "docs/architecture/runtime/node-default-support-posture.json"
    md_path = repo / "docs/architecture/runtime/node-default-support-posture.md"
    posture = build_posture(repo)
    errors = validate(posture)
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    if args.check:
        expected_json = json.dumps(posture, indent=2, sort_keys=True) + "\n"
        if not json_path.exists() or json_path.read_text(encoding="utf-8") != expected_json:
            print(f"error: {json_path} is stale", file=sys.stderr)
            return 1
        expected_md = render_markdown(posture)
        if not md_path.exists() or md_path.read_text(encoding="utf-8") != expected_md:
            print(f"error: {md_path} is stale", file=sys.stderr)
            return 1
        print("node default support posture: pass")
        return 0

    write_json(json_path, posture)
    write_markdown(md_path, posture)
    print(f"wrote {json_path}")
    print(f"wrote {md_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
