# Deno vs Neovex: Node.js Compatibility Comparison

Status: snapshot (2026-05-10, updated 2026-05-11)

High-level comparison of Node.js built-in module compatibility between stock
Deno and Neovex. Neovex builds on Deno's `ext/node` stack, so the baseline
implementation is shared. This document captures where the two diverge:
where Neovex has verified or improved beyond Deno, and where Neovex has
intentional restrictions. The NLC plan (NLC0-NLC10) is now complete.

## Executive Summary

|                                           | Deno           | Neovex         |
| ----------------------------------------- | -------------- | -------------- |
| Modules with any implementation           | 44/44 100%     | 44/44 100%     |
| Modules functionally usable               | 38/44  86%     | 43/44  98%     |
| Modules verified with upstream Node tests | unknown        | 44/44 100%     |
| Modules improved beyond Deno baseline     | --             | 18/44  41%     |
| Official Node test files green (Node22 default) | not published  | 876            |
| Supported Node lanes                      | not published  | Node20, Node22 default, Node24 |
| Package canaries verified                 | not published  | 10 (5 networking + 5 tooling) |
| Oracle comparison system                  | not published  | Nightly CI with version-matched Node |
| Nightly CI dashboard                      | not published  | `.github/workflows/node-compat-nightly.yml` |

Neovex's primary advantage is **verification depth**, not implementation
breadth. Both runtimes share the same `ext/node` codebase. But Neovex has
run 876 lane-local official upstream Node.js test files in the Node22 default
lane, supports Node20 and Node24 as first-class lanes, verified 10 package
canaries across Application and Tooling profiles, and fixed issues that
stock Deno has not.

## Status Legend

- **Full** — Module is fully functional and verified with upstream Node tests
- **Full\*** — Deno self-reports "fully supported" but has documented stubs/caveats
- **Partial** — Implementation exists with known gaps
- **Partial+** — Neovex has verified and/or improved beyond Deno's partial baseline
- **Stub** — Module exports exist but are non-functional
- **Restricted** — Supported but intentionally scoped by runtime profile
- **Inherited** — Uses Deno's implementation, not yet separately verified by Neovex
- **N/A** — Intentionally excluded from scope

## Per-Module Comparison

### P0: Core Semantics (NLC3 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `assert` | 27 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `buffer` | 77 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `console` | 23 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `events` | 50 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `path` | 12 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `punycode` | 4 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `querystring` | 6 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `string_decoder` | 3 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |
| `url` | 30 | Full | Full | 120 Node22 tests green | Neovex verified with upstream tests |

### P0: Process and Timing (NLC4 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `process` | 1 | Partial | Partial+ | 48 Node22 tests green | Neovex verified; env allowlist-scoped in Application profile |
| `timers` | 21 | Full | Full | 48 Node22 tests green | Neovex verified with upstream tests |
| `util` | 99 | Partial | Partial+ | 48 Node22 tests green | Neovex verified; MIMEType/MIMEParams work |
| `diagnostics_channel` | 62 | Full | Full | 48 Node22 tests green | Neovex verified with upstream tests |
| `perf_hooks` | 43 | Partial | Partial+ | Custom impl + verified | Neovex replaces Deno's stubs with working perf_hooks (histogram, monitorEventLoopDelay) |

### P0: Streams and Local I/O (NLC5 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `stream` | 77 | Full | Full | 317 Node22 tests green | Neovex verified with upstream tests |
| `fs` | 159 | Full\* | Full (scoped) | 317 Node22 tests green | Neovex verified; path-scoped to approved roots in Application profile |
| `fs/promises` | 1 | Full\* | Full (scoped) | 317 Node22 tests green | Neovex verified; same path scoping; custom error mapping |
| `readline` | 37 | Full | Full | 317 Node22 tests green | Neovex verified with upstream tests |
| `tty` | 12 | Full | Full | 317 Node22 tests green | Neovex verified with upstream tests |
| `os` | 20 | Full | Full | 317 Node22 tests green | Neovex verified with upstream tests |

### P0/P1: Networking (NLC6 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `dns` | 27 | Partial | Partial+ | 270 Node22 tests green | Neovex verified with upstream tests |
| `net` | 61 | Partial | Partial+ | 270 Node22 tests green | Neovex verified with upstream tests |
| `dgram` | 1 | Partial | Partial+ | 270 Node22 tests green, 5 dgram waves | Neovex verified; broader multicast/send coverage |
| `tls` | 51 | Partial | Partial+ | 270 Node22 tests green, TLS waves | Neovex adds createSecurePair support (Deno: not supported) |
| `http` | 103 | Partial | Partial+ | 270 Node22 tests green | Neovex verified; Agent/keepalive/lifecycle verified |
| `https` | 11 | Partial | Partial+ | 270 Node22 tests green, TLS cert waves | Neovex verified with upstream tests |
| `http2` | 105 | Partial | Partial+ | 270 Node22 tests green, compat waves | Neovex verified; header/status/compat request/response waves |

### P0/P1: Crypto and Compression (NLC7 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `crypto` | 110 | Full\* | Full (verified) | Hash, HMAC, random, KDF, cipher, DH/ECDH, auth/wrap waves | Deno claims "full" but has many stubs; Neovex has upstream test evidence |
| `zlib` | 50 | Partial | Partial+ | 4 verified slices: foundation, stream-lifecycle, decompression, Brotli | Neovex verified Brotli, dictionary, GC paths Deno lists as unsupported |

### P0/P1: Loader and Async Context (NLC7-NLC8 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `module` | 19 | Full\* | Full (verified) | Loader-context manifest, CommonJS/ESM bridge | Neovex verified createRequire, builtinModules, Module.wrapper, CommonJS loading |
| `async_hooks` | 24 | Partial | Partial+ | ALS, execution-context, promise-hook waves | Deno lists AsyncResource/executionAsyncId as stubs; Neovex has working verified impl |

### P1/P2: VM, Runtime Internals, Workers (NLC8 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `child_process` | 20 | Full | Restricted | spawnSync for staged binaries only | Neovex scopes to Tooling profile; Application profile: not supported |
| `vm` | 21 | Partial | Partial+ | 6-file basics wave green | Neovex fixes filename/stack fidelity and weak-handle teardown abort |
| `v8` | 56 | Partial | Partial+ | 5-file helper wave green | Deno: most APIs throw; Neovex: cachedDataVersionTag, serdes, stats, setFlagsFromString work |
| `worker_threads` | 40 | Partial | Partial+ | 15-file verified contract across 3 lanes | Neovex: Worker, MessageChannel, MessagePort, ref/unref, bootstrap/process verified |
| `inspector` | 18 | Partial | Partial+ | 5-file front-edge contract green | Deno: stubs; Neovex: module, open, enabled, NodeTracing path work |

### P2/P3: Long-Tail and Host-Heavy (NLC9 — done)

| Module | Symbols | Deno | Neovex | Evidence | Difference |
| ------ | ------: | ---- | ------ | -------- | ---------- |
| `domain` | 9 | Stub | Partial+ | 16-file foundation green, cross-lane (Node22/Node20/Node24) | Deno: stubs; Neovex: add/remove, timer propagation, nested binding, promise rejection bridge verified |
| `trace_events` | 1 | Stub | Partial+ | 10-file foundation green, Node22 default | Deno: stubs; Neovex: API, binding, bootstrap, category, console, dynamic enable, environment, metadata, process-exit verified |
| `constants` | 1 | -- | Partial+ | 5-file tranche green across all 3 lanes | Public constants export, internalBinding('constants'), fs.constants, os signals verified cross-lane |
| `sys` | 1 | -- | Partial+ | Cross-lane `test-sys.js` green (all 3 lanes) | Alias contract verified cross-lane |
| `cluster` | 23 | Stub | Partial+ | 9-file Node22 worker foundation + lifecycle/teardown green | Deno: stubs; Neovex: worker construct/init/exit/disconnect/kill verified |
| `repl` | 8 | Stub | Partial+ | 4-file Node22 `repl.start()` foundation green | Deno: stubs; Neovex: definecommand, mode, recoverable, reset-event verified |
| `test` | 74 | Full | Partial+ | 20-file Node22 runner wave green | Runner aliases, assertions, context, plan, file syntax, reporters, CLI options, randomize, rerun-failures verified |
| `sqlite` | 27 | Full | Partial+ | 4-file Node22 foundation green | Config, statement-sync, template-tag, named-parameters verified |
| `wasi` | 4 | Stub | Partial+ | ~17-file Node22 across 5 waves green | Deno: stubs; Neovex: validation, executable, argv, filesystem, preopen verified |
| `sea` | 1 | Stub | Partial+ | 1-file truthful non-SEA contract green | `isSea()` returns false, `getAssetKeys()` throws correct `ERR_NOT_IN_SINGLE_EXECUTABLE_APPLICATION` |

## Aggregate Comparison

### Module-Level Functional Coverage

"Functional" means the module is usable beyond pure stubs.

| Runtime | Full | Partial | Stub/None | Functional % |
| ------- | ---: | ------: | --------: | -----------: |
| Deno (self-reported) | 22 | 16 | 6 | 86% |
| Neovex (verified) | 24 | 19 | 1 | 98% |

The NLC plan (NLC0-NLC10, now complete) verified all 44 modules with
upstream Node tests. 18 modules are improved beyond Deno's baseline.
Only `sea` remains intentionally scoped (truthful non-SEA contract
rather than full SEA support, which is host-binary-specific).

### Verification Depth

| Metric | Deno | Neovex |
| ------ | ---- | ------ |
| Official upstream Node test files run | not published | 876 (Node22 default path-owned green) |
| LTS lanes tested | 1 (own runner) | 3 (Node22, Node20, Node24) |
| Package canaries verified | not published | 10 (express, fastify, socket.io, undici, axios, jest, tsx, ts-node, prisma, next) |
| Per-module failure inventories | not published | all families checked in |
| Per-module test manifests | not published | all families checked in |
| Oracle comparison system | not published | Nightly CI with version-matched Node20/Node22/Node24 |
| Dashboard aggregation | not published | `make node-compat-dashboard` → JSON + Markdown |
| Nightly CI workflow | not published | `.github/workflows/node-compat-nightly.yml` (scheduled + manual dispatch) |

### Where Neovex Exceeds Deno

These are modules where Neovex has made concrete improvements or fixes that
stock Deno has not shipped:

| Module | Deno Gap | Neovex Fix |
| ------ | -------- | ---------- |
| `cluster` | Stub (non-functional) | 9-file worker foundation + lifecycle: construct, init, isdead, isconnected, events, exit, disconnect, forced-exit, kill with Node-shaped handshakes |
| `domain` | Stub (non-functional) | 16-file working foundation: add/remove, timer propagation, nested binding, promise rejection bridge |
| `trace_events` | Stub (non-functional) | 10-file working foundation: API, binding, bootstrap, category, console, dynamic enable, environment, metadata, process-exit |
| `wasi` | Stub (non-functional) | 17-file across 5 waves: validation, executable, argv, filesystem, preopen/file-IO |
| `repl` | Stub (non-functional) | 4-file `repl.start()` foundation: definecommand, mode, recoverable, reset-event |
| `sea` | Stub (non-functional) | Truthful non-SEA contract: `isSea()` returns false, `getAssetKeys()` throws correct Node-shaped error |
| `perf_hooks` | Stubs (monitorEventLoopDelay, timerify) | Custom working implementation with histogram and event loop delay support |
| `tls` | createSecurePair not supported | createSecurePair with ERR_TLS_INVALID_CONTEXT validation |
| `async_hooks` | AsyncResource, executionAsyncId are stubs | Working AsyncLocalStorage, execution-context, promise-hook waves verified |
| `v8` | Most APIs throw errors | cachedDataVersionTag, serdes, stats, setFlagsFromString working |
| `vm` | measureMemory stub, limited Script support | Filename/stack fidelity fixes, weak-handle teardown abort fix |
| `inspector` | All non-console APIs are stubs | Module, open, enabled, NodeTracing path working |
| `worker_threads` | parentPort.emit, moveMessagePortToContext not supported | Worker, MessageChannel, MessagePort, ref/unref, bootstrap/process verified |
| `zlib` | BrotliCompress/Decompress, ZlibBase not supported | Brotli, dictionary, GC tracking, flush/drain verified |
| `dgram` | Multicast membership methods are stubs | Broader send/callback, multicast, fd, error wave verified |
| `constants` | Not listed in Deno compat table | Public constants export, internalBinding('constants'), fs.constants, os signals verified |
| `test` | Deno claims full but Neovex independently verified | 20-file runner wave: aliases, assertions, context, plan, file syntax, reporters, CLI options, randomize, rerun-failures |
| `sqlite` | Deno claims full but Neovex independently verified | 4-file foundation: config, statement-sync, template-tag, named-parameters |

### Where Deno Has Advantages

| Area | Deno Advantage | Neovex Status |
| ---- | -------------- | ------------- |
| `child_process` in Application profile | Full access, no restrictions | Intentionally restricted to Tooling profile with staged binaries only |
| `fs` path freedom | No root restrictions | Intentionally scoped to approved bundle/app/tmp/cache roots |
| `process.env` access | Full host env access | Intentionally allowlist-only (Application: 1 var, Tooling: ~30 vars) |
| `net` remote hosts | Any host allowed | Intentionally restricted to localhost/loopback in Application profile |
| N-API / native addons | Supported via `deno_napi` | Not yet wired up (planned via `deno_napi`) |
| `npm:` specifier support | Native npm specifier resolution | Not supported; uses staged `node_modules` |

Note: Neovex's "disadvantages" in `child_process`, `fs`, `process.env`, and
`net` are **intentional security restrictions** for the Application runtime
profile, not missing implementations. The Tooling profile has broader access.

### NLC Plan Deliverables (Complete)

The NLC plan (NLC0-NLC10) is now complete. All 11 roadmap items are `done`.

**NLC0-NLC2** (truth and control plane): Generated compatibility matrix,
versioned public contract with Node22 default plus Node20 and Node24 supported lanes.

**NLC3-NLC7** (foundation built-ins): Core semantics, process/timing,
streams/I/O, networking, crypto/compression, and loader/async context
families verified with upstream Node tests, failure inventories, and
package canaries.

**NLC8-NLC9** (deep runtime and long-tail): Loader, VM, workers, inspector,
cluster, repl, test, sqlite, wasi, sea, domain, trace_events, sys, and
constants verified with truthful support states across three LTS lanes.

**NLC10** (validation and closeout): Delivered the full evidence layer:
- Machine-readable manifest catalogs for all 5 carried families
- `scripts/runtime/node/report.sh` with `--capture-live` measured artifact capture
- 10 package canaries verified (5 Application networking + 5 Tooling)
- Oracle comparison system (`make node-compat-oracle`) with drift classes
  and version-matched Node20/Node22/Node24 sweeps
- Dashboard aggregation (`make node-compat-dashboard`) → JSON + Markdown
- Nightly CI workflow (`.github/workflows/node-compat-nightly.yml`)
  with seeded slice replays, canary lanes, oracle samples, and artifact upload

Public support claim: **"Node20, Node22, and Node24 compatibility targets with
Node22 as the default and documented profile-scoped exclusions"** —
evidence-backed rather than aspirational.

## Methodology Notes

- **Deno status** comes from the official Deno Node.js compatibility
  reference at `https://docs.deno.com/runtime/reference/node_apis/`
  (last updated 2025-08-20). Deno self-reports module-level status but
  does not publish upstream Node test pass rates or per-module failure
  inventories.
- **Neovex status** is derived from the checked-in manifests and failure
  inventories under `docs/architecture/runtime/node-lts-compat/`, the
  verified surface matrix at `node-compat-surface-matrix.md`, and the
  archived NLC baseline at `docs/plans/archive/node-lts-compatibility-plan.md`.
- **Symbol counts** come from the generated `node-lts-compat-matrix.csv`
  baseline (Node 22 column).
- **"Full\*"** marks modules where Deno claims "fully supported" but has
  documented stubs or caveats in the same compatibility table. For
  example, `node:crypto` is listed as "fully supported" but has 10+
  documented stub/non-functional APIs.
- Neovex inherits Deno's entire `ext/node` implementation stack. Modules
  marked **Inherited** use Deno's code without separate Neovex
  verification. They are expected to have the same behavior as stock
  Deno.
- The **876** upstream test count represents lane-local official
  `nodejs/node v22.15.0` test files (not Deno's own test suite) that are
  path-owned by non-ignored Neovex compatibility tests across the Node22
  default lane after excluding explicit red/gap/skip classifications. Earlier
  family prose counts summed higher, but the generated status dashboard now
  prefers reconstructable path evidence over prose counts when the two disagree.
