# Node.js Compatibility Test Architecture: Cross-Runtime Survey and Improvement Recommendations

**Date:** 2026-05-08
**Scope:** Research survey of how alternative JavaScript runtimes structure their Node.js
compatibility test suites, compared with Nimbus's current implementation, to inform a
refactoring plan that is future-proof across many Node LTS versions and inspires enterprise
trust.

**Runtimes and harnesses surveyed:** Deno, Bun, Cloudflare workerd, Node.js core harness,
Node.js WPT runner
**Local reference repos:**

- Nimbus: `/Users/jack/src/github.com/nimbus/nimbus`
- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- Upstream Deno: `/Users/jack/src/github.com/denoland/deno`
- Upstream Node: `/Users/jack/src/github.com/nodejs/node`

---

## Table of Contents

1. [Nimbus Current State](#1-nimbus-current-state)
2. [Deno](#2-deno)
3. [Bun](#3-bun)
4. [Cloudflare workerd](#4-cloudflare-workerd)
5. [Node.js Core Harness and WPT](#5-nodejs-core-harness-and-wpt)
6. [Tooling and Management Workflows](#6-tooling-and-management-workflows)
7. [Comparative Analysis](#7-comparative-analysis)
8. [Gap Analysis: Nimbus vs. Industry Patterns](#8-gap-analysis-nimbus-vs-industry-patterns)
9. [Recommended Improvement Plan](#9-recommended-improvement-plan)

---

## 1. Nimbus Current State

### 1.1 File layout

All Node compatibility tests live under
`crates/nimbus-runtime/src/runtime/tests/`. The module is declared in
`runtime.rs` as `#[cfg(test)] mod tests { mod node_compat; ... }`.

| File | Lines | Role |
|------|-------|------|
| `node_compat.rs` | 6,904 | Macros, batch constants, infrastructure, and `#[test]` functions |
| `support.rs` | — | `RecordingHost`, test auth, invocation helpers |

Source: `wc -l crates/nimbus-runtime/src/runtime/tests/node/mod.rs` → 6,904.

### 1.2 Key metrics

| Metric | Value |
|--------|-------|
| `#[test]` functions | 213 |
| `#[ignore]` annotations | 60 |
| Rust macros defined | 9 |
| Macro invocations (batch entries) | 1,194 |
| Unique `test_relative_path` entries | 877 |
| Batch constant arrays | 43 |
| Fixture files: node20/ | 1,291 |
| Fixture files: node22/ | 1,215 |
| Fixture files: node24/ | 1,479 |
| Fixture files: shared (vendored/patched) | 154 |
| Total fixture disk size | 20 MB |
| Byte-identical files: node20 vs. node22 | 1,023 of 1,178 overlapping (87%) |
| Byte-identical files: all three versions | 739 of 1,166 overlapping (63%) |

Source: measured counts from the working tree as of 2026-05-08.

### 1.3 Three-lane model

Nimbus tests against three Node.js versions simultaneously:

| Lane | Enum variant | Role | CI status |
|------|-------------|------|-----------|
| Node 20 | `NodeCompatLane::Node20` | Validation lane — preserves installed-base compatibility with the previous LTS/EOL line | Active |
| Node 22 | `NodeCompatLane::Node22` | Primary lane — the runtime's target shape | Active |
| Node 24 | `NodeCompatLane::Node24` | Preview lane — forward visibility for the newer Node 24 line | `#[ignore]`d |

All three lanes use `RuntimeLimits::application_node22()` because the runtime is
Node 22-shaped. The preview lane sets `capture_top_level_skip = true` so import
errors become skips rather than failures.

As of 2026-05-08, upstream Node lifecycle status is: Node 20 EOL as of
2026-03-24, Node 22 LTS, and Node 24 LTS. Nimbus's lane roles are harness roles,
not upstream-support labels.

Source: `node_compat.rs` lines 101–112 (enum), line 3041 (Node24 skip logic).

### 1.4 Compatibility family slices

Batch constants map to domain-owned compatibility families:

| Batch constant | Compatibility family | Approximate entry count |
|----------------|----------------------|------------------------|
| `CORE_SEMANTICS_BATCH` | Core semantics | ~200 |
| `PROCESS_AND_TIMING_BATCH` | Process and timing | ~80 |
| `STREAMS_AND_LOCAL_IO_BATCH` | Streams and local I/O | ~150 |
| `NETWORKING_BATCH` | Networking | ~400 |
| `LOADER_CONTEXT_BATCH` | Loader context | ~300+ |

Source: `grep -c` on batch constant ranges in `node_compat.rs`.

### 1.5 Code breakdown by section

| Section | Line range | Approx. lines | Content |
|---------|-----------|---------------|---------|
| Infrastructure | 1–916 | ~916 | Imports, preludes, macros, types, env helpers |
| Batch constants | 917–4,899 | ~3,983 | 43 `const` arrays of `NodeCompatBatchEntry` plus extra-fixture tables |
| Infrastructure functions | 2,745–3,499 | ~755 | Bundle writer, executor, runners (overlapping with batch section) |
| Test functions | 4,900–6,904 | ~2,005 | 213 `#[test]` declarations |

Source: `grep -n` analysis of function and constant boundaries.

### 1.6 Execution pipeline

```
#[test] function
  → run_node_compat_watchpoint() or run_manifested_subset_for_lane()
    → execute_manifested_node_compat_test()
      reads fixture source + extra files from disk via include paths
      → execute_upstream_node_compat_test_with_extra_files()
        1. acquire_runtime_suite_lock()         (global serialization)
        2. detect --pending-deprecation         (scan first 40 lines)
        3. ScopedProcessEnvVar::set()           (TERM, NODE_OPTIONS)
        4. write_node_compat_bundle()           (temp dir with ESM entry + fixtures)
        5. NimbusRuntime::invoke_bundle()       (V8 execution)
        6. validate JSON: { ok: true, testPath, skipped }
```

Source: `node_compat.rs` lines 2,843–2,897.

### 1.7 Fixture provenance

**There is no record of which `nodejs/node` tag each `nodeXX/` directory was
pulled from.** The plan mentions "pinned upstream test corpus" but no version
stamps, sync dates, or tooling exist.

Source: searched `docs/plans/archive/node-lts-compatibility-plan.md`, `node_compat.rs`,
and fixture directories for version markers.

### 1.8 Strengths of the current system

- Runs **real upstream Node.js tests** — highest-confidence compat signal.
- **Three-lane model** is unique among surveyed runtimes — nobody else proves compat
  across three major Node lines simultaneously.
- **Prelude/postlude injection** avoids patching upstream fixtures, keeping sync clean.
- **Batch runner with panic-catching** produces per-fixture results within a single
  `#[test]` function.
- **`#[ignore]` with descriptive reasons** documents every known gap inline.
- **Shared test harness** (`test/common/index.js`) provides real `mustCall` assertion
  semantics.

### 1.9 Structural pressure points

- `node_compat.rs` at 6,904 lines exceeds the repo's own 2,000-line modularity
  threshold by 3.4×.
- ~1,000 byte-identical files are stored three times on disk.
- 9 macros grow combinatorially with each new Node version.
- No fixture provenance tracking or sync tooling.
- No structured reporting output.
- Status information (ignore reasons, expected failures) is locked in Rust source,
  not machine-readable.

---

## 2. Deno

### 2.1 Overview

Deno maintains the most mature structured Node.js compatibility test
infrastructure among the surveyed runtimes. Tests live in a dedicated directory
with a JSONC manifest, JSON Schema, Rust runner, and automated CI with public
reporting.

### 2.2 Repository layout

```
tests/node_compat/
├── config.jsonc              ← master manifest (3,783 lines, ~2,908 entries)
├── schema.json               ← JSON Schema (draft-07, 86 lines)
├── mod.rs                    ← Rust test runner (902 lines)
├── report.rs                 ← JSON report generation
├── Cargo.toml
├── deno.json
├── slack.ts                  ← Slack notification integration
├── add_day_summary_to_month_summary.ts
└── runner/
    └── suite/                ← git submodule → github.com/denoland/node_test
        ├── test/             ← vendored Node.js test files
        ├── node_version.ts   ← single source of truth: "25.8.1"
        └── vendor.ts         ← 20-line sync script
```

Source: `ls ~/src/github.com/nimbus/deno/tests/node_compat/`, verified
against the nimbus Deno fork checked out locally.

### 2.3 Manifest format (`config.jsonc`)

The manifest is a flat map from test path to configuration object, validated by
a JSON Schema:

```jsonc
{
  "$schema": "./schema.json",
  "tests": {
    "parallel/test-assert.js": {},
    "parallel/test-crypto-x509.js": {
      "ignore": true,
      "reason": "requires OpenSSL FIPS"
    },
    "parallel/test-http-timeout.js": {
      "windows": false,
      "darwin": { "exitCode": 1, "output": "[WILDCARD]timeout[WILDCARD]" }
    },
    "parallel/test-buffer-alloc.js": { "flaky": true }
  }
}
```

| Field | Type | Purpose |
|-------|------|---------|
| `ignore` | bool | Skip on all platforms (requires `reason`) |
| `flaky` | bool | Retry up to 3 times in CI |
| `windows` / `darwin` / `linux` | bool or `ExpectedFailure` | Per-platform skip or expected failure |
| `exitCode` | int | Expected exit code (global or per-platform) |
| `output` | string | Expected output pattern with `[WILDCARD]` |
| `reason` | string | Explanation for skip or special config |

Status breakdown (nimbus fork):

| Status | Count |
|--------|-------|
| Total entries | 2,908 |
| Ignored (`"ignore": true`) | 224 |
| Flaky (`"flaky": true`) | 12 |
| Expected failure (`"exitCode"`) | 4 |
| Platform-specific (`"darwin"`) | 13 |
| Platform-specific (`"windows"`) | 33 |

Source: `grep -c` against
`~/src/github.com/nimbus/deno/tests/node_compat/config.jsonc`.

### 2.4 Schema validation

The JSON Schema (`schema.json`, 86 lines) defines the `testConfig` type and
`expectedFailure` / `platformExpectation` sub-types. The Rust runner
deserializes into matching structs:

```rust
struct TestConfig {
    flaky: bool,
    ignore: bool,
    windows: Option<PlatformExpectation>,
    darwin: Option<PlatformExpectation>,
    linux: Option<PlatformExpectation>,
    reason: Option<String>,
    exit_code: Option<i32>,
    output: Option<String>,
}
```

Source: `~/src/github.com/nimbus/deno/tests/node_compat/mod.rs` lines
52–98; schema at `~/src/github.com/nimbus/deno/tests/node_compat/schema.json`.

### 2.5 Runner architecture

The Rust runner (`mod.rs`, 902 lines) follows this pipeline:

1. **Load** `config.jsonc` → `HashMap<String, TestConfig>`
2. **Discover** test files under `runner/suite/test/` matching `test-*.{js,mjs,cjs,ts}`
3. **Filter** to entries present in config (opt-in model)
4. **Shard** via `CI_SHARD_INDEX` / `CI_SHARD_TOTAL` environment variables
5. **Partition** into sequential (`sequential/` prefix, parallelism=1) and parallel
6. **Execute** each test via `deno run -A --quiet --unstable-*` or `deno test`
7. **Validate** exit code and output against expected failure config
8. **Retry** flaky tests up to 3 times
9. **Report** structured JSON (`report.json`)

Source: `mod.rs` lines 1–120 and function signatures throughout.

### 2.6 Version tracking and sync tooling

Version is tracked in a single file:

```typescript
// runner/suite/node_version.ts
export const version = "25.8.1";
```

Sync is performed by `vendor.ts` (20 lines):

```typescript
const tag = "v" + version;
await $`git clone --depth 1 --sparse --branch ${tag} --single-branch https://github.com/nodejs/node.git`;
await $`git sparse-checkout add test`.cwd("node");
await $`cp -r node/test ./test`;
```

The vendored tests live in a separate repository (`github.com/denoland/node_test`)
managed as a git submodule.

Source: `~/src/github.com/nimbus/deno/tests/node_compat/runner/suite/vendor.ts`
and `node_version.ts`.

### 2.7 CI and reporting

- **Workflow:** `.github/workflows/node_compat_test.generated.yml`
- **Schedule:** Daily at 10 AM UTC + manual dispatch
- **Matrix:** 9 jobs — 3 OS (Linux, Windows, macOS) × 3 shards
- **Report:** JSON with per-test results, Deno version, Node version, OS, arch
- **Storage:** S3 at `dl.deno.land/node-compat-test/`
- **Dashboard:** Public at `node-test-viewer.deno.dev/results/latest`
- **Notifications:** Slack integration via `slack.ts`

Reference: https://github.com/denoland/deno/blob/main/.github/workflows/node_compat_test.generated.yml

### 2.8 Multi-version strategy

**Deno targets one Node.js version at a time.** When upstream Node releases a new
version, the submodule is bumped. There is no multi-version lane system. The
nimbus fork tracks Node 25.8.1; upstream Deno tracks ~24.x.

This is the key architectural divergence from Nimbus: Deno accepts that their
Node compat surface tracks the latest release, while Nimbus explicitly validates
backward compatibility with previous LTS versions.

### 2.9 Supplementary tests beyond vendored Node fixtures

Beyond the large vendored Node fixture surface in `tests/node_compat`, Deno
also maintains substantial supplementary coverage that the Deno team wrote
themselves. A single rolled-up count is brittle because the local
`nimbus/deno` comparison branch and upstream `denoland/deno` `main`
already differ. The stable pattern is the important part: Deno carries dozens
of focused `tests/unit_node/` files plus hundreds of `tests/specs/node/`
behavioral fixtures that verify alternative-runtime behavior directly.

#### Test locations

Measured on 2026-05-09:

| Path | Local `nimbus/deno` comparison branch | Upstream `denoland/deno` `main` | Level |
|------|--------------------------------------------|----------------------------------|-------|
| `tests/unit_node/` | 83 `*_test.*` files | 82 `*_test.*` files | Unit-level module verification |
| `tests/specs/node/` | 90 top-level spec directories / 455 files | 102 top-level spec directories / 553 files | Integration-level behavioral verification |

#### Categories and gap coverage

**CJS/ESM bridge (13+ specs).** The single largest category. Node has no CJS/ESM
bridge to test from the outside — it IS the reference. Deno must verify that its
dual-module system faithfully emulates Node's behavior:

- `dynamic_import_and_require_dual/` — `.cts`, `.mjs`, `.mts` resolution
- `require_esm_module_exports/` — CJS `require()` of ESM with `export default`
- `cjs_dynamic_import_esm_with_exports/` — dynamic imports with conditional
  `exports` field
- `detect_es_module_defined_as_cjs/` — heuristic detection of mislabeled modules
- `esm_dir_import/` — directory resolution in ESM context

**Process lifecycle and globals (8+ specs).** Node does have targeted tests for
some of these surfaces, but Deno still needs direct coverage of global
injection and process shape from the perspective of an alternative runtime:

- `process_stdout_indestructible/` — `process.stdout` cannot be destroyed
- `process_argv0/`, `process_title/` — process identity
- `node_process_beforeexit_exit_events_emitted_without_listeners/` — event
  lifecycle without explicit listeners

**Child process translation (9 specs).** Deno's CLI is not Node's CLI, so
`child_process.fork()` must translate arguments:

- `child_process_fork_deno_args/` — vitest passes pre-translated Deno args;
  Deno must not double-translate
- `child_process_node_cli_args/` — translating `--require` → `--import`
- `child_process_shell_escape/` — shell metacharacter escaping

**Module system completeness.** Unit tests verify `isBuiltin()` recognizes both
`node:fs` and bare `fs`, `_nodeModulePaths()` prevents duplicate resolution,
`Module.prototype._compile` overriding (used by pirates/esbuild-register).

**Framework-motivated regression tests.** Many specs include GitHub issue
references pointing to real npm packages that broke:

| Package | Spec | What it tests |
|---------|------|---------------|
| hono.js | `wrapped_http_response/` | Custom `Response` wrapping `ServerResponse` |
| vitest | `child_process_fork_deno_args/` | Deno arg passthrough in `fork()` |
| fflate | `worker_threads/` (eval sloppy mode) | Sloppy-mode `eval` in workers |
| pirates | `require_compile_hook/` | `Module._compile` hooks |
| yaml-ast-parser | `cjs_detect_bound_reexport/` | Re-exported bound methods |

**Other categories:** Worker threads (3 specs), HTTP/network streams (12+
specs), crypto edge cases (3 specs), async/event ordering, timer/microtask
ordering, permission boundary tests.

#### Key insight

These supplementary tests are numerically smaller than the vendored fixture
surface but cover critical behavioral gaps: the module resolution bridge,
global injection fidelity, process lifecycle, and real-world framework
compatibility. They are not upstream Node tests — they are Deno's own proof
that the compatibility layer behaves correctly in areas that Node's own suite
does not comprehensively exercise from the perspective of an alternative
runtime.

---

## 3. Bun

### 3.1 Overview

Bun vendors upstream Node.js test files directly and runs them as exit-code
tests. Starting with Bun 1.2, they shifted from reactive bug-fixing to
systematically running the official Node test suite on every commit.

Reference: https://bun.sh/blog/bun-v1.2 — "We started running Node.js's own
test suite on every commit."

### 3.2 Repository layout

```
test/js/node/
├── test/                    ← vendored upstream Node.js tests (unmodified)
│   ├── parallel/            ← ~1,000+ files
│   ├── sequential/          ← ~43 files
│   ├── common/              ← vendored test harness
│   └── fixtures/            ← vendored test fixtures
├── harness.ts               ← adapter bridging Node assert → Bun's Jest API
├── {module}/                ← Bun-authored per-module tests (fs/, http/, etc.)
└── ...
```

Reference: https://github.com/oven-sh/bun/tree/main/test/js/node

### 3.3 Test execution model

Bun uses a two-tier testing approach:

| Tier | Runner | Success criteria | Modifiable? |
|------|--------|-----------------|-------------|
| Upstream Node tests | `bun bd <file>` (bare direct execution) | Exit code 0 | No — "These are Node.js compatibility tests not written by Bun, so we cannot modify these tests" |
| Bun-authored tests | `bun test` (Jest-compatible) | Test assertions | Yes |

Source: `test/js/node/test/parallel/CLAUDE.md` in the Bun repo (per research agent).

### 3.4 Manifest and status tracking

**Bun has no structured manifest.** Their approach is inclusion-based: they vendor
the subset of upstream tests that pass. Inclusion in the `test/` directory IS the
pass list.

Coverage tracking is via internal CI dashboards. Blog posts reference metrics:
- "15% of Node's test suite" (pre-1.2)
- "25% of Node's test suite on every commit" (1.2 target)
- "37%" → "75% target" (later milestone)

Reference: https://bun.sh/blog/bun-v1.2 and GitHub issue
https://github.com/oven-sh/bun/issues/159.

### 3.5 Sync model

Manual vendoring. No automated sync tooling found. Target Node version is not
explicitly declared.

### 3.6 Multi-version strategy

**Bun targets one Node.js version** (implied current/LTS). No multi-version
testing.

### 3.7 Supplementary tests beyond vendored Node fixtures

Bun maintains hundreds of self-authored Node-compat tests under
`test/js/node/` outside the vendored `test/js/node/test/*` tree. On the
fetched upstream `main` branch measured on 2026-05-09, there are 262
supplementary `*.test` / `*.spec` files outside the vendored tree, spread
across dozens of module areas. The vendored parallel suite alone is 2,208
files, plus 43 vendored sequential files.

Source: Bun repo `test/js/node/` directory listing and
https://github.com/oven-sh/bun/tree/main/test/js/node.

#### Test categories and gap coverage

**Module system completeness (`stubs.test.js`).** Iterates through all 75+
Node builtins and verifies each is importable via `import()`, `require()`, and
`import.meta.require()` in both bare and `node:`-prefixed forms. Also tests
internal specifiers like `_http_client` and `_stream_duplex`.

This is a universally portable test pattern — any alternative runtime can adopt
it to verify builtin import completeness.

**CJS/ESM interop edge cases.** `missing-module.test.js` tests error messages
for module resolution failures across `require`/`import`/`require.resolve` and
the differences between CJS and ESM error codes. `dirname.test.js` validates
`__dirname` and `__filename` availability and mapping.

**Resource leak detection (`fs-leak.test.js`).** Tracks max allocated file
descriptor, verifies it returns to baseline after stream close. Tests both
`createWriteStream`/`createReadStream` and `FileHandle.close()` idempotency.

**Buffer TOCTOU security (`buffer-copy-fill-detach.test.ts`).** Tests that
`Buffer.copy()` and `Buffer.fill()` protect against detaching/resizing
`ArrayBuffer` during numeric coercion side-effects. Returns 0 bytes copied
rather than crashing.

**Signal handler management (`process-signal-listener-count.test.ts`).** Tests
that removing one signal listener while others remain keeps the OS handler
installed, and removing ALL listeners uninstalls the handler.

**Custom extension loaders (`require-extensions.test.ts`).** Tests
`require.extensions` mutation and custom extension handler behavior — used by
tools like `ts-node` and `tsx`.

**Other categories:** Buffer edge cases (8 tests), filesystem operations (18
tests), HTTP protocol (22 tests), path module (19 tests), crypto (16 tests),
process (13 tests), readline (7 tests), async hooks (5 tests), child process
(8 tests), HTTP/2 (4 tests), net/sockets (5 tests), platform-specific behavior.

#### Key insight

Bun's supplementary tests focus on three areas that Node's own suite does not
comprehensively cover from the perspective of an alternative runtime:

1. **Completeness verification** — ensuring every builtin is importable in
   every supported form
2. **Resource safety** — leak detection and crash resistance for operations
   that are trivially safe in Node but require careful implementation in
   alternative runtimes
3. **Engine-specific concerns** — behaviors that differ between V8 and
   JavaScriptCore (Bun's engine), particularly around `ArrayBuffer` detach
   and typed array coercion

Reference: https://github.com/oven-sh/bun/tree/main/test/js/node;
https://github.com/oven-sh/bun/blob/main/test/js/node/test/parallel/CLAUDE.md.

---

## 4. Cloudflare workerd

### 4.1 Overview

workerd implements Node.js APIs natively in C++ and TypeScript within the Workers
runtime. Tests are **adapted/rewritten** from Node.js source, not vendored
unmodified. This is a fundamentally different approach from Nimbus, Deno, and Bun.

Reference: https://github.com/cloudflare/workerd/tree/main/src/workerd/api/node

### 4.2 Test format

Each test consists of two files:

1. `{name}-nodejs-test.js` — JavaScript test code using `node:assert`
2. `{name}-test.wd-test` — Cap'n Proto configuration declaring worker, modules,
   and compatibility flags

Tests are executed via Bazel `wd_test` rules.

### 4.3 Compatibility flag system

workerd uses date-based compatibility flags defined in
`src/workerd/io/compatibility-date.capnp`:

- `nodeJsCompat` → base Node.js API support
- `nodeJsCompatV2` → expanded support (implies v1 after `2024-09-23`)
- `enable_nodejs_fs_module` → filesystem access

Reference: https://github.com/cloudflare/workerd/blob/main/src/workerd/io/compatibility-date.capnp

### 4.4 Test count

~96 test files (adapted) + ~96 `.wd-test` config files. Significantly smaller
corpus than Deno or Bun.

### 4.5 Supplementary: Workers Node.js Compat Matrix

Cloudflare maintains a separate project tracking API compatibility across
runtimes: `cloudflare/workers-nodejs-compat-matrix`. It generates structured
data files (`data/node-{20,22,24}.json`, `data/deno.json`, `data/bun.json`,
`data/workerd.json`) by introspecting runtime APIs.

Reference: https://github.com/cloudflare/workers-nodejs-compat-matrix,
published at https://workers-nodejs-compat-matrix.pages.dev/

### 4.6 Multi-version strategy

Not applicable — workerd implements its own Node API surface, not a
Node-compatible runtime. Tests are written against the workerd implementation.

---

## 5. Node.js Core Harness and WPT

### 5.1 Overview

Node.js exposes two relevant testing architectures:

1. The main `test/` harness driven by `tools/test.py`, which is the closer
   analogue for Nimbus because it executes large compatibility suites with
   explicit execution classes, suite taxonomy, and status modeling.
2. The WPT runner under `test/wpt/`, which is the canonical example of
   cross-project fixture provenance tracking and structured expected-results
   files.

For Nimbus, the core harness is the primary comparison for enterprise-grade
Node compatibility execution. WPT is the secondary comparison for provenance
and status-file discipline.

### 5.2 Core harness repository layout

```
test/
├── parallel/                ← tests safe to run concurrently
├── sequential/              ← tests that must not run in parallel
├── known_issues/            ← tests expected to fail
├── internet/                ← real outbound network tests
├── pseudo-tty/              ← tests requiring TTY semantics
├── pummel/                  ← heavy-load stress tests
├── addons/                  ← addon / native-build-dependent tests
├── common/                  ← shared harness helpers
├── fixtures/                ← shared fixtures
├── root.status              ← global status annotations such as SLOW
└── ...
tools/
└── test.py                  ← main suite runner
```

Selected suite types from `test/README.md`:

| Suite | Purpose |
|-------|---------|
| `parallel/` | default concurrent execution |
| `sequential/` | serialized execution for stateful or timing-sensitive cases |
| `known_issues/` | expected-failure reproductions |
| `internet/` | capability-gated outbound networking |
| `pseudo-tty/` | capability-gated TTY semantics |
| `pummel/` | stress / hardship lanes |

This is exactly the kind of taxonomy Nimbus will need once it starts making
support claims across profiles, host capabilities, and multiple Node lines.

### 5.3 Core harness runner and status model

`tools/test.py` is not just a file enumerator. It models:

- separate **parallel** and **sequential** queues
- configurable task parallelism via `-j`
- explicit flaky-test handling modes
- suite-level default exclusions such as `addons`, `internet`, and `pummel`
- status metadata such as `SLOW`, `FLAKY`, and `SKIP`
- directory-level semantics like `known_issues/` for expected failures

Node's default harness therefore treats execution class and requirement gates as
first-class data, not as comments or ad hoc skip reasons inside test source.

### 5.4 Why the core harness is the closer analogue for Nimbus

Nimbus is not merely vendoring upstream fixtures. It is:

- running them inside an embedded runtime instead of the host `node` process
- classifying results by lane and profile
- working around capability restrictions such as sandboxed host fs and env access
- planning support claims that differ between `Application` and `Tooling`

That makes Node's core harness more relevant than WPT for the following
patterns:

- `parallel` vs `sequential` as an explicit execution axis
- requirement-gated suites like `internet` and `pseudo-tty`
- expected-failure isolation via `known_issues/`
- severity labels like `SLOW` and `FLAKY`
- runner-owned status and filtering behavior separated from test bodies

### 5.5 WPT runner overview

Node.js also imports Web Platform Tests from the upstream
`web-platform-tests/wpt` repository, pinned per-module to specific upstream
commits. This remains the canonical example of cross-project test sharing with
provenance tracking and structured status management.

Reference: https://github.com/nodejs/node/tree/main/test/wpt

### 5.6 Repository layout

```
test/
├── wpt/
│   ├── test-url.js             ← per-module runner scripts
│   ├── test-encoding.js
│   ├── status/                 ← per-module status files
│   │   ├── url.cjs
│   │   ├── encoding.json
│   │   ├── streams.json
│   │   └── ... (31 modules)
│   ├── README.md
│   └── testcfg.py
├── fixtures/wpt/
│   ├── versions.json           ← per-module upstream commit pins
│   └── {module}/               ← vendored WPT test files
└── common/
    └── wpt.js                  ← WPTRunner class
```

Source: `ls ~/src/github.com/nodejs/node/test/wpt/status/` and
`cat ~/src/github.com/nodejs/node/test/fixtures/wpt/versions.json`.

### 5.7 Version tracking (`versions.json`)

Each module is pinned to its own upstream WPT commit hash:

```json
{
  "url": { "commit": "abc123...", "path": "url" },
  "encoding": { "commit": "def456...", "path": "encoding" },
  "console": { "commit": "789012...", "path": "console" }
}
```

Modules are updated independently — a `url` sync does not affect `encoding`.

Source: `~/src/github.com/nodejs/node/test/fixtures/wpt/versions.json`.

### 5.8 Status file format

Per-module files under `test/wpt/status/` declare expected outcomes:

```javascript
// status/url.cjs
module.exports = {
  'historical.any.js': {
    fail: {
      expected: ['searchParams on location object'],
    },
  },
  'javascript-urls.window.js': {
    skip: 'requires document.body reference',
  },
  'toascii.window.js': {
    skipTests: [/\(using <a(rea)?>/],
  },
};
```

| Field | Purpose |
|-------|---------|
| `skip` | Skip entire file with reason string |
| `fail.expected` | Array of test names expected to fail |
| `fail.flaky` | Array of tests allowed to fail intermittently |
| `skipTests` | Array of exact names or regex patterns for subtest-level skipping |
| `requires` | Build requirements (`full-icu`, `crypto`, `inspector`) |

Source: `~/src/github.com/nodejs/node/test/wpt/status/url.cjs` and
`~/src/github.com/nodejs/node/test/wpt/status/console.json`.

### 5.9 Three-tier outcome model

The `WPTRunner` class evaluates results against a three-tier model:

1. **Expected pass** — test succeeds as configured
2. **Expected failure** — test fails in the documented way (characterized gap)
3. **Flaky** — test may fail intermittently (tolerated)
4. **Unexpected failure** — test fails but was expected to pass → CI failure
5. **Unexpected pass** — test passes but was expected to fail → signal to update status

This bidirectional model (unexpected failures AND unexpected passes) ensures the
status files stay synchronized with actual runtime behavior.

Source: `~/src/github.com/nodejs/node/test/common/wpt.js` (`WPTRunner` class).

### 5.10 Sync tooling

Node provides `git node wpt <module>` as a CLI command to pull fresh test files
from upstream WPT, update `versions.json` with the new commit hash, and stage
the changes. A GitHub Actions nightly workflow tests against the `epochs/daily`
WPT branch, and results are uploaded to wpt.fyi for cross-project comparison.

Reference: https://github.com/nodejs/node/blob/main/test/wpt/README.md

### 5.11 Multi-version strategy

Not applicable — Node.js is tracking upstream WPT, not cross-Node-version
compatibility.

---

## 6. Tooling and Management Workflows

This section documents how each runtime provides developer-facing tooling for
running, filtering, syncing, and reporting on compatibility tests. This is a
critical dimension for day-to-day usability and for building the kind of
operational discipline that enterprise users expect.

### 6.1 Nimbus (current state)

**Running tests:**

All node_compat tests are standard Rust `#[test]` functions, invoked through
`cargo test` with name filtering:

```bash
# Run all node_compat tests
cargo test -p nimbus-runtime -- node_compat --nocapture

# Run a single named test
cargo test -p nimbus-runtime -- node_compat::node22_process_env_delete --nocapture

# Run a batch
cargo test -p nimbus-runtime -- node22_primary_lane_executes_manifested_core_semantics_subset --nocapture

# Run all ignored tests (to check known-gap status)
cargo test -p nimbus-runtime -- node_compat --ignored --nocapture
```

The `Makefile` provides wrapped entrypoints with single-flight guards:

```bash
make test                              # Full workspace test suite
make verify-harness SURFACE=runtime    # Focused verification harness
make verify-harness-repro SURFACE=runtime MODE=pr CASE=<case-id>  # Reproduce single case
```

Source: `Makefile` lines 36–37 (`test` target) and 84–97 (harness targets);
`scripts/verification-harness.sh` for the harness pattern.

**Filtering limitations:**

- Individual fixtures inside a batch test are not addressable. When
  `node22_primary_lane_executes_manifested_core_semantics_subset` fails, you
  cannot run just one fixture within it — you must rerun the entire batch.
- There is no way to run "all Node 20 tests" or "all core semantics tests" without
  knowing the specific test function names.
- There is no way to list which tests exist, which are ignored, or what their
  status is without reading the Rust source.

**Fixture sync:** Manual. No tooling.

**Reporting:** Batch tests emit progress to stderr during execution:
```
node_compat core-semantics node22 -> test/parallel/test-assert-async.js
node_compat core-semantics node22 summary -> passed: 42, skipped: 3, failed: 0
```
No structured output. No persisted reports.

**Existing related tooling:**

- `scripts/runtime/node/generate_matrix.py` — generates the node-compat surface
  matrix documentation from Node.js API docs and Deno compat data. This is a
  documentation tool, not a test runner.
- `scripts/verification-harness.sh` — thin shell wrapper that maps named
  surfaces and modes to `cargo test` commands. This is the established pattern
  for the project's developer-facing test tooling.

Source: `scripts/runtime/node/generate_matrix.py` (60 lines inspected);
`scripts/verification-harness.sh` (full file).

### 6.2 Deno

**Running tests:**

Deno's node_compat tests are a standalone Cargo test binary with built-in
filtering:

```bash
# Run all configured tests
cargo test --test node_compat

# Run tests matching a filter (any test file in the suite, even unconfigured)
cargo test --test node_compat -- test-assert

# Run with report generation
deno task --cwd tests/node_compat/runner test --report
```

When a filter is provided, Deno runs any matching test from the full suite —
even tests not listed in `config.jsonc`. Without a filter, only configured tests
run. This dual-mode design lets developers explore unconfigured tests locally
while CI runs only the curated set.

Source: `~/src/github.com/nimbus/deno/tests/node_compat/mod.rs` lines
123–141; README at `~/src/github.com/nimbus/deno/tests/node_compat/README.md`.

**CI sharding:**

Tests are sharded across CI runners via environment variables:

```yaml
env:
  CI_SHARD_INDEX: '${{ matrix.shard_index }}'
  CI_SHARD_TOTAL: '${{ matrix.shard_total }}'
```

The workflow runs a 9-job matrix (3 OS × 3 shards):

```yaml
# .github/workflows/node_compat_test.generated.yml
on:
  schedule:
    - cron: 0 10 * * 1-5    # weekdays at 10 AM UTC
  workflow_dispatch: {}
jobs:
  test:
    strategy:
      matrix:
        include:
          - { os: linux, runner: ubuntu-latest, shard_index: '0', shard_total: '3' }
          - { os: linux, runner: ubuntu-latest, shard_index: '1', shard_total: '3' }
          # ... 9 total jobs
```

Source: `~/src/github.com/nimbus/deno/.github/workflows/node_compat_test.generated.yml`
lines 1–58.

**Fixture sync:**

The `vendor.ts` script (20 lines) performs a sparse checkout from upstream Node:

```typescript
import { version } from "./node_version.ts";
const tag = "v" + version;
await $`git clone --depth 1 --sparse --branch ${tag} --single-branch https://github.com/nodejs/node.git`;
await $`git sparse-checkout add test`.cwd("node");
await $`cp -r node/test ./test`;
```

To update: edit `node_version.ts` with the new version, run `vendor.ts`,
commit the result.

Source: `~/src/github.com/nimbus/deno/tests/node_compat/runner/suite/vendor.ts`.

**Report generation and publishing:**

The runner emits a `report.json` with per-test results:

```rust
struct TestReport {
    date: String,
    deno_version: String,
    os: String,
    arch: String,
    node_version: String,
    run_id: Option<String>,
    total: usize,
    pass: usize,
    ignore: usize,
    results: HashMap<String, TestResultEntry>,
}
```

CI uploads gzipped reports to S3:

```bash
aws s3 cp tests/node_compat/report.json.gz \
  s3://dl-deno-land/node-compat-test/$(date +%F)/report-${os}-${shard_index}.json.gz
```

A summary job aggregates daily reports into monthly summaries and posts to
Slack with pass ratio trends (current vs. previous day, with color-coded
deltas: green/red/yellow).

The public dashboard at `node-test-viewer.deno.dev/results/latest` renders
these reports for anyone to inspect.

Source: `~/src/github.com/nimbus/deno/tests/node_compat/report.rs` lines
1–80; CI workflow lines 87–102 (upload); `slack.ts` lines 1–80.

**Adding a new passing test:**

From Deno's README:

> If you fixed some Node.js compatibility and some test cases started passing,
> then add those cases to `config.jsonc`. The items listed in there are checked
> in CI check.

The workflow: fix the compat issue → run the test with a filter
(`cargo test --test node_compat -- test-foo`) → if it passes, add it to
`config.jsonc` → CI now guards it.

Source: `~/src/github.com/nimbus/deno/tests/node_compat/README.md`.

### 6.3 Bun

**Running tests:**

Bun uses two different execution modes:

```bash
# Upstream Node tests (exit code = pass/fail)
bun bd test/js/node/test/parallel/test-assert.js

# Bun-authored tests (Jest-compatible runner)
bun test test/js/node/fs/
```

The `bun bd` command ("bare direct") runs a file and checks exit code 0. This
is the only way upstream Node tests are executed — they are not wrapped in
Bun's test framework.

**CI sharding:**

Bun supports Jest/Vitest-compatible sharding:

```bash
bun test --shard=1/3
```

CI runs on Buildkite (`.buildkite/ci.mjs`). A separate ecosystem CI repo
(`github.com/oven-sh/bun-ecosystem-ci`) tests third-party packages.

Reference: https://github.com/oven-sh/bun/blob/main/test/README.md;
https://github.com/oven-sh/bun-ecosystem-ci.

**Fixture sync:** Manual vendoring. No script. No version tracking.

**Reporting:** Internal CI dashboard. No structured public reports.

**Adding a new passing test:**

Per the CLAUDE.md in `test/js/node/test/parallel/`:

> These are Node.js compatibility tests not written by Bun, so we cannot
> modify these tests.

New tests are vendored by copying from an upstream Node checkout. If a test
needs Bun-specific assertions, a separate `*.test.ts` file is created in the
module directory using the `node-harness` adapter.

Reference: https://github.com/oven-sh/bun/blob/main/test/js/node/test/parallel/CLAUDE.md.

### 6.4 Node.js WPT

**Running tests:**

```bash
# Run all URL WPT tests
python tools/test.py test/wpt/test-url.js

# Or directly
node test/wpt/test-url.js
```

Each module has its own runner script that instantiates `WPTRunner`:

```javascript
const { WPTRunner } = require('../common/wpt');
const runner = new WPTRunner('url');
runner.pretendGlobalThisAs('Window');
runner.runJsTests();
```

Source: `~/src/github.com/nodejs/node/test/wpt/README.md`.

**Fixture sync:**

```bash
# Pull latest WPT tests for a specific module
cd /path/to/node/project
git node wpt url
```

This CLI command:
1. Clones/updates the upstream WPT repo
2. Copies the module's test directory into `test/fixtures/wpt/`
3. Updates `versions.json` with the new commit hash
4. Stages the changes for commit

Modules are updated independently — syncing `url` does not affect `encoding`.

A GitHub Actions nightly workflow tests against the `epochs/daily` WPT branch,
and results are uploaded to wpt.fyi.

Source: `~/src/github.com/nodejs/node/test/wpt/README.md`;
`~/src/github.com/nodejs/node/test/fixtures/wpt/versions.json`.

**Adding a new module:**

From the README, adding WPT coverage for a new module is a documented
three-step process:
1. `git node wpt <module>` to download fixtures
2. Create `test/wpt/test-<module>.js` with a `WPTRunner` instance
3. Create `test/wpt/status/<module>.json` with expected failures

### 6.5 Cross-runtime tooling comparison

| Capability | Nimbus | Deno | Bun | Node WPT |
|------------|--------|------|-----|----------|
| **Run single test** | `cargo test -- <name>` | `cargo test --test node_compat -- <filter>` | `bun bd <file>` | `node test/wpt/test-url.js` |
| **Run single fixture in batch** | Not possible | N/A (no batches) | N/A | N/A (per-module runners) |
| **Run by version lane** | Know the function name | N/A (single version) | N/A | N/A |
| **Run by compatibility family** | Know the batch function name | N/A | N/A | Run module runner script |
| **List all tests** | Read Rust source | Read `config.jsonc` | List files in `test/` | List `test/wpt/test-*.js` |
| **List ignored tests** | `grep '#\[ignore' node_compat.rs` | `grep '"ignore"' config.jsonc` | N/A | Read status files |
| **Sync from upstream** | Manual copy | `vendor.ts` (20 lines) | Manual copy | `git node wpt <module>` |
| **Track source version** | Not tracked | `node_version.ts` | Not tracked | `versions.json` per module |
| **CI sharding** | Not implemented | `CI_SHARD_INDEX/TOTAL` env vars | `--shard` flag | N/A |
| **Structured report** | None | JSON → S3 → dashboard | Internal only | JSON → wpt.fyi |
| **Public dashboard** | None | `node-test-viewer.deno.dev` | None | wpt.fyi |
| **Slack/notification** | None | `slack.ts` with trend deltas | None | N/A |
| **Add passing test** | Add batch entry macro + `#[test]` fn | Add line to `config.jsonc` | Copy file to `test/` | `git node wpt` + status file |
| **Mark test ignored** | `#[ignore = "reason"]` in Rust | `"ignore": true, "reason": "..."` in JSONC | Remove file from `test/` | `"skip": "reason"` in status file |

The Node core harness adds another important pattern on top of the table above:
suite taxonomy and requirement gating are first-class concepts (`parallel`,
`sequential`, `known_issues`, `internet`, `pseudo-tty`, `SLOW`, `FLAKY`)
rather than free-form comments. That pattern is highly relevant to Nimbus's
embedded-runtime and multi-profile support story.

### 6.6 Ideas for Nimbus tooling

The following are ideas for discussion, not recommendations. They are inspired
by patterns observed in the surveyed runtimes and adapted to Nimbus's unique
multi-version, embedded-runtime architecture.

#### 6.6.1 CLI for test management

A `make node-compat` or `scripts/node-compat.sh` entrypoint following the
established verification-harness pattern:

```bash
# Run all node_compat tests for a lane
make node-compat LANE=node22

# Run a specific compatibility family
make node-compat LANE=node22 SLICE=core-semantics

# Run a single fixture by path
make node-compat FIXTURE=test/parallel/test-assert-async.js

# Run all lanes for one fixture (cross-version comparison)
make node-compat FIXTURE=test/parallel/test-assert-async.js LANE=all

# List all tests with status for a lane
make node-compat-status LANE=node22

# List only ignored tests with reasons
make node-compat-status LANE=node22 STATUS=ignored
```

This would give developers a vocabulary for common operations without
requiring knowledge of Rust test function names.

#### 6.6.2 Sync script

A script wrapping the local Node checkout at
`~/src/github.com/nodejs/node`:

```bash
# Sync node22 fixtures from a specific tag
scripts/node-compat-sync.sh node22 v22.x.y

# Diff what changed since last sync (dry run)
scripts/node-compat-sync.sh node22 v22.x.y --diff-only

# Sync and update provenance in the manifest
scripts/node-compat-sync.sh node22 v22.x.y --update-manifest
```

Internally: `cd ~/src/github.com/nodejs/node && git checkout v22.x.y`, then
rsync/diff against `node_compat_fixtures/node22/`, update
`node_compat_sources.json`.

The `--diff-only` mode would report new/changed/removed files without
modifying anything — useful for evaluating whether a sync is needed.

#### 6.6.3 Status query tool

If test configuration moves to a manifest (JSONC, TOML, or similar), a query
tool could answer common questions:

```bash
# How many tests per lane?
scripts/node-compat-query.sh count-by-lane
#   node20: 850 pass, 40 ignore, 8 expected-failure
#   node22: 920 pass, 25 ignore, 5 expected-failure
#   node24: 780 pass, 90 ignore, 30 preview-skip

# Which tests are ignored for node22?
scripts/node-compat-query.sh ignored --lane=node22

# Which tests differ between node20 and node22?
scripts/node-compat-query.sh divergent --lanes=node20,node22

# Which tests have no node24 entry yet?
scripts/node-compat-query.sh missing --lane=node24

# Which tests are application-profile compatible but blocked on main-thread or TTY requirements?
scripts/node-compat-query.sh gated --profile=application --requires=main-thread,tty

# Coverage by compatibility family
scripts/node-compat-query.sh coverage-by-slice --lane=node22
#   core-semantics: 195/200 (97.5%)
#   process-and-timing: 78/82 (95.1%)
#   ...
```

This would replace ad-hoc `grep` commands against Rust source with
structured, scriptable queries.

#### 6.6.4 Structured JSON report

Following Deno's pattern, emit a JSON report after each test run:

```json
{
  "date": "2026-05-08",
  "runtimeVersion": "0.1.0",
  "sources": {
    "node20": { "tag": "v20.x.y", "synced": "2026-05-08", "upstreamStatus": "eol", "laneRole": "validation" },
    "node22": { "tag": "v22.x.y", "synced": "2026-05-08", "upstreamStatus": "lts", "laneRole": "primary" },
    "node24": { "tag": "v24.x.y", "synced": "2026-05-08", "upstreamStatus": "lts", "laneRole": "preview" }
  },
  "lanes": {
    "node20": { "passed": 420, "skipped": 12, "expectedFailure": 8, "failed": 0 },
    "node22": { "passed": 485, "skipped": 10, "expectedFailure": 5, "failed": 0 },
    "node24": { "passed": 380, "skipped": 45, "expectedFailure": 15, "failed": 0 }
  },
  "slices": {
    "core-semantics": {
      "node20": { "passed": 180, "skipped": 5 },
      "node22": { "passed": 195, "skipped": 3 },
      "node24": { "passed": 160, "skipped": 20 }
    }
  }
}
```

This enables time-series tracking, PR-level regression detection, and
evidence-backed compatibility claims while also making the upstream line
status and Nimbus lane role explicit.

Deno uploads to S3 and renders via `node-test-viewer.deno.dev`. Nimbus could
follow a similar pattern or integrate with existing CI artifact storage.

#### 6.6.5 CI integration patterns

**Sharding:** For large test suites, Deno's `CI_SHARD_INDEX`/`CI_SHARD_TOTAL`
pattern splits tests across parallel CI runners. This is straightforward to
implement: the runner reads the env vars, partitions the test list, and only
runs its shard.

**Scheduled vs. per-commit:** Deno runs node_compat daily (not per-commit)
because the full suite is slow. A similar split could work for Nimbus:
- **Per-commit:** curated fast subset (e.g., the batch tests)
- **Nightly/scheduled:** full suite including ignored tests in exploratory mode
  to detect unexpected passes

This mirrors Nimbus's existing `pr` vs. `nightly` verification-harness split.

**Report archival:** Deno's CI workflow uploads gzipped JSON to S3, then a
summary job aggregates daily reports into monthly summaries. This creates an
auditable time-series of compatibility progress.

#### 6.6.6 Public compatibility dashboard

Deno's `node-test-viewer.deno.dev` and Cloudflare's
`workers-nodejs-compat-matrix.pages.dev` are the two public dashboards in
this space.

A Nimbus dashboard could be unique by showing **multi-version data**: three
lanes with per-compatibility-family breakdowns, trend lines, and provenance links to the
upstream Node tag each fixture set came from. This would be a first in the
space — no other runtime publishes cross-version compat data.

#### 6.6.7 "Unexpected pass" detection

Node's WPT runner detects when an expected-failure test starts passing and
flags it. This is valuable signal: it means a runtime improvement has fixed a
previously-known gap, and the manifest should be updated.

Without this, ignored tests stay ignored forever. With it, every CI run
answers "did we fix anything new?" automatically.

#### 6.6.8 Fixture freshness checking

A CI job or pre-sync check that compares the `synced` date in the provenance
manifest against the latest upstream release:

```bash
# Check if fixtures are current
scripts/node-compat-freshness.sh
#   node20: v20.x.y (synced 2026-05-08) — upstream line is EOL
#   node22: v22.x.y (synced 2026-05-08) — newer upstream patch releases available
#   node24: v24.x.y (synced 2026-05-08) — newer upstream patch releases available
```

This makes staleness visible without requiring manual tracking.

#### 6.6.9 Version-aware runtime behavior testing

If Nimbus evolves to support user-selectable Node version targets (e.g., a
user declares their project targets Node 20), the test infrastructure must
validate that the runtime produces version-correct behavior. Ideas for
how this could work:

**Approach A: Version-parameterized runtime.** The
`RuntimeCompatibilityTarget` enum gains `Node20` and `Node24` variants
alongside the existing `Node22`. The bootstrap sets `process.version`
accordingly. The module loader, stream defaults, error messages, and API
surface adjust based on the target. Tests for each lane run against the
matching target:

```rust
// Node20 tests run against Node20-shaped runtime
RuntimeLimits { compatibility_target: Node20, .. }
// Node22 tests run against Node22-shaped runtime
RuntimeLimits { compatibility_target: Node22, .. }
```

This is the highest-fidelity approach but requires the most runtime work.

**Approach B: Behavioral polyfills in the manifest.** The test manifest
declares expected version-specific differences:

```jsonc
"test/parallel/test-assert-checktag.js": {
  "node20": {
    "fixture": "node20",
    "expectedBehavior": {
      "process.version": "v20.20.2",
      "assertDiffTrailingNewline": false
    }
  },
  "node22": {
    "fixture": "node22",
    "expectedBehavior": {
      "process.version": "v22.15.0",
      "assertDiffTrailingNewline": true
    }
  }
}
```

This documents behavioral divergence without requiring a full version-
parameterized runtime. The test runner could inject version-specific preludes
that patch `process.version` and adjust known-different defaults.

**Approach C: Behavioral test fixtures.** Instead of running the same upstream
test against all versions, maintain version-specific test fixtures that assert
version-correct behavior. This is what the `split_batch_case!` macro already
does — different fixture source paths for Node 20 vs. Node 22. Extending this
to explicitly test behavioral differences (not just "does it pass?") would
strengthen the version-correctness signal.

Each approach has different cost/benefit tradeoffs. Approach A gives the
strongest guarantees but is the most work. Approach C is closest to what
exists today. Approach B is a middle ground that improves documentation
without requiring runtime changes.

#### 6.6.10 Bare specifier verification matrix

A dedicated test matrix that verifies both bare and `node:`-prefixed
resolution for every supported builtin module:

```javascript
// For each builtin: verify both CJS and ESM, bare and prefixed
const builtins = ['assert', 'buffer', 'crypto', 'events', 'fs', 'http', ...];
for (const mod of builtins) {
  // CJS paths
  require(mod);           // bare
  require(`node:${mod}`); // prefixed

  // ESM paths (in separate test files)
  import mod from mod;            // bare ESM
  import mod from `node:${mod}`;  // prefixed ESM
}
```

This could be a standalone test file (not part of the upstream Node fixtures)
that runs as part of each lane, verifying that both specifier forms resolve
to the same module and produce equivalent behavior. It would catch regressions
where a bare import silently fails or resolves to a different module than the
prefixed form.

Deno explicitly passes `--unstable-bare-node-builtins` to all node_compat
tests to enable this. Nimbus's CJS path already normalizes bare specifiers
via `normalizeBuiltinSpecifier()`, but the ESM path in
`supports_extension_backed_node_builtin()` only matches `node:`-prefixed
specifiers (source: `module_loader.rs` lines 4012–4053). This gap should be
verified by tests.

---

## 7. Comparative Analysis

### 7.1 Summary table

| Dimension | Deno | Bun | workerd | Node WPT | **Nimbus** |
|-----------|------|-----|---------|----------|------------|
| **Fixture source** | Git submodule (`denoland/node_test`) | Vendored (unmodified) | Adapted/rewritten | Vendored, per-module pinned | Vendored per version |
| **Target versions** | One (25.8.1) | One (implied) | Own impl | N/A (WPT) | **Three (20, 22, 24)** |
| **Manifest format** | JSONC + JSON Schema | None (inclusion = pass) | `.wd-test` per test | `status/*.cjs` + `versions.json` | Rust macros + `#[ignore]` |
| **Manifest entries** | 2,908 | ~1,000 | ~96 | ~31 modules | 1,194 macro invocations |
| **Provenance tracking** | `node_version.ts` | None | N/A | `versions.json` per module | **None** |
| **Sync tooling** | `vendor.ts` (20 lines) | Manual | Manual | `git node wpt` CLI | **None** |
| **Status model** | pass / ignore / expected-failure / flaky / per-platform | binary (included or not) | binary | pass / skip / expected / flaky / unexpected | pass / `#[ignore]` |
| **Runner** | Rust (902 lines) | `bun bd`, exit code | Bazel `wd_test` | JS `WPTRunner` in Workers | Rust (embedded V8, ~556 lines infra) |
| **Reporting** | JSON → S3 → public dashboard + Slack | Internal CI dashboard | Bazel results | wpt.fyi | Stderr summary only |
| **CI cadence** | Daily scheduled | Every commit | Every commit | Nightly (WPT daily) | On demand |
| **Test isolation** | Per-process spawning | Per-process spawning | Bazel sandbox | Worker threads | Global mutex + embedded V8 |
| **Supplementary tests** | Dozens of `unit_node` tests plus hundreds of `specs/node` files | Hundreds of self-authored `*.test` / `*.spec` files outside vendored `test/js/node/test/*` | N/A (tests are adapted, not vendored) | N/A | **None** |

### 7.2 Key architectural differences

**Multi-version testing:** Nimbus is the only surveyed runtime that tests against
multiple Node.js versions simultaneously. Deno, Bun, and workerd all target a
single version. This is a genuine competitive advantage but creates structural
challenges that no other runtime has solved.

**Embedded vs. spawned execution:** Deno, Bun, and workerd all spawn tests as
separate processes. Nimbus runs tests inside an embedded V8 runtime via
`invoke_bundle()`. This means Nimbus cannot use exit codes as a signal — it must
parse JSON return values. It also means Nimbus needs the prelude/postlude
injection system to simulate process-level behavior (env vars, process.exit,
etc.) that process-spawning runtimes get for free.

**Node core harness as analogue:** Node's main `test/` harness contributes the
canonical patterns for suite taxonomy, capability gating, and parallelism
control. WPT contributes the canonical patterns for vendored-fixture provenance
and expected-results files. Nimbus needs both.

**Manifest-driven vs. code-driven declaration:** Deno and Node WPT use
structured data files (JSONC, JSON, CJS) for test configuration. Bun uses
inclusion. Nimbus encodes everything in Rust source (macros, batch constants,
`#[ignore]` attributes). The data-driven approach enables tooling, dashboards,
and programmatic analysis. The code-driven approach provides compile-time
guarantees but locks information inside Rust source.

**Supplementary behavioral tests as a distinct tier:** Both Deno and Bun
maintain substantial bodies of self-authored tests that cover behaviors Node's
own test suite does not comprehensively exercise from the perspective of an
alternative runtime. These fall into distinct categories that are relevant to
any embedded or compatibility-focused runtime:

| Category | What it tests | Why alternative runtimes still need their own layer |
|----------|--------------|----------------------------------------------------|
| Module resolution bridge | CJS/ESM interop, bare vs. `node:` specifiers, conditional `exports` | Node tests pieces of this surface, but not exhaustive cross-runtime bridge matrices |
| Global injection fidelity | `__dirname`, `__filename`, `require` as globals in CJS | Node has targeted checks, but alternative runtimes must prove their own injection paths |
| Builtin completeness | Every builtin importable in every form | Node does not need a separate completeness matrix for reimplemented builtin loading |
| Process lifecycle | `process.version`, `process.features`, signal handlers, event ordering | Alternative runtimes emulate Node process behavior on top of different host process models |
| Resource safety | FD leak detection, `ArrayBuffer` detach protection, crash resistance | Safety properties depend on the alternative runtime's own host integration and engine behavior |
| Framework regression | Specific patterns from express, hono, vitest, prisma, etc. | Framework authors validate against Node directly, not against compatibility layers |

Nimbus currently has no supplementary test tier — only vendored upstream
fixtures. This is a significant gap: passing Node's own tests proves API
correctness but does not prove bridge correctness, global injection fidelity,
or real-world ecosystem compatibility.

---

## 8. Gap Analysis: Nimbus vs. Industry Patterns

### 8.1 Manifest and status tracking

**Industry pattern:** Deno's `config.jsonc` with JSON Schema validation. Each
test entry is a structured object with machine-readable status, reason, and
per-platform behavior.

**Nimbus gap:** Test status is encoded in three places:
1. Rust macro invocations choose per-lane fixture routing (~1,194 entries)
2. `#[ignore = "reason"]` annotations document known gaps (~60)
3. Batch constant membership determines which tests run at all (~43 arrays)

None of these are machine-readable outside of Rust compilation. Answering
"how many Node 24 tests pass?" requires parsing Rust source.

**Impact:** Cannot generate coverage dashboards, cannot diff status between
versions, cannot automate "what changed since last sync?" analysis.

### 8.2 Fixture provenance

**Industry pattern:** Deno tracks the source Node version in `node_version.ts`.
Node's WPT runner tracks per-module upstream commits in `versions.json`.

**Nimbus gap:** No record of which `nodejs/node` tag each `nodeXX/` directory
was pulled from. No sync dates. No way to know if fixtures are current without
manually diffing against a guessed tag.

**Impact:** Cannot verify claims like "we test against a specific pinned Node 22
tag." Cannot detect when upstream test changes invalidate local fixtures.
Audit trail is absent.

### 8.3 Fixture deduplication

**Industry pattern:** Deno and Bun store one copy of each fixture. Node WPT
stores one copy per vendored module.

**Nimbus gap:** 1,023 files are byte-identical between node20/ and node22/. 739
files are byte-identical across all three versions. This is ~6–8 MB of
unnecessary duplication across 20 MB total.

**Impact:** Harder to see which tests actually differ between versions (the
interesting signal). Larger repo. More disk I/O during test runs for identical
content. Sync operations touch more files than necessary.

### 8.4 Sync tooling

**Industry pattern:** Deno's `vendor.ts` (20 lines) automates sparse checkout +
copy from upstream. Node's `git node wpt` CLI automates per-module pulls with
version tracking.

**Nimbus gap:** No sync script. Fixture updates are manual copy operations with
no automated diffing or provenance recording.

**Impact:** Sync is error-prone and time-consuming. No audit trail for when
fixtures were updated. Risk of partial syncs or missed files.

### 8.5 Expected failure model

**Industry pattern:** Deno supports five states (pass, ignore, expected-failure,
flaky, per-platform). Node WPT supports a three-tier outcome model (expected,
flaky, unexpected) with bidirectional detection (unexpected passes are flagged
alongside unexpected failures).

**Nimbus gap:** Two states only: pass or `#[ignore]`. No expected-failure
tracking (a test that fails in a known way is simply ignored). No flaky retry.
No detection of unexpected passes (a previously-ignored test that now passes
stays ignored indefinitely).

**Impact:** Cannot distinguish "we know this fails and have characterized why"
from "we haven't looked at this." No signal when runtime improvements fix
previously-broken tests. No protection against flaky test noise.

### 8.6 File size and modularity

**Industry pattern:** Deno's runner is 902 lines of Rust. Test configuration
lives in a separate 3,783-line JSONC file. Clean separation of concerns.

**Nimbus gap:** `node_compat.rs` is 6,904 lines combining infrastructure,
data tables, and test declarations. This is 3.4× the repo's own 2,000-line
modularity threshold. The Rust runner infrastructure is only a fraction of the
file; the overwhelming majority is batch data and `#[test]` declarations.

**Impact:** Hard to navigate. Hard to review changes. A new batch entry
requires editing a 6,904-line file. New contributors face a high barrier to
understanding the test structure.

### 8.7 Structured reporting

**Industry pattern:** Deno produces JSON reports uploaded to S3 with a public
dashboard at `node-test-viewer.deno.dev`. Cloudflare publishes a compat matrix
at `workers-nodejs-compat-matrix.pages.dev`.

**Nimbus gap:** Test results are emitted to stderr during batch runs. No
structured output, no time-series tracking, no dashboard.

**Impact:** Cannot produce evidence-backed compatibility claims. Cannot track
improvement trends over time. No public trust signal.

### 8.8 Macro scalability

**Industry pattern:** Deno's manifest entries are plain JSON objects — adding a
field is a schema change, not a code change. No macros.

**Nimbus gap:** 9 Rust macros handle different combinations of per-lane fixture
routing and extra files. When Node 26 arrives, each macro needs a
`node26_fixture_source_path` field, the `NodeCompatBatchEntry` struct grows
another pair of fields, and new macro variants may be needed.

**Impact:** Linear growth in macro complexity with each new Node version.
Cognitive overhead for developers choosing the right macro.

### 8.9 Version-specific behavioral divergence

**Industry pattern:** Node.js itself changes behavior between major versions:
error message wording, default values, API surface additions, and deprecation
semantics all shift. Runtimes that claim multi-version support must handle
these divergences. Currently, Deno and Bun sidestep the problem by targeting
a single Node version. Node's WPT runner sidesteps it by testing its own
behavior (not cross-version).

**Nimbus gap:** The runtime hardcodes `process.version` to `"v22.0.0-nimbus"`
(source: `bootstrap/source.rs` line 761) and `RuntimeCompatibilityTarget` has
only two variants: `WebStandardIsolate` and `Node22` (source: `limits.rs`
lines 19–22). All three test lanes (Node 20, 22, 24) run against this single
Node 22-shaped runtime.

This means version-divergent behaviors are currently handled by ignoring the
test. The `#[ignore]` annotations document this explicitly:

- "Pinned Node20 divergence: official v20.20.2 still accepts `once(emitter,
  event, null)`, while the current runtime matches the newer Node22
  invalid-options behavior and rejects null"
- "Pinned Node20 divergence: official v20.20.2 `process.features` does not
  expose the Node22-only `typescript` key"
- "Pinned Node20 divergence: official v20.20.2 `PerformanceResourceTiming
  #toJSON()` omits the Node22-era `deliveryType` and `responseStatus` fields"
- "Pinned Node20 divergence: official v20.20.2 `test-stream-transform-split
  -highwatermark.js` still expects the older 16 KiB split Transform default
  highWaterMark"

Source: `grep '#\[ignore.*divergence' node_compat.rs` — at least 10 such
annotations.

There are concrete, observable differences between Node versions that a
multi-version runtime must handle:

| Dimension | Example: Node 20 vs. Node 22 |
|-----------|------------------------------|
| Error message wording | `assert.deepStrictEqual` diff output includes trailing `\n` in Node 22 |
| API surface | `process.features.typescript` exists in Node 22, not Node 20 |
| Default values | Stream `highWaterMark` defaults differ (16 KiB → 64 KiB) |
| Deprecation behavior | `once(emitter, event, null)` accepted in Node 20, rejected in Node 22 |
| Global references | `global` → `globalThis` in test harness code |
| PerformanceResourceTiming | Additional fields (`deliveryType`, `responseStatus`) in Node 22 |

**Impact:** If Nimbus claims to support "Node 20 through 24," users deploying
with a Node 20 target expect Node 20 behavior — including Node 20 error
messages, Node 20 defaults, and Node 20 API surface. A single Node 22-shaped
runtime that claims Node 20 support but produces Node 22 error messages will
break applications that depend on those messages (e.g., error parsing, test
assertions in user code).

The test infrastructure must be able to validate that version-specific
behavior is correct for the declared target version, not just that the test
"passes against some runtime shape."

### 8.10 Bare module specifier support (`require('fs')` vs. `require('node:fs')`)

**Industry pattern:**

| Runtime | `require('fs')` | `require('node:fs')` | `import 'fs'` | `import 'node:fs'` |
|---------|-----------------|---------------------|---------------|-------------------|
| Node.js | Always supported | Supported since Node 12 | Requires `--experimental-specifier-resolution` (older) or supported natively (newer) | Always supported in ESM |
| Deno | Requires `--unstable-bare-node-builtins` flag | Always supported | Requires flag | Always supported |
| Bun | Always supported | Always supported | Always supported | Always supported |

Source: Deno's `mod.rs` lines 37 and 45 pass `--unstable-bare-node-builtins`
to all node_compat test runs.

**Nimbus current state:**

The CJS `createRequire` path in `module_loader.rs` uses
`normalizeBuiltinSpecifier()` which strips the `node:` prefix for resolution:

```javascript
function normalizeBuiltinSpecifier(specifier) {
  return specifier.startsWith("node:")
    ? specifier.slice(5)
    : specifier;
}
```

The `builtinModules` array is populated with both bare names and `node:`-
prefixed names. The `isBuiltin()` function checks both. The ESM path in
`supports_extension_backed_node_builtin()` only matches `node:`-prefixed
specifiers (source: `module_loader.rs` lines 4012–4053).

Source: `module_loader.rs` lines 3583–3622 (CJS normalization and builtins);
lines 4012–4053 (ESM builtin matching).

**Real-world prevalence of bare specifiers:**

A significant portion of the npm ecosystem still uses bare specifiers:

- `require('fs')`, `require('path')`, `require('crypto')` — standard in
  packages targeting Node 12+ (before `node:` was common)
- Major packages like `express`, `axios`, `lodash`, `moment`, and most of
  their dependencies use bare specifiers
- The `node:` prefix became conventional around Node 16+ but adoption is
  gradual
- Bundlers (webpack, esbuild, Rollup) resolve bare specifiers without issue
- Many enterprise codebases have legacy code using bare specifiers

**Gap:** The test suite does not systematically verify bare specifier
resolution for all supported builtins. The ESM path only matches
`node:`-prefixed specifiers. If a user's code does
`import fs from 'fs'` (a bare ESM import of a builtin), the behavior depends
on whether the Deno-family resolver handles this or whether Nimbus's
module loader intercepts it.

**Impact:** An enterprise user migrating existing Node.js code to Nimbus will
likely encounter bare-specifier imports. If these fail silently or produce
confusing errors, trust is immediately eroded. The test infrastructure should
verify both `require('fs')` and `require('node:fs')` (and the ESM
equivalents) for every supported builtin.

### 8.11 Execution class, profile, and capability modeling

**Industry pattern:** Node's core harness makes execution class and requirement
gating explicit through suite taxonomy (`parallel`, `sequential`,
`known_issues`, `internet`, `pseudo-tty`, `pummel`) plus runner-owned status
metadata. Deno's manifest adds platform-aware expectations.

**Nimbus gap:** The current system and the proposed manifest treat lane and
fixture routing as the primary axes, but they do not yet model the dimensions
Nimbus actually claims against:

- `Application` vs `Tooling`
- main-thread-only vs worker-safe execution
- host capability requirements such as TTY, loopback networking, crypto, or
  bundle-root-only fs access
- execution class such as parallel, sequential, watchpoint, or expected-failure
- host-platform restrictions

Without these as machine-readable fields, the system will keep collapsing
intentional profile restrictions together with genuine runtime gaps.

**Impact:** Dashboards, CI summaries, and public support claims will be unable
to answer questions like "does this pass in Tooling only?" or "is this a real
Node gap, or an Application-profile capability restriction?" That ambiguity is
exactly the kind of fuzziness enterprise buyers do not trust.

### 8.12 Package and framework canaries

**Industry pattern:** Mature runtimes supplement upstream API suites with
ecosystem validation, whether through internal dashboards, ecosystem CI, or
checked-in smoke tests. API parity alone does not prove npm compatibility.

**Nimbus gap:** The survey mentions package usage patterns and Bun's ecosystem
CI, but the recommended improvement plan stops at upstream Node fixtures,
manifests, sync tooling, and reports. It does not require a stable,
version-pinned package/framework canary set mapped to claimed support.

**Impact:** Nimbus could eventually claim strong Node compatibility while still
failing on real packages and frameworks that matter to enterprise adoption:
HTTP servers, clients, loader tools, WebSocket stacks, test runners, and
framework scaffolding. That would undermine trust at exactly the point where
buyers try the first migration.

### 8.13 Harness self-verification and oracle comparison

**Industry pattern:** Spawned-process runtimes get a large amount of harness
truth "for free" from the host process model: exit codes, stdio behavior,
signal handling, and process lifecycle are all delegated to the OS and the
official runtime binary. Node's own harnesses also keep their runner semantics
explicit and testable.

**Nimbus gap:** Nimbus's compatibility runner is itself a substantial piece of
infrastructure:

- prelude/postlude injection
- skip detection
- temporary bundle materialization
- embedded JSON result envelopes
- scoped host env mutation
- top-level import-error capture for preview lanes

The research doc identifies this embedded architecture, but the recommended
plan does not add a dedicated harness-verification layer or a periodic oracle
comparison against real Node 20 / 22 / 24.

**Impact:** Nimbus could end up with excellent dashboards and polished support
matrices built on top of a drifting harness. That is a serious trust risk,
because it creates false precision rather than reliable evidence.

### 8.14 Supplementary behavioral test tier

**Industry pattern:** Both Deno and Bun maintain substantial self-authored test
surfaces between vendored upstream fixtures and ecosystem canaries. Exact
counts vary by branch, which is one reason to avoid a single synthetic total.
The stable pattern is what matters: Deno carries dozens of `unit_node` tests
plus hundreds of `specs/node` fixtures, and Bun carries hundreds of
self-authored `*.test` / `*.spec` files outside its vendored Node tree. These
tests form a distinct tier between "upstream vendored fixtures" and
"framework canaries" — they prove that the runtime's reimplementation of Node
internals behaves correctly for behaviors that Node's own suite does not
comprehensively exercise from the perspective of an alternative runtime.

Six categories emerge consistently across both projects:

| Category | Deno examples | Bun examples |
|----------|--------------|-------------|
| Module resolution bridge | `require_esm_module_exports/`, `esm_dir_import/`, `cjs_dynamic_import_esm_with_exports/` | `missing-module.test.js`, `require-extensions.test.ts` |
| Global/process injection | `process_test.ts`, `process_stdout_indestructible/` | `dirname.test.js`, `process-signal-listener-count.test.ts` |
| Builtin completeness | `module_test.ts` (`isBuiltin()`, `builtinModules`) | `stubs.test.js` (all 75+ builtins × 3 import forms) |
| Resource safety | Stream backpressure specs, handle cleanup | `fs-leak.test.js`, `buffer-copy-fill-detach.test.ts` |
| Framework regression | hono, vitest, fflate, pirates, yaml-ast-parser | express, various HTTP edge cases |
| Platform/engine-specific | Permission boundary tests | JSC-vs-V8 ArrayBuffer detach, Windows env case sensitivity |

**Nimbus gap:** Nimbus currently has zero supplementary behavioral tests. The
entire Node compatibility test surface consists of vendored upstream fixtures
executed through the embedded runtime. There is no verification of:

- CJS/ESM bridge correctness from the outside
- Builtin import completeness across all specifier forms
- Global injection fidelity (`__dirname`, `__filename`, `require` as globals)
- Process object shape (`process.version`, `process.features`,
  `process.versions`)
- Resource safety under the embedded V8 execution model
- Real-world framework patterns that depend on specific Node behaviors

**Impact:** Passing Node's own tests proves that individual API functions
produce correct outputs. It does not by itself prove that the module loader
resolves correctly, that globals are injected properly, that the process
object has the right shape, or that real npm packages work. These are the gaps
that cause enterprise users to hit immediate failures when deploying real code
— and they are gaps that Node's own suite does not comprehensively cover from
the perspective of an embedded alternative runtime.

The supplementary test tier is where Deno and Bun have both invested
significant engineering effort, and it is where Nimbus has zero coverage today.

---

## 9. Recommended Improvement Plan

The following items are ordered by dependency: each builds on the previous.

### Phase 1: Foundation — Provenance, Manifest, and Taxonomy

**1.1 Fixture provenance manifest**

Create a `node_compat_sources.json` recording the upstream tag and sync date for
each version lane:

```json
{
  "node20": { "tag": "v20.x.y", "synced": "2026-05-08", "upstreamStatus": "eol", "laneRole": "validation" },
  "node22": { "tag": "v22.x.y", "synced": "2026-05-08", "upstreamStatus": "lts", "laneRole": "primary" },
  "node24": { "tag": "v24.x.y", "synced": "2026-05-08", "upstreamStatus": "lts", "laneRole": "preview" }
}
```

This should make the support story explicit: Node 20 is now an EOL upstream line
but remains a deliberate Nimbus validation lane for installed-base trust;
Node 24 is upstream LTS but may still remain a Nimbus preview lane until the
runtime contract catches up.

Precedent: Deno's `node_version.ts`, Node WPT's `versions.json`, plus Node's own
release schedule metadata.

**1.2 Sync script**

A script that takes a version label and tag, checks out the tag in the local
`~/src/github.com/nodejs/node` clone, copies `test/parallel/` (and other
relevant dirs) into the fixture directory, and updates
`node_compat_sources.json`.

Precedent: Deno's `vendor.ts` (20 lines).

**1.3 Manifest-driven test declaration**

Replace the Rust macro-based batch constants with a JSONC manifest validated by
a JSON Schema:

```jsonc
{
  "$schema": "./node_compat_manifest.schema.json",
  "tests": {
    "test/parallel/test-assert-async.js": {
      "slice": "core-semantics",
      "profiles": ["application"],
      "executionClass": "parallel",
      "requires": [],
      "node20": { "fixture": "node20" },
      "node22": { "fixture": "node20" },
      "node24": { "fixture": "node24" }
    },
    "test/parallel/test-process-env-delete.js": {
      "slice": "process-and-timing",
      "profiles": ["application"],
      "executionClass": "sequential",
      "requires": ["process-env-write"],
      "node20": { "ignore": true, "reason": "application-profile env restriction" },
      "node22": { "ignore": true, "reason": "application-profile env restriction" },
      "node24": { "ignore": true, "reason": "application-profile env restriction" }
    }
  }
}
```

Per-lane fields:
- `"fixture": "node20"` — resolves to `node_compat_fixtures/node20/...`
- `"fixture": "shared"` — resolves to vendored/patched fixture
- `"ignore": true, "reason": "..."` — skipped with documented reason
- `"expectedFailure": true, "reason": "..."` — characterized known gap
- `"preview": true` — lenient (Node 24 style: import errors → skip)
- `"flaky": true` — retry up to N times
- `"extraFiles": [...]` — named extra-file groups
- `"prelude": "process-exit-sentinel"` — named prelude script

Shared entry fields:
- `"profiles": ["application"] | ["tooling"] | ["application", "tooling"]`
- `"executionClass": "parallel" | "sequential" | "known-issue" | "watchpoint"`
- `"requires": [...]` — capability gates such as `main-thread`, `tty`,
  `host-network-loopback`, `bundle-root-fs`, `crypto`, or `inspector`
- `"platforms": [...]` — optional host OS restriction when behavior is truly
  platform-specific rather than a runtime gap

This is where Nimbus should directly adopt the strongest ideas from both Node
and Deno:

- Node core harness: suite taxonomy and requirement gating are first-class
- Deno manifest: machine-readable status, reasons, and platform expectations

This replaces: 9 macros, 43 batch constants, ~3,965 lines of batch data, and 60
`#[ignore]` annotations.

Precedent: Deno's `config.jsonc` + `schema.json`.

**1.4 Node-style suite taxonomy**

Define a small, explicit suite taxonomy instead of treating every test as just
"a fixture in a slice":

- `parallel`
- `sequential`
- `known_issue`
- `watchpoint`
- `stress`
- `capability_gated`

This does not need to mirror Node's directory names exactly, but it should carry
the same architectural idea: execution class and requirement model belong to
the harness contract, not to free-text notes.

### Phase 2: Structural — File Split and Deduplication

**2.1 Split `node_compat.rs`**

With the manifest handling data declaration, split into:

```
tests/
├── node_compat/
│   ├── mod.rs            ← manifest loader + test generation
│   ├── runner.rs         ← bundle writer + execution pipeline
│   ├── preludes.rs       ← prelude/postlude scripts + matching
│   └── manifest.rs       ← JSONC parser + entry types
├── node_compat_manifest.jsonc
├── node_compat_manifest.schema.json
└── node_compat_fixtures/
```

Estimated line counts: ~150 (manifest types) + ~400 (runner) + ~200 (preludes) +
~200 (test generation) = ~950 lines total, down from 6,904.

**2.2 Fixture deduplication**

Restructure from three full copies to canonical-plus-overrides:

```
node_compat_fixtures/
├── canonical/test/            ← primary copy (Node 22 baseline)
├── overrides/
│   ├── node20/test/           ← only files that differ from canonical
│   └── node24/test/           ← only files that differ from canonical
├── vendored/test/             ← patched/adapted fixtures
└── shared/test/common/        ← harness files (index.js, fixtures.js, tmpdir.js)
```

Resolution: per the manifest, `"fixture": "node22"` reads from `canonical/`,
`"fixture": "node20"` checks `overrides/node20/` then falls back to
`canonical/`. Files that exist in `overrides/` represent actual version
divergence — immediately visible signal.

Estimated savings: ~6–8 MB (1,023 identical files removed from node20/, 739
from node24/).

### Phase 3: Quality — Expected Failures and Reporting

**3.1 Expected failure model**

Extend the manifest to support Deno-style expected failures and Node
WPT-style bidirectional detection:

- A test marked `"expectedFailure": true` runs and must fail. If it passes →
  CI flags an unexpected pass (signal to update manifest).
- A test marked `"flaky": true` is retried up to 3 times before counting as
  failed.
- A test marked `"ignore": true` is skipped entirely with a documented reason.

Precedent: Deno's five-state model, Node WPT's three-tier outcome model.

**3.2 Structured JSON reporting**

Emit a JSON report after each test run:

```json
{
  "date": "2026-05-08",
  "runtimeVersion": "0.1.0",
  "sources": {
    "node20": { "tag": "v20.x.y", "upstreamStatus": "eol", "laneRole": "validation" },
    "node22": { "tag": "v22.x.y", "upstreamStatus": "lts", "laneRole": "primary" },
    "node24": { "tag": "v24.x.y", "upstreamStatus": "lts", "laneRole": "preview" }
  },
  "slices": {
    "core-semantics": { "node20": { "passed": 180, "skipped": 5, "expectedFailure": 3 }, ... },
    "process-and-timing": { ... },
    ...
  },
  "totals": {
    "node20": { "passed": 420, "skipped": 12, "expectedFailure": 8, "failed": 0 },
    "node22": { "passed": 485, "skipped": 10, "expectedFailure": 5, "failed": 0 },
    "node24": { "passed": 380, "skipped": 45, "expectedFailure": 15, "failed": 0 }
  }
}
```

This enables: coverage dashboards, time-series tracking, evidence-backed support
claims, PR-level regression detection, and clear separation between upstream
line status and Nimbus lane role.

Precedent: Deno's `report.json` → S3 → `node-test-viewer.deno.dev`.

**3.3 Harness self-verification and shadow-oracle runs**

Add a dedicated verification layer for the compatibility harness itself:

- golden tests for manifest parsing, fixture resolution, extra-file grouping,
  skip detection, expected-failure handling, and JSON report generation
- a shadow-oracle lane that periodically runs selected fixtures in official
  Node 20 / 22 / 24 and compares Nimbus harness outcomes against the real
  process model
- explicit classification of oracle drift as a harness bug until proven to be
  a legitimate runtime difference

Because Nimbus executes upstream fixtures inside embedded V8 with custom
prelude/postlude logic, this is not optional polish. It is part of the trust
story.

**3.4 Supplementary behavioral test tier**

Introduce a distinct test tier between vendored upstream fixtures and framework
canaries: Nimbus-authored tests that verify behaviors Node's own test suite
does not comprehensively test from the perspective of an alternative runtime.
Initial coverage should
include:

- **Builtin completeness:** iterate all supported builtins, verify each is
  importable via `require('fs')`, `require('node:fs')`, `import 'fs'`,
  `import 'node:fs'` in both CJS and ESM contexts. Run per lane.
  (Precedent: Bun's `stubs.test.js`)
- **Module resolution bridge:** CJS `require()` of ESM modules, ESM `import`
  of CJS modules, `createRequire()`, conditional `exports` resolution.
  (Precedent: Deno's `require_esm_module_exports/`, `esm_dir_import/`)
- **Global injection fidelity:** verify `__dirname`, `__filename` exist in CJS
  and do not exist in ESM; verify `require` is a function in CJS contexts.
  (Precedent: Bun's `dirname.test.js`)
- **Process object shape:** verify `process.version`, `process.versions`,
  `process.features`, `process.env` structure per lane target.
  (Precedent: Deno's `process_test.ts`)
- **Resource safety:** verify `createWriteStream`/`createReadStream` do not
  leak file descriptors; verify `Buffer` operations handle detached
  `ArrayBuffer` gracefully.
  (Precedent: Bun's `fs-leak.test.js`, `buffer-copy-fill-detach.test.ts`)
- **Framework-motivated patterns:** focused reproductions of behaviors that
  real packages depend on (e.g., `Module._compile` hooks for tsx/ts-node,
  `ServerResponse` wrapping for hono/express, `worker_threads` eval mode for
  fflate).
  (Precedent: Deno's specs/node framework regression tests)

These supplementary tests should be tracked in the manifest with a `testTier`
field distinguishing them from upstream vendored fixtures. They run per lane
and per profile, and their results contribute to structured reporting and
support claims. Unlike vendored fixtures, supplementary tests CAN be modified
since Nimbus authors them.

### Phase 4: Trust — Package and Framework Canaries

**4.1 Version-pinned ecosystem canaries**

Add checked-in, version-pinned canaries for real packages and frameworks that
exercise the most important Node contracts:

- servers: `express`, `fastify`
- clients and transports: `axios`, `undici`, `socket.io`, `ws`
- loader / toolchain surfaces: `tsx`, `esbuild` or similar runtime-resolver probes
- test-runner expectations where relevant: representative Jest/Vitest-style
  environment probes

Each canary should assert at least one user-visible success condition and one
compatibility-sensitive behavior, not just "process exited 0".

**4.2 Claim mapping**

Every public framework or package support statement should map to:

- at least one upstream Node family proof
- at least one package/framework canary
- the exact Nimbus profile (`Application`, `Tooling`, or both)
- the exact supported Node lane(s)

This is the minimum evidence model required to make enterprise-trustworthy
compatibility claims.

### Phase 5: Scale — Future Node Versions

**5.1 Version-agnostic manifest schema**

Design the manifest so adding Node 26 is:
1. Add `"node26": { "tag": "v26.0.0", "synced": "..." }` to sources
2. Run sync script: `./scripts/node-compat-sync.sh node26 v26.0.0`
3. Add `"node26": { "fixture": "node26" }` or `"node26": { "preview": true }`
   to relevant test entries

No Rust code changes needed — the runner reads lane names from the manifest.
The struct and macro proliferation problem is eliminated.

**5.2 Automated fixture diffing**

Extend the sync script to report:
- New files added in upstream (candidates for new test entries)
- Changed files (need re-evaluation)
- Removed files (need cleanup)
- Files now identical to canonical (candidates for dedup)

This converts "what changed upstream?" from a manual investigation into a
machine-generated report.

### Phase 6: Correctness — Version-Specific Behavior and Bare Specifiers

**6.1 Version-parameterized `RuntimeCompatibilityTarget`**

Extend the `RuntimeCompatibilityTarget` enum with `Node20` and `Node24`
variants. Each variant configures:
- `process.version` (e.g., `"v20.20.2-nimbus"` vs. `"v22.15.0-nimbus"`)
- Stream default `highWaterMark` values (16 KiB for Node 20, 64 KiB for
  Node 22+)
- API surface differences (`process.features.typescript` absent in Node 20)
- Deprecation behavior differences (`once(emitter, event, null)` accepted
  in Node 20, rejected in Node 22)
- Error message format differences (trailing `\n` in assert diffs)

The test manifest's per-lane configuration maps to the correct target:
```jsonc
"test/parallel/test-assert-checktag.js": {
  "node20": { "fixture": "node20", "target": "Node20" },
  "node22": { "fixture": "node22", "target": "Node22" },
  "node24": { "fixture": "node24", "target": "Node24" }
}
```

The test runner creates the runtime with the lane-appropriate target:
```rust
let limits = match lane {
    Node20 => RuntimeLimits::application_node20(),
    Node22 => RuntimeLimits::application_node22(),
    Node24 => RuntimeLimits::application_node24(),
};
```

This eliminates the "Pinned Node20 divergence" ignores — those tests should
pass when run against a Node 20-shaped runtime.

**6.2 Bare specifier verification**

Add a standalone test fixture that verifies both bare and `node:`-prefixed
resolution for every supported builtin module in both CJS and ESM contexts.
This fixture runs in every lane and catches regressions where a bare import
fails or resolves differently from the prefixed form.

The ESM path in `supports_extension_backed_node_builtin()` (source:
`module_loader.rs` lines 4012–4053) currently only matches `node:`-prefixed
specifiers. This should be extended to also resolve bare ESM imports of
builtins, matching the behavior of Node.js and Bun. Deno handles this via
the `--unstable-bare-node-builtins` flag; Nimbus should handle it natively
since enterprise users expect `import fs from 'fs'` to work.

**6.3 Behavioral divergence matrix**

Document the known behavioral differences between Node 20, 22, and 24 in a
structured format (JSONC or Markdown table) that can be:
- Cross-referenced by the test runner to validate version-correct behavior
- Published as part of the compatibility documentation
- Used by the version-parameterized runtime to configure version-specific
  defaults

Example dimensions to track:

| Behavior | Node 20 | Node 22 | Node 24 |
|----------|---------|---------|---------|
| `process.features.typescript` | absent | present | present |
| Stream default highWaterMark | 16 KiB | 64 KiB | 64 KiB |
| `once(emitter, event, null)` | accepted | rejected | rejected |
| Assert diff trailing newline | no | yes | yes |
| `PerformanceResourceTiming.deliveryType` | absent | present | present |

---

## References

### Primary sources (local)

| Reference | Path |
|-----------|------|
| Nimbus node_compat.rs | `crates/nimbus-runtime/src/runtime/tests/node/mod.rs` |
| Nimbus Node compatibility roadmap | `docs/plans/archive/node-lts-compatibility-plan.md` |
| Nimbus fixture root | `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/` |
| Node core test harness overview | `~/src/github.com/nodejs/node/test/README.md` |
| Node core test runner | `~/src/github.com/nodejs/node/tools/test.py` |
| Node core status file | `~/src/github.com/nodejs/node/test/root.status` |
| Node common harness docs | `~/src/github.com/nodejs/node/test/common/README.md` |
| Deno config.jsonc | `~/src/github.com/nimbus/deno/tests/node_compat/config.jsonc` |
| Deno schema.json | `~/src/github.com/nimbus/deno/tests/node_compat/schema.json` |
| Deno mod.rs (runner) | `~/src/github.com/nimbus/deno/tests/node_compat/mod.rs` |
| Deno vendor.ts | `~/src/github.com/nimbus/deno/tests/node_compat/runner/suite/vendor.ts` |
| Deno node_version.ts | `~/src/github.com/nimbus/deno/tests/node_compat/runner/suite/node_version.ts` |
| Deno unit_node tests | `~/src/github.com/nimbus/deno/tests/unit_node/` |
| Deno specs/node tests | `~/src/github.com/nimbus/deno/tests/specs/node/` |
| Node WPT versions.json | `~/src/github.com/nodejs/node/test/fixtures/wpt/versions.json` |
| Node WPT status files | `~/src/github.com/nodejs/node/test/wpt/status/` |
| Node upstream tests | `~/src/github.com/nodejs/node/test/` |

### External references

| Reference | URL |
|-----------|-----|
| Deno node_compat directory | https://github.com/denoland/deno/tree/main/tests/node_compat |
| Deno node_test vendored repo | https://github.com/denoland/node_test |
| Deno Node compat CI workflow | https://github.com/denoland/deno/blob/main/.github/workflows/node_compat_test.generated.yml |
| Deno public test dashboard | https://node-test-viewer.deno.dev/results/latest |
| Deno "run all Node tests" issue | https://github.com/denoland/deno/issues/28318 |
| Bun GitHub repository | https://github.com/oven-sh/bun |
| Bun 1.2 blog post (Node test suite) | https://bun.sh/blog/bun-v1.2 |
| Bun Node.js compat docs | https://bun.sh/docs/runtime/nodejs-compat |
| Bun Node test CLAUDE.md | https://github.com/oven-sh/bun/blob/main/test/js/node/test/parallel/CLAUDE.md |
| Bun test README | https://github.com/oven-sh/bun/blob/main/test/README.md |
| Bun ecosystem CI | https://github.com/oven-sh/bun-ecosystem-ci |
| Bun supplementary Node tests | https://github.com/oven-sh/bun/tree/main/test/js/node |
| Deno unit_node tests | https://github.com/denoland/deno/tree/main/tests/unit_node |
| Deno specs/node tests | https://github.com/denoland/deno/tree/main/tests/specs/node |
| Cloudflare workerd | https://github.com/cloudflare/workerd |
| Cloudflare workerd Node API tests | https://github.com/cloudflare/workerd/tree/main/src/workerd/api/node/tests |
| Cloudflare compat date flags | https://github.com/cloudflare/workerd/blob/main/src/workerd/io/compatibility-date.capnp |
| Cloudflare Workers Node compat matrix | https://github.com/cloudflare/workers-nodejs-compat-matrix |
| Cloudflare Workers Node compat matrix (live) | https://workers-nodejs-compat-matrix.pages.dev/ |
| Node.js core test harness overview | https://github.com/nodejs/node/tree/main/test |
| Node.js release schedule | https://nodejs.org/en/about/previous-releases |
| Node.js WPT runner | https://github.com/nodejs/node/tree/main/test/wpt |
| Node.js WPT versions.json | https://github.com/nodejs/node/blob/main/test/fixtures/wpt/versions.json |
| Web Platform Tests | https://github.com/web-platform-tests/wpt |
| wpt.fyi cross-runtime results | https://wpt.fyi/ |
| unjs runtime-compat | https://runtime-compat.unjs.io/ |
