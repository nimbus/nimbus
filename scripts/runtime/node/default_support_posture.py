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
    # NDS3 wave-3 disposition (2026-06-06): the only observable for the http.Agent
    # drained-socket reuse assertion is process.report.getReport().libuv handle
    # introspection (a host-process diagnostic surface that leaks host libuv
    # state), followed by process.exit(). The isolate does not own process.report,
    # so the assertion mechanism is host process control, not in-isolate API.
    "test/parallel/test-http-agent-reuse-drained-socket-only.js",
    # NDS3 cycle-26 disposition (2026-06-12): the remaining blocker in the
    # official file is the "Does not hang forever" case, which runs
    # child_process.spawn(process.execPath, ["--input-type=module"]) and asserts
    # the spawned host process exit code. The in-isolate util.aborted() behavior
    # is tested earlier in the same file; the subprocess postlude is host
    # process control and must fail closed in the default multi-tenant isolate.
    "test/parallel/test-aborted-util.js",
    # NDS3 cycle-49 (2026-06-13): source-confirmed against identical node22/node24
    # fixture bodies. Before asserting fs.utimes precision, the fixture probes
    # host filesystem Y2K38 support by running host `touch -t ...` and
    # `date -r ...` via `child_process.spawnSync`. Ambient subprocess execution
    # and host filesystem capability probing are outside the default
    # multi-tenant isolate contract.
    "test/parallel/test-fs-utimes-y2K38.js",
}

HOST_PROCESS_CONTROL_PREFIXES = (
    "test/abort/",
    "test/parallel/test-process-execve",
)

# NDS3 wave-25 disposition (2026-06-09): each fixture below binds or connects a
# real host TCP/UDP/TLS socket (or listening server) and then asserts the host
# libuv async-resource handle graph (TCPSERVERWRAP / TCPWRAP / TLSWRAP /
# SHUTDOWNWRAP / UDPWRAP) or a socket-level error/address behavior. The default
# multi-tenant V8 isolate denies ambient host network access, so the
# socket-backed behavior is host-owned and must fail closed unless a
# host-capable (sandbox-backed service / microVM) backend is selected. Source-
# confirmed per-fixture against the official fixture body:
#   async-hooks/test-graph.http.js        -> http.createServer().listen()
#   async-hooks/test-graph.shutdown.js    -> net.createServer().listen()
#   async-hooks/test-graph.tcp.js         -> net.connect() to host '::1'
#   async-hooks/test-graph.tls-write.js   -> tls.createServer/connect
#   async-hooks/test-graph.tls-write-12.js-> tls.createServer/connect (TLSv1.2)
#   async-hooks/test-tcpwrap.js           -> net.createServer() TCPWRAP graph
#   async-hooks/test-tlswrap.js           -> tls.createServer/connect TLSWRAP graph
#   parallel/test-double-tls-client.js    -> tls.connect double-TLS client
#   parallel/test-double-tls-server.js    -> tls.createServer double-TLS server
#   parallel/test-dgram-error-message-address.js -> dgram bind to '1.1.1.1'
#   parallel/test-dgram-ipv6only.js       -> dgram udp6 bind ipv6Only option
#   parallel/test-dgram-reuseport.js      -> dgram bind reusePort option
#   parallel/test-dgram-udp6-link-local-address.js -> dgram udp6 link-local bind
#   parallel/test-dgram-udp6-send-default-host.js  -> dgram udp6 send default host
#   parallel/test-https-connect-address-family.js  -> https.get real connection
# The node24 lane already routes the dgram/https members through the
# requires_unpromoted host-owned keyword catch (diagnostic_only_non_isolate);
# this set reconciles the node22 watchpoint-pinned lane and promotes the
# async-hooks graph + double-tls members (v8_isolate_required on both lanes) to
# the same host-owned disposition.
HOST_NETWORK_SOCKET_PATHS = {
    "test/async-hooks/test-graph.http.js",
    "test/async-hooks/test-graph.shutdown.js",
    "test/async-hooks/test-graph.tcp.js",
    "test/async-hooks/test-graph.tls-write.js",
    "test/async-hooks/test-graph.tls-write-12.js",
    "test/async-hooks/test-tcpwrap.js",
    "test/async-hooks/test-tlswrap.js",
    "test/parallel/test-double-tls-client.js",
    "test/parallel/test-double-tls-server.js",
    "test/parallel/test-dgram-error-message-address.js",
    "test/parallel/test-dgram-ipv6only.js",
    "test/parallel/test-dgram-reuseport.js",
    "test/parallel/test-dgram-udp6-link-local-address.js",
    "test/parallel/test-dgram-udp6-send-default-host.js",
    "test/parallel/test-https-connect-address-family.js",
    # NDS3 cycle-17 fresh-census reclassification (2026-06-11): source-confirmed
    # against crates/nimbus-runtime/.../node22/test/parallel/. Both fixtures stand
    # up a real host TLS listener and drive client sockets against it:
    #   test-https-localaddress-bind-error.js: https.createServer + server.listen(
    #     0, '127.0.0.1') then https.request({ localAddress: '1.2.3.4' }) to assert
    #     the OS-level client-socket bind error; the multi-tenant isolate denies
    #     ambient host network access, so the fresh census reports
    #     `NotCapable: Requires net access to "1.2.3.4:0"`.
    #   test-https-agent-additional-options.js: https.Server + many live TLS client
    #     requests through https.globalAgent asserting the socket-pool keying across
    #     TLS options (dhparam/ecdhCurve/secureProtocol/...); needs a real host TLS
    #     server+client loopback the isolate must not own (census: `unsupported
    #     protocol`). Same host-owned socket class as test-https-connect-address-
    #     family.js directly above.
    "test/parallel/test-https-localaddress-bind-error.js",
    "test/parallel/test-https-agent-additional-options.js",
    # NDS3 cycle-46 (2026-06-13): source-confirmed against node22/node24 fixture
    # bodies. The assertion is a stream base TypeError, but the fixture only
    # reaches it after creating a real `net.createServer().listen(0)` listener
    # and connecting a real `net.connect(server.address().port)` client socket.
    # That host TCP listener/client topology is the same host-owned socket
    # surface as the async-hooks TCP graph fixtures.
    "test/parallel/test-stream-base-typechecking.js",
    # NDS3 cycle-47 (2026-06-13): source-confirmed against node22/node24
    # test/es-module fixture bodies. The WebAssembly streaming cases call a
    # helper that creates a real `http.createServer(...).unref().listen(0)`,
    # waits for the listening event, reads `server.address().port`, and drives
    # `fetch("http://127.0.0.1:${port}/foo.wasm")` against that host listener.
    # The streaming API assertions are therefore coupled to a host loopback HTTP
    # server/client topology that the default multi-tenant isolate must not own.
    "test/es-module/test-wasm-web-api.js",
    # NDS3 cycle-48 (2026-06-13): source-confirmed against identical node22/node24
    # fixture bodies. The fixture chains host process `beforeExit` callbacks and
    # one mandatory step creates a real `net.createServer().listen(0)` listener
    # inside that lifecycle chain before closing it and continuing the exit-loop
    # assertions. The default multi-tenant isolate must not own host process
    # beforeExit liveness or ambient host TCP listeners.
    "test/parallel/test-process-beforeexit.js",
    # NDS3 cycle-45 (2026-06-13): source-confirmed against node22/node24 fixture
    # bodies. The fixture runs with `// Flags: --expose-gc`, creates a real
    # `http.createServer(...).listen(0)`, repeatedly drives `http.get()` clients
    # to localhost, destroys each accepted socket via `res.connection.destroy()`,
    # then requires every ClientRequest object to surface an async_hooks
    # GC-tracker destroy event after `globalThis.gc()`. This is host-owned HTTP
    # socket teardown plus exact exposed-GC async-resource topology, not portable
    # multi-tenant V8 isolate Application API support.
    "test/parallel/test-gc-http-client-connaborted.js",
}

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
    # NDS3 wave-3 disposition (2026-06-06): each fixture below runs its core
    # assertion in a SPAWNED Node child process (spawn/spawnSync/spawnPromisified/
    # execFile/fork) or a cluster.fork worker, or asserts a spawned child's
    # stdout/stderr/exit-code snapshot. A multi-tenant V8 isolate has no ambient
    # subprocess execution, so the assertion is not in-isolate Application API.
    # Source-confirmed per-fixture (// Flags, require lines, spawn call sites);
    # the workflow's adversarial skeptic upheld and the lead re-read each source.
    "test/parallel/test-async-context-frame.mjs",
    "test/parallel/test-async-hooks-stack-overflow-try-catch.js",
    "test/parallel/test-buffer-constructor-node-modules.js",
    "test/parallel/test-fs-readfile-error.js",
    "test/parallel/test-fs-write-stream-patch-open.js",
    "test/parallel/test-inspect-async-hook-setup-at-inspect.js",
    "test/parallel/test-node-output-console.mjs",
    "test/parallel/test-node-output-errors.mjs",
    "test/parallel/test-performance-nodetiming-uvmetricsinfo.js",
    "test/parallel/test-process-exec-argv.js",
    "test/parallel/test-process-exit-code-validation.js",
    "test/parallel/test-process-finalization.mjs",
    "test/parallel/test-process-uncaught-exception-monitor.js",
    "test/parallel/test-throw-error-with-getter-throw-traced.mjs",
    "test/parallel/test-throw-undefined-or-null-traced.mjs",
    "test/parallel/test-trace-env-stack.js",
    "test/parallel/test-trace-env.js",
    "test/parallel/test-trace-exit-stack-limit.js",
    "test/parallel/test-v8-startup-snapshot-api.js",
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
    # NDS3 wave-3 disposition (2026-06-06): // Flags: --expose-internals, with a
    # top-level require('internal/event_target') for NodeEventTarget. The isolate
    # intentionally does not expose private internal/* modules to tenant code, so
    # the fixture cannot run as-is; isolate-safe but a visible optional gap.
    "test/parallel/test-timers-immediate-promisified.js",
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
        # NDS3 census wave (2026-06-05): source-confirmed + adversarially verified (analyze+refute).
        'test/parallel/test-trace-events-promises.js',
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
        # NDS3 census wave (2026-06-05): source-confirmed + adversarially verified (analyze+refute).
        'test/es-module/test-esm-no-addons.mjs',
        'test/parallel/test-async-wrap-pop-id-during-load.js',
        'test/parallel/test-buffer-constructor-node-modules-paths.js',
        'test/parallel/test-dgram-bind-socket-close-before-cluster-reply.js',
        'test/parallel/test-dgram-cluster-bind-error.js',
        'test/parallel/test-dgram-cluster-close-during-bind.js',
        'test/parallel/test-dgram-exclusive-implicit-bind.js',
        'test/parallel/test-diagnostics-channel-process.js',
        'test/parallel/test-os-userinfo-handles-getter-errors.js',
        'test/parallel/test-process-chdir-errormessage.js',
        'test/parallel/test-promise-reject-callback-exception.js',
        'test/parallel/test-promise-unhandled-flag.js',
        'test/parallel/test-set-http-max-http-headers.js',
        'test/parallel/test-trace-events-async-hooks-dynamic.js',
        'test/parallel/test-trace-events-fs-async.js',
        'test/parallel/test-trace-events-fs-sync.js',
        'test/parallel/test-trace-exit.js',
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
        # NDS3 cycle-9 dossier wave (2026-06-09): source-confirmed +
        # adversarially verified (analyze+refute, not refuted). Each fixture's
        # BLOCKING assertion is common.spawnPromisified(process.execPath, ['-pe',
        # ...]) (common/index.js:834 spawns a real child_process), executed to
        # verify timers/promises ref/unref behavior in a fresh subprocess. The
        # in-isolate timers/promises surface is exercised elsewhere; the
        # subprocess postlude must fail closed.
        'test/parallel/test-timers-interval-promisified.js',
        'test/parallel/test-timers-timeout-promisified.js',
    }),
    ('diagnostic_only_non_isolate', 'host_owned_non_isolate_harness', 'diagnostic_stub'): frozenset({
        # NDS3 census wave (2026-06-05): source-confirmed + adversarially verified (analyze+refute).
        'test/parallel/test-trace-events-threadpool.js',
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
    ('diagnostic_only_non_isolate', 'absolute_host_path_policy_boundary', 'diagnostic_stub'): frozenset({
        # NDS3 cycle 12 (2026-06-08): source-confirmed against the official
        # fixture and the existing ignored watchpoint. The fixture's first
        # assertion opens an absolute host-root path outside the generated
        # bundle root and requires raw Node ENOENT behavior. Nimbus intentionally
        # denies absolute host-path probes before raw host open in the
        # multi-tenant isolate, so greening this fixture by remapping the
        # capability denial would weaken the host fs boundary.
        'test/parallel/test-fs-open.js',
        'test/parallel/test-fs-stream-construct-compat-error-read.js',
    }),
    ('diagnostic_only_non_isolate', 'host_filesystem_ownership_boundary', 'diagnostic_stub'): frozenset({
        # NDS3 cycle 13 (2026-06-08): source-confirmed. After in-isolate
        # lchown argument validation, the non-Windows block reads host uid/gid
        # via process.geteuid()/getegid() and calls fs.lchown* against a real
        # file. The default multi-tenant isolate must not expose host uid/gid
        # sys access or symlink/file ownership mutation.
        'test/parallel/test-fs-lchown.js',
    }),
    ('diagnostic_only_non_isolate', 'host_owned_system_resource_surface', 'diagnostic_stub'): frozenset({
        # NDS3 wave-25 (2026-06-09): source-confirmed. Each fixture's blocking
        # assertion reads or mutates a host-owned system resource the default
        # multi-tenant isolate denies as host sys access:
        #   test-os.js: the priority block (gated only on !common.isIBMi) calls
        #     os.setPriority()/os.getPriority(), which round-trips host process
        #     scheduling priority (setpriority(2)); granting it would let tenant
        #     code renice host processes. The sibling test-os-process-priority.js
        #     is already classified host-owned here, and the pure os.* surfaces
        #     (hostname/type/release/...) remain visible elsewhere.
        #   test-process-available-memory.js: a five-line fixture whose sole
        #     assertion calls process.availableMemory(), reading host free system
        #     memory (systemMemoryInfo); exposing host RAM stats to tenant code is
        #     a host-info leak the sandbox denies. A fabricated value would be a
        #     false green, so the honest disposition is host-owned fail-closed.
        'test/parallel/test-os.js',
        'test/parallel/test-process-available-memory.js',
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
        # NDS3 census wave (2026-06-05): source-confirmed + adversarially verified (analyze+refute).
        # V8 native-syntax intrinsics (--allow-natives-syntax) + internalBinding('debug') fast-API counters.
        'test/parallel/test-perf-hooks-histogram-fast-calls.js',
        'test/parallel/test-timers-fast-calls.js',
        'test/parallel/test-timers-now.js',
        # NDS3 gap-taxonomy wave (2026-06-05): consistency follow-up to the
        # fast-calls cluster directly above. Source-confirmed identical structural
        # gate - each requires a private debug/native-build surface that cannot
        # exist inside the public V8 isolate, so it can never run as Application API:
        #   test-buffer-write-fast / test-buffer-swap-fast: header is
        #     `// Flags: --expose-internals --no-warnings --allow-natives-syntax`
        #     with require('internal/test/binding') + %-prefixed V8 intrinsics; the
        #     file is unparseable as normal JS without the --allow-natives-syntax flag
        #     (the recorded gap detail is literally `SyntaxError: Unexpected token '%'`).
        #     Exact siblings test-timers-fast-calls / test-perf-hooks-histogram-fast-calls
        #     were already reclassified here on identical grounds.
        #   test-buffer-alloc-unsafe-is-uninitialized / ...-is-initialized-with-zero-fill-flag:
        #     `if (!common.isDebug) common.skip('Only works in debug mode')` plus
        #     internalBinding('debug').getGenericUsageCount('NodeArrayBufferAllocator.*')
        #     - debug-build-only native allocator instrumentation counters that the
        #     isolate does not expose; the public Buffer.allocUnsafe surface they wrap
        #     remains a visible required gap elsewhere.
        'test/parallel/test-buffer-write-fast.js',
        'test/parallel/test-buffer-swap-fast.js',
        'test/parallel/test-buffer-alloc-unsafe-is-uninitialized.js',
        'test/parallel/test-buffer-alloc-unsafe-is-initialized-with-zero-fill-flag.js',
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
        # NDS3 cycle-43 (2026-06-13): source-confirmed. The fixture imports
        # test/fixtures/webcrypto/supports-modern-algorithms.mjs, which derives
        # its expected SubtleCrypto.supports() matrix from Node's exact OpenSSL
        # version gates (`hasOpenSSL(3)`, `hasOpenSSL(3, 5)`) rather than from
        # pure WebCrypto algorithm syntax. Nimbus/Deno's native provider is
        # aws-lc/BoringSSL-shaped: the process polyfill reports
        # openssl_is_boringssl, AES-OCB exists in deno_crypto, ML-DSA/ML-KEM
        # exist through aws-lc unstable hooks, and KMAC128/256 are not registered
        # in deno_crypto's WebCrypto tables. Focused cycle-43 census confirmed
        # the fixture first wanted AES-OCB true, then failed on the provider
        # matrix skew (ML-DSA expected false under OpenSSL<3.5; KMAC object
        # support expected true under OpenSSL>=3). That is Node's native crypto
        # dependency composition, not a portable isolate API guarantee.
        'test/parallel/test-webcrypto-supports.mjs',
        'test/parallel/test-strace-openat-openssl.js',
        'test/parallel/test-util-getcallsites.js',
        'test/parallel/test-util-types.js',
        'test/parallel/test-v8-flag-pool-size-0.js',
        'test/parallel/test-v8-flags.js',
        'test/parallel/test-whatwg-url-canparse.js',
        'test/v8-updates/test-linux-perf-logger.js',
        'test/v8-updates/test-linux-perf.js',
        # NDS3 wave-3 disposition (2026-06-06): each fixture's CORE assertion is
        # gated by a CLI flag the multi-tenant isolate cannot honor or asserts
        # Node's exact native build / experimental-protocol composition, so it
        # cannot run as public Application API. Source-confirmed per-fixture:
        #   test-esm-type-field-errors-2: `// Flags: --no-experimental-require-module`
        #     asserts require(esm) THROWS; the isolate enables require(esm) and
        #     cannot toggle the flag, so the throw is unreproducible.
        #   test-eval-disallow-code-generation-from-strings: `// Flags:
        #     --disallow-code-generation-from-strings` asserts eval/new Function
        #     throw EvalError under an isolate code-gen policy flag the runtime
        #     does not expose per-invocation.
        #   test-global-webcrypto-disbled: `// Flags:
        #     --no-experimental-global-webcrypto` asserts globalThis.crypto ===
        #     undefined; the isolate always exposes global webcrypto and cannot
        #     toggle the flag.
        #   test-process-config: asserts process.config deepEquals the build's
        #     config.gypi (Node's exact native build composition).
        #   test-process-exception-capture* (x3): `--abort-on-uncaught-exception`
        #     (or v8.setFlagsFromString of it) gating host abort/fatal-exit
        #     behavior the isolate must fail closed on.
        #   test-quic-session-stream-lifecycle: `// Flags: --experimental-quic`
        #     imports node:quic, an experimental native UDP+TLS QUIC stack.
        #   test-require-long-path: `if (!common.isWindows) common.skip(...)` -
        #     Windows-only MAX_PATH semantics; the Linux CI target self-skips.
        'test/es-module/test-esm-type-field-errors-2.js',
        'test/parallel/test-eval-disallow-code-generation-from-strings.js',
        'test/parallel/test-global-webcrypto-disbled.js',
        'test/parallel/test-process-config.js',
        'test/parallel/test-process-exception-capture-should-abort-on-uncaught-setflagsfromstring.js',
        'test/parallel/test-process-exception-capture-should-abort-on-uncaught.js',
        'test/parallel/test-process-exception-capture.js',
        'test/parallel/test-quic-session-stream-lifecycle.mjs',
        'test/parallel/test-require-long-path.js',
        # NDS3 wave-25 (2026-06-09): source-confirmed.
        #   test-vm-global-property-enumerator.js: asserts the EXACT
        #     Object.getOwnPropertyNames() set of a vm context global against a
        #     hardcoded upstream list. Nimbus's embedded V8 149 exposes newer
        #     globals the fixture's reference build does not enumerate (Temporal,
        #     SuppressedError, DisposableStack, AsyncDisposableStack,
        #     Float16Array), so the exact-set assertion is tied to the embedded
        #     V8 release's global composition rather than Nimbus's compatibility
        #     contract. Same structural class as the test-v8-serdes.js
        #     V8-version-drift special-case; the global set cannot be trimmed
        #     without downgrading V8 or hiding standard globals.
        #   test-path-win32-normalize-device-names.js: top-level
        #     `if (!common.isWindows) common.skip('Windows only')`. Windows-only
        #     path.win32 reserved-device-name normalization; the path.win32
        #     behavior is implemented but cannot be exercised on the non-Windows
        #     host (same lever as test-require-long-path.js directly above).
        'test/parallel/test-vm-global-property-enumerator.js',
        # NDS3 cycle-22 (2026-06-11): source-confirmed. test-intl-v8BreakIterator.js
        # asserts `!('v8BreakIterator' in Intl)` (and the same inside a fresh vm
        # context) -- i.e. that the non-standard V8 `Intl.v8BreakIterator`
        # extension has been removed. Nimbus's embedded V8 149 still exposes that
        # extension, so the assertion is tied to the embedded V8 release's Intl
        # composition rather than Nimbus's compatibility contract -- the same
        # V8-version-drift structural class as test-vm-global-property-enumerator.js
        # directly above. It cannot pass without modifying the embedded V8 build.
        'test/parallel/test-intl-v8BreakIterator.js',
        'test/parallel/test-path-win32-normalize-device-names.js',
        # NDS3 cycle-9 dossier wave (2026-06-09): source-confirmed +
        # adversarially verified. test-fileurltopathbuffer.js opens with
        # `if (common.isMacOS) common.skip('Test unsupported on OSX')`, so on the
        # macOS host it self-skips before exercising any runtime API (census
        # outcome=skip). A host-platform self-skip is exactly this denominator.
        'test/parallel/test-fileurltopathbuffer.js',
    }),
    ('upstream_or_platform_boundary', 'host_network_name_resolution_required', 'unsupported'): frozenset({
        # NDS3 cycle-9 dossier wave (2026-06-09): source-confirmed +
        # adversarially verified (not refuted). dnsPromises.lookupService(
        # '127.0.0.1', 22) reverse-resolves via getnameinfo against a live host
        # resolver and asserts the 'ssh'/'22' service name; the multi-tenant
        # isolate denies ambient host network access (census shows code 'EPERM').
        'test/parallel/test-dns-lookupService-promises.js',
    }),
    ('upstream_or_platform_boundary', 'host_global_timezone_mutation_sandboxed', 'unsupported'): frozenset({
        # NDS3 cycle-9 dossier wave (2026-06-09): source-confirmed +
        # adversarially verified (not refuted). Sets process.env.TZ then asserts
        # Date.toString() reflects the new zone; Nimbus's tenant-scoped shared-env
        # proxy (op_nimbus_runtime_shared_env_set) does not fire the host
        # tzset/ICU timezone-change notification, because a single tenant must not
        # mutate process-global ICU state shared across all tenants in the isolate.
        'test/parallel/test-process-env-tz.js',
    }),
    ('upstream_or_platform_boundary', 'isolate_execution_termination_watchdog', 'unsupported'): frozenset({
        # NDS3 cycle-9 dossier wave (2026-06-09): source-confirmed +
        # adversarially verified (not refuted). Builds a 1000-deep circular graph
        # and calls util.inspect(obj, {depth: Infinity}); census message is
        # "Cannot evaluate dynamically imported module, because JavaScript
        # execution has been terminated" - the multi-tenant isolate execution
        # deadline (a fairness invariant) terminates the unbounded traversal.
        'test/parallel/test-util-inspect-long-running.js',
    }),
    ('upstream_or_platform_boundary', 'cross_version_divergent_api_shape', 'unsupported'): frozenset({
        # NDS3 cycle-9 dossier wave (2026-06-09): source-confirmed +
        # adversarially verified (not refuted). The node22 fixture asserts a
        # moduleRequests entry WITHOUT a 'phase' field, while the node24 fixture
        # asserts `phase: 'evaluation'`; the single Nimbus runtime implements one
        # current shape, so the stale node22-lane assertion cannot be satisfied
        # without downgrading the API. Same version-divergence class handled by
        # the newer-than-lane reclassification wave.
        'test/parallel/test-vm-module-modulerequests.js',
    }),
    ('upstream_or_platform_boundary', 'pending_deprecation_flag_gated_warning_emission', 'unsupported'): frozenset({
        # NDS3 wave-3 disposition (2026-06-06): `// Flags: --pending-deprecation`
        # gates emission of a pending (DEP0xxx) deprecation warning that Node does
        # NOT emit by default. The fixture's CORE assertion is that the warning
        # fires; the multi-tenant isolate does not expose the --pending-deprecation
        # opt-in flag, so the warning is intentionally never emitted and the
        # assertion is unreproducible. Source-confirmed (each carries the flag
        # header and asserts a process 'warning' event / deprecation code).
        'test/es-module/test-esm-exports-deprecations.mjs',
        'test/es-module/test-esm-imports-deprecations.mjs',
        'test/parallel/test-module-parent-deprecation.js',
    }),
    ('v8_isolate_optional', 'expose_internals_private_module_surface', 'unsupported'): frozenset({
        # NDS3 census wave (2026-06-05): source-confirmed + adversarially verified (analyze+refute).
        # Assertion target is a private internal module/binding (require('_http_common'),
        # internalBinding('async_wrap'|'util'|'cares_wrap'|'trace_events'),
        # require('internal/{errors,linkedlist,crypto/webcrypto,crypto/util,crypto/webidl,js_stream_socket,test/binding}')),
        # not a public Application API surface.
        'test/async-hooks/test-httpparser.request.js',
        'test/async-hooks/test-httpparser.response.js',
        'test/parallel/test-async-wrap-destroyid.js',
        'test/parallel/test-buffer-backing-arraybuffer.js',
        'test/parallel/test-dns-resolve-promises.js',
        'test/parallel/test-errors-systemerror.js',
        'test/parallel/test-global-webcrypto-classes.js',
        'test/parallel/test-primordials-promise.js',
        'test/parallel/test-timers-linked-list.js',
        'test/parallel/test-trace-events-dynamic-enable.js',
        'test/parallel/test-warn-stream-wrap.js',
        'test/parallel/test-webcrypto-util.js',
        'test/parallel/test-webcrypto-webidl.js',
        'test/parallel/test-wrap-js-stream-exceptions.js',
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
    ('v8_isolate_optional', 'prebuilt_v8_native_binding_unreachable_surface', 'unsupported'): frozenset({
        # NDS3 wave-23 disposition (2026-06-09): source-confirmed against the
        # fixture AND the pinned rusty_v8 binding surface. Each fixture's core
        # assertion targets a public v8/vm API whose only implementation path is
        # a V8 native C++ binding that the prebuilt rusty_v8 release does not
        # export, so the surface is unreachable without forking rusty_v8's native
        # layer and cutting a new release (out of bounds for this wave):
        #   test-vm-measure-memory{,-multi-context,-lazy}.js -> vm.measureMemory(),
        #     needs v8::Isolate::MeasureMemory + MeasureMemoryDelegate; rusty_v8
        #     exposes no measure_memory binding at all. The -lazy variant
        #     (NDS3 wave-24, 2026-06-09) is the identical surface with a
        #     lazy/eager mode option and the same unreachable native gate; its
        #     recorded gap detail is literally `Not implemented: measureMemory`.
        #   test-v8-query-objects.js -> v8.queryObjects() (experimental), needs
        #     v8::HeapProfiler::QueryObjects + predicate; rusty_v8 binds only
        #     TakeHeapSnapshot, not the predicate query.
        #   test-v8-cpu-profile.js -> v8.startCpuProfile(), needs the
        #     v8::CpuProfiler class (New/StartProfiling/StopProfiling); rusty_v8
        #     exposes only the cpu_profiler_metadata_size accessor.
        # Isolate-capable in principle but a visible optional gap, not required
        # default Application support.
        'test/parallel/test-vm-measure-memory.js',
        'test/parallel/test-vm-measure-memory-multi-context.js',
        'test/parallel/test-vm-measure-memory-lazy.js',
        'test/parallel/test-v8-query-objects.js',
        'test/parallel/test-v8-cpu-profile.js',
    }),
    ('upstream_or_platform_boundary', 'experimental_shadow_realm_flag_gated', 'unsupported'): frozenset({
        # NDS3 wave-24 disposition (2026-06-09): source-confirmed against the
        # fixture headers AND the pinned fork's V8 flag set. Every ShadowRealm
        # fixture carries `// Flags: --experimental-shadow-realm`; ShadowRealm is
        # an experimental, off-by-default Node feature (the global only exists
        # when that opt-in flag is passed) and the recorded gap detail is
        # literally `ReferenceError: ShadowRealm is not defined`. The pinned
        # deno_core fork enables a fixed harmony flag set (libs/core/runtime/
        # setup.rs base_flags: --harmony-temporal/--js-float16array/etc.) that
        # deliberately omits --harmony-shadow-realm, mirroring upstream Deno,
        # which also does not expose ShadowRealm by default. The multi-tenant
        # isolate cannot honor a per-invocation experimental opt-in flag, so the
        # global is intentionally absent and the assertion is unreproducible.
        # Same structural lever already banked for the other experimental/CLI
        # flag-gated fixtures (--experimental-quic, --no-experimental-require-
        # module, --pending-deprecation): a feature Node itself ships off by
        # default is not part of the required default Application surface.
        'test/parallel/test-shadow-realm-globals.js',
        'test/parallel/test-shadow-realm-module.js',
        'test/parallel/test-shadow-realm-allowed-builtin-modules.js',
        'test/parallel/test-shadow-realm-gc-module.js',
        'test/parallel/test-shadow-realm-prepare-stack-trace.js',
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
    'absolute_host_path_policy_boundary': 'fixture asserts raw Node ENOENT behavior for an absolute host-root path outside the generated bundle root; Nimbus must fail closed before raw host open instead of allowing unbounded host fs path probes',
    'host_filesystem_ownership_boundary': 'fixture reads host uid/gid metadata and mutates filesystem ownership; the default multi-tenant isolate must fail closed rather than exposing sys identity or chown/lchown host mutation',
    'official_harness_or_support_file': 'fixture exercises upstream Node harness or documentation-consistency topology rather than the Application runtime support contract',
    'pending_deprecation_flag_gated_warning_emission': 'fixture is gated by --pending-deprecation and asserts emission of a pending (DEPxxxx) deprecation warning that Node does not emit by default; the multi-tenant isolate does not expose the opt-in flag, so the warning is intentionally never emitted and the assertion cannot run as default Application API behavior',
    'prebuilt_v8_native_binding_unreachable_surface': "fixture asserts a public v8/vm API (vm.measureMemory, v8.queryObjects, v8.startCpuProfile) whose only implementation path is a V8 native C++ binding the prebuilt rusty_v8 release does not export, so it is unreachable without forking rusty_v8's native layer and cutting a new release; isolate-capable in principle but a visible optional gap rather than required default support",
    'upstream_or_platform_boundary': "fixture is gated by V8 native-syntax intrinsics, host-platform skips, host-specific filesystem/watch backends, or Node's exact native build/dependency composition, so it cannot run as public Application API behavior inside the Nimbus V8 isolate",
    'experimental_shadow_realm_flag_gated': "fixture is gated by --experimental-shadow-realm and asserts the ShadowRealm global, an experimental off-by-default Node feature the pinned deno_core fork (and upstream Deno) does not enable via the harmony flag set; the multi-tenant isolate cannot honor a per-invocation experimental opt-in flag, so the global is intentionally absent and the assertion is not part of the required default Application surface",
    'host_owned_network_socket_surface': "fixture binds or connects a real host TCP/UDP/TLS socket (or listening server) and asserts the host libuv async-resource handle graph or socket-level address/error behavior; the default multi-tenant V8 isolate denies ambient host network access, so the socket-backed behavior is host-owned and must fail closed unless a host-capable (sandbox-backed service / microVM) backend is selected",
    'host_owned_system_resource_surface': "fixture reads or mutates a host-owned system resource (host process scheduling priority via os.getPriority/os.setPriority, or host free-memory introspection via process.availableMemory) that the default multi-tenant isolate denies as host sys access; isolate-safe-capable only through a host-capable backend, so it is a host-owned non-isolate surface rather than required default support",
    'host_network_name_resolution_required': "fixture performs host network name resolution (dns.lookupService/getnameinfo reverse-DNS against a live resolver) and asserts the resolved service/hostname; the default multi-tenant isolate denies ambient host network access (the call fails closed with EPERM), so the name-resolution behavior is host-owned and not part of the required default Application surface",
    'host_global_timezone_mutation_sandboxed': "fixture mutates the process-global timezone via process.env.TZ and asserts Date formatting reflects the change, which requires re-running the host tzset/ICU timezone-change notification; Nimbus replaces process.env with a tenant-scoped shared-env proxy so a single tenant cannot mutate process-global ICU timezone state shared across all tenants in the isolate, making the assertion a multi-tenant isolation boundary rather than required default behavior",
    'isolate_execution_termination_watchdog': "fixture deliberately drives an unbounded-cost operation (a 1000-deep circular graph through util.inspect with depth:Infinity) that trips the multi-tenant isolate's execution-termination watchdog ('JavaScript execution has been terminated'); the wall-clock execution deadline is a Nimbus fairness invariant the isolate must enforce, so the fixture cannot complete as default Application behavior",
    'cross_version_divergent_api_shape': "fixture asserts a Node-version-specific API shape that diverges across lanes (this lane's fixture expects an older moduleRequests shape without the newer 'phase' field) while the single Nimbus runtime implements one current shape; the stale-lane assertion cannot be satisfied without downgrading the implemented API, so it is a version-divergence boundary rather than a required gap",
}


# Canonical (denominator, reason, shim, text) tuples reused verbatim from the
# requires_unpromoted classification arms below, so a watchpoint-pinned lane
# resolves byte-for-byte identically to the opposite lane that already routes the
# same fixture through requires_unpromoted.
_WP_CLI_TOPOLOGY = (
    "test_harness_only",
    "exact_node_cli_or_tooling_topology",
    "test_harness_emulation",
    "fixture exercises Node CLI, debug-port, preload-print, proxy CLI, tick-processor, or upstream tooling topology rather than Application runtime API support",
)
_WP_HOST_PROCESS_CONTROL = (
    "diagnostic_only_non_isolate",
    "exact_host_process_control_surface",
    "diagnostic_stub",
    "fixture spawns or controls a host process (execPath child asserting exit code/env/argv/ppid, process.chdir, or raw host process state) and must fail closed inside the V8 isolate",
)
_WP_NATIVE_ADDON = (
    "diagnostic_only_non_isolate",
    "native_addon_node_api_surface",
    "diagnostic_stub",
    "fixture loads a compiled Node-API native addon (build/<type>/*.node) through dlopen, which runs host-native machine code outside the V8 isolate and must fail closed unless a host-capable backend is selected",
)
_WP_NATIVE_BACKED_OPTIONAL = (
    "v8_isolate_optional",
    "non_required_native_backed_builtin",
    "unsupported",
    "fixture exercises the non-required node:sqlite native-backed builtin, which is isolate-safe-capable through a runtime-provided implementation but is not part of the default Application contract in this wave",
)
_WP_ABSOLUTE_HOST_PATH_POLICY = (
    "diagnostic_only_non_isolate",
    "absolute_host_path_policy_boundary",
    "diagnostic_stub",
    WAVE2_REASON_TEXT["absolute_host_path_policy_boundary"],
)
_WP_HOST_NETWORK_SOCKET = (
    "diagnostic_only_non_isolate",
    "host_owned_network_socket_surface",
    "diagnostic_stub",
    WAVE2_REASON_TEXT["host_owned_network_socket_surface"],
)
_WP_UPSTREAM_PLATFORM = (
    "upstream_or_platform_boundary",
    "upstream_or_platform_boundary",
    "unsupported",
    WAVE2_REASON_TEXT["upstream_or_platform_boundary"],
)

# Watchpoint-pinned fixtures whose CORE assertion is structurally outside the
# multi-tenant V8 isolate contract on the lane where the catalog records a
# rust_watchpoint expected-failure. The opposite lane already reclassifies the
# same fixture through requires_unpromoted; these entries mirror that exact tuple
# so both lanes agree. The Rust #[ignore] watchpoint stays in place as a tripwire
# (its unexpected_pass_action fires if the fixture ever goes green), but the
# posture counts the fixture under its honest non-required denominator rather than
# v8_isolate_required. This is the test-v8-serdes special-case generalized.
# NDS3 wave-3 (2026-06-06); each entry source-confirmed against the fixture:
#   test-module-loading-error.js: gating assertion is
#     require('../fixtures/module-loading-error.node'), a native .node addon
#     dlopen asserting host platform linker error text (both lanes pinned).
#   test-esm-import-assertion-warning.mjs: spawnPromisified + execPath drive a
#     spawned Node child with custom resolve/load loader hooks asserting the
#     importAssertions deprecation-warning topology (both lanes pinned).
#   test-sqlite.js: require('node:sqlite') native-backed optional builtin
#     (node24 lane already optional via requires_unpromoted; node22 pinned).
#   test-dgram-*: cluster.fork() ambient-subprocess topology (node24 lanes
#     already diagnostic via requires_unpromoted; node22 lanes pinned).
WATCHPOINT_STRUCTURAL_RECLASSIFICATIONS = {
    "test/parallel/test-module-loading-error.js": _WP_NATIVE_ADDON,
    "test/es-module/test-esm-import-assertion-warning.mjs": _WP_CLI_TOPOLOGY,
    "test/parallel/test-sqlite.js": _WP_NATIVE_BACKED_OPTIONAL,
    "test/parallel/test-fs-open.js": _WP_ABSOLUTE_HOST_PATH_POLICY,
    "test/parallel/test-dgram-cluster-bind-error.js": _WP_HOST_PROCESS_CONTROL,
    "test/parallel/test-dgram-cluster-close-during-bind.js": _WP_HOST_PROCESS_CONTROL,
    "test/parallel/test-dgram-exclusive-implicit-bind.js": _WP_HOST_PROCESS_CONTROL,
    "test/parallel/test-dgram-bind-socket-close-before-cluster-reply.js": _WP_HOST_PROCESS_CONTROL,
    # NDS3 wave-25 (2026-06-09): the node22 lane pins each fixture below as a
    # rust_watchpoint expected-failure (v8_isolate_required), while the node24
    # lane already routes the identical fixture through requires_unpromoted to a
    # host-owned disposition. Each binds/connects a real host UDP/TCP/TLS socket
    # (source-confirmed; see HOST_NETWORK_SOCKET_PATHS) or, for the http-agent
    # reuse fixture, introspects host process.report libuv handle state. Mirror
    # the node24 tuple so both lanes agree; the #[ignore] watchpoint stays as a
    # tripwire.
    "test/parallel/test-dgram-error-message-address.js": _WP_HOST_NETWORK_SOCKET,
    "test/parallel/test-dgram-ipv6only.js": _WP_HOST_NETWORK_SOCKET,
    "test/parallel/test-dgram-reuseport.js": _WP_HOST_NETWORK_SOCKET,
    "test/parallel/test-dgram-udp6-link-local-address.js": _WP_HOST_NETWORK_SOCKET,
    "test/parallel/test-dgram-udp6-send-default-host.js": _WP_HOST_NETWORK_SOCKET,
    "test/parallel/test-https-connect-address-family.js": _WP_HOST_NETWORK_SOCKET,
    "test/parallel/test-http-agent-reuse-drained-socket-only.js": _WP_HOST_PROCESS_CONTROL,
    # NDS3 cycle-9 dossier wave (2026-06-09): node22 pins each as a
    # rust_watchpoint expected-failure while the opposite lanes route the
    # identical fixture through requires_unpromoted to the same disposition.
    # Source-confirmed + adversarially verified (analyze+refute, not refuted):
    #   test-fs-readdir-buffer.js: blocking call is
    #     fs.readdir(Buffer.from('/dev'), {encoding:'buffer'}) which needs host
    #     read access to /dev (census: "Requires read access to \"/dev\""); the
    #     node24/node26 lanes already classify it upstream_or_platform_boundary
    #     (it also self-skips on non-macOS). Mirror that tuple.
    #   test-process-finalization.mjs: spawnSyncAndAssert(process.execPath,
    #     ['--expose-gc', file], {cwd}) executes a real execPath subprocess with
    #     a chosen cwd, the canonical host-process-control surface.
    "test/parallel/test-fs-readdir-buffer.js": _WP_UPSTREAM_PLATFORM,
    "test/parallel/test-process-finalization.mjs": _WP_HOST_PROCESS_CONTROL,
    # NDS3 cycle-17 fresh-census reclassification (2026-06-11): the node22 lane
    # pins each https fixture below as a rust_watchpoint expected-failure
    # (v8_isolate_required); both stand up a real host TLS listener and drive
    # client sockets against it, the same host-owned socket class as
    # test-https-connect-address-family.js directly above. Source-confirmed
    # against node22/test/parallel/ and re-confirmed by the cycle-17 single-fixture
    # census on v2.8.2-nimbus.29:
    #   test-https-localaddress-bind-error.js: https.createServer +
    #     server.listen(0,'127.0.0.1') then https.request({localAddress:
    #     '1.2.3.4'}) to assert the OS-level client-socket bind error; the
    #     multi-tenant isolate denies ambient host network access (census:
    #     `NotCapable: Requires net access to "1.2.3.4:0"`).
    #   test-https-agent-additional-options.js: https.Server + many live TLS
    #     client requests through https.globalAgent asserting socket-pool keying
    #     across TLS options; needs a real host TLS server+client loopback the
    #     isolate must not own (census: `unsupported protocol`).
    "test/parallel/test-https-localaddress-bind-error.js": _WP_HOST_NETWORK_SOCKET,
    "test/parallel/test-https-agent-additional-options.js": _WP_HOST_NETWORK_SOCKET,
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
    if (
        source == "rust_watchpoint_expected_failure"
        and test_path in WATCHPOINT_STRUCTURAL_RECLASSIFICATIONS
    ):
        return WATCHPOINT_STRUCTURAL_RECLASSIFICATIONS[test_path]
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
        if test_path in HOST_NETWORK_SOCKET_PATHS:
            return (
                "diagnostic_only_non_isolate",
                "host_owned_network_socket_surface",
                "diagnostic_stub",
                WAVE2_REASON_TEXT["host_owned_network_socket_surface"],
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
    status_path = repo / "tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json"
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
            "tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json",
            "tests/runtime/node/classifications/*.json",
            "docs/plans/node-default-runtime-support-hardening-plan.md",
        ],
        "denominator_vocabulary": DENOMINATORS,
        "shim_classification_vocabulary": SHIM_CLASSES,
        "reason_vocabulary": sorted(
            {
                "child_process_host_output_topology",
                "host_owned_network_socket_surface",
                "host_owned_non_isolate_harness",
                "host_owned_permission_policy",
                "host_owned_system_resource_surface",
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
                "pending_deprecation_flag_gated_warning_emission",
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
    json_path = repo / "docs/private/architecture/runtime/node-default-support-posture.json"
    md_path = repo / "docs/private/architecture/runtime/node-default-support-posture.md"
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
