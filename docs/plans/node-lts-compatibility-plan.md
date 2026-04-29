# Node LTS Compatibility Plan

Status: in_progress

Canonical execution plan for completing Node.js built-in module compatibility
on top of the landed `docs/plans/node-compatible-runtime-plan.md` baseline.
This plan owns the broad follow-on wave required to make Neovex's
`deno_core`/V8 runtime credibly compatible with the Node.js 22 LTS built-in
module contract while preserving a Node.js 20 compatibility lane for upstream
ecosystem support.

This is an execution control plane, not a one-off architecture memo. It is
designed so an agent can resume from the current git worktree plus this file,
survive compaction, close one roadmap item, checkpoint the result, and
continue to the next eligible item without waiting for fresh human direction.

The prior plan, `docs/plans/node-compatible-runtime-plan.md`, is complete. It
remains the latest completed baseline for:

- one canonical `deno_core`/V8 runtime backend
- `RuntimeProfile::Application` and `RuntimeProfile::Tooling`
- `CompatibilityTarget::WebStandardIsolate` and `CompatibilityTarget::Node22`
- Deno-family fork alignment through `~/src/github.com/agentstation/deno`
  and `~/src/github.com/agentstation/rusty_v8`
- the conservative public contract that still keeps external Node.js in the
  happy-path authoring flow

This new plan owns the next wave: **full Node built-in module parity work,
versioned compatibility truth, upstream test automation, and the public
contract needed before Neovex can honestly claim Node LTS compatibility.**

## Status

- **Plan status:** `in_progress`
- **Control item:** `—`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Control item rule:** when a roadmap item is marked `in_progress`, mirror
  it here. Use `—` only when no item is currently active.
- **Primary source of truth:** this file plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status and the execution log before stopping.
- **Continuation rule:** after closing one roadmap item and recording its
  verification, immediately advance to the next eligible `pending` item in the
  same session unless the work is blocked or the whole plan is complete.

## Plan Ownership And Canonical Inputs

This is the active plan for Node built-in module compatibility, Node 20/22
truth, Deno-family Node API adoption, and Node-upstream validation work.
Do not start another broad runtime-compatibility or Node-parity wave without
promoting a new active plan that cites this one as the last execution owner.

Implementation work must keep these source inputs open:

- Top-level repo references:
  `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`
- Completed baseline:
  `docs/plans/node-compatible-runtime-plan.md`
- Runtime boundary references:
  `docs/architecture/runtime/adapter-boundary.md`,
  `docs/architecture/server/auth-runtime-trust.md`,
  `docs/architecture/runtime/node-compat-surface-matrix.md`
- Current implementation roots:
  `crates/neovex-runtime/`, `crates/neovex-bin/`, `packages/codegen/`
- Canonical local fork worktrees:
  - `~/src/github.com/agentstation/deno`
  - `~/src/github.com/agentstation/rusty_v8`
  - `~/src/github.com/agentstation/deno_core` as historical delta reference
    only
- Upstream comparison worktree:
  `~/src/github.com/denoland/deno`
- Primary external truth sources:
  - Node.js 20 docs: `https://nodejs.org/docs/latest-v20.x/api/`
  - Node.js 22 docs: `https://nodejs.org/docs/latest-v22.x/api/`
  - Node.js v20 changelog:
    `https://github.com/nodejs/node/blob/main/doc/changelogs/CHANGELOG_V20.md`
  - Node.js v22 changelog:
    `https://github.com/nodejs/node/blob/main/doc/changelogs/CHANGELOG_V22.md`
  - Node.js test suite: `https://github.com/nodejs/node/tree/main/test`
  - Deno Node compatibility table:
    `https://docs.deno.com/runtime/reference/node_apis/`
  - Deno `ext/node` source:
    `https://github.com/denoland/deno/tree/main/ext/node`
  - LLRT API and modules:
    `https://github.com/awslabs/llrt/blob/main/API.md`,
    `https://github.com/awslabs/llrt/tree/main/llrt_modules`

## Autonomous Execution Contract

This plan is designed to survive compaction and resume autonomously. Each
roadmap item must be actionable from:

- this file
- the execution log
- the current git worktree
- the source files
- the canonical fork worktrees listed above

An agent resuming this plan must:

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, the completed
   `docs/plans/node-compatible-runtime-plan.md`, and this plan.
2. Run `git status --short` before choosing work.
3. Treat existing dirty changes as intentional progress state. Do not revert
   unrelated edits.
4. Resume the existing `in_progress` item if one exists.
5. Otherwise pick the first `pending` item in roadmap order whose hard deps
   are `done`.
6. Continue directly to the next eligible item after a closeout instead of
   stopping at a verification boundary.

## Control Plan Rules

1. Read the canonical docs and this plan before starting a roadmap item.
2. Run `git status --short` before choosing work. If the worktree is dirty,
   reconcile before editing.
3. If any roadmap item is `in_progress`, resume it. If none, pick the first
   `pending` item in roadmap order whose hard deps are `done`.
4. Mark exactly one roadmap item `in_progress` before implementation.
5. After an item reaches `done` and its verification is logged, immediately
   start the next eligible `pending` item in the same session unless the work
   is blocked or the whole plan is complete.
6. A roadmap item is not `done` until its verification, touched files, and
   contract changes are recorded in the execution log.
7. If an item is blocked by an external dependency, mark it `blocked`, record
   the exact blocker, and continue to the next eligible `pending` item whose
   hard deps are satisfied and whose work is independent of the blocker.
8. Use one Cargo or `make` verification lane at a time against the shared
   target directory. Do not paper over lock contention with parallel workspace
   verification storms.

## Verification Contract

Every completed roadmap item must leave durable evidence:

- the roadmap item status is updated
- the execution log records the date, item, status, files touched, and
  verification
- focused tests or fixtures cover the changed behavior
- `cargo fmt --all --check` and `make clippy` run after each implementation
  item unless the log records a concrete blocker
- broader verification runs when the item changes public behavior or external
  compatibility claims
- compatibility matrices, docs, and plan claims are narrowed immediately if a
  verification lane fails

## Why This Plan Exists

Neovex now has the runtime shape it wanted: a single `deno_core`/V8 backend,
versioned compatibility targets, Deno-family fork alignment, and a conservative
public Node22 contract. What it does **not** have yet is a truthful basis for
claiming full Node LTS compatibility.

The critical gaps are no longer architectural ambiguity. They are:

- symbol-by-symbol compatibility truth for Node 20 and Node 22
- upstream Node behavioral validation instead of module-presence optimism
- closure of remaining Deno-family Node API gaps that block real npm packages
- clear separation between:
  - modules that exist in `ext/node`
  - modules whose APIs are present but partial
  - modules whose behavior is still observably non-Node-compatible
- a versioned public contract the README and CLI docs can defend without
  overstating support

The biggest lesson from the previous runtime wave is that **"module exists" is
not the same thing as "module is Node-compatible."** This plan therefore makes
the generated compatibility matrix and Node-upstream test harness the first
class execution artifacts, not after-the-fact polish.

## Scope

- Build and maintain a generated Node 20 / Node 22 symbol inventory from
  official Node docs.
- Build and maintain a generated Deno-family implementation inventory from
  `ext/node`, sibling `deno_*` crates, and the Deno compatibility table.
- Produce a checked-in compatibility matrix that joins the two and records
  `ImplementedFull`, `ImplementedPartial`, `StubOnly`, `NotImplemented`, and
  `NeedsVerification`.
- Close the remaining Node built-in module gaps needed for a truthful Node 22
  contract.
- Preserve a Node 20 validation lane for upstream ecosystem support.
- Add upstream Node test execution and pass-rate reporting by module family.
- Add package-level and framework-level smoke tests for the Node contract.
- Keep the public compatibility docs and CLI claims truthful as support
  changes.

## Non-Goals

- Claiming "all Node versions" compatibility.
- Replacing the `V8DenoCore` backend with Bun, workerd, or another engine in
  this plan.
- Claiming full npm ecosystem parity solely from built-in module support.
- Claiming unrestricted host behavior when runtime capabilities remain scoped.
- Leaving the compatibility matrix as a hand-maintained spreadsheet in prose.

## Versioned Contract Decision

This plan standardizes the public contract as follows:

- **Primary compatibility target:** `Node22`
- **Compatibility validation lane:** `Node20`
- **Preserved non-Node target:** `WebStandardIsolate`

Implications:

- Node 22 is the versioned built-in module contract Neovex should optimize for,
  document, and gate in CI.
- Node 20 remains important because upstream Convex and Firebase ecosystems may
  continue to support it for some time, but Neovex should treat it as a
  compatibility lane rather than the primary named baseline.
- A successful Node 22 claim is not enough if the runtime regresses the
  preserved `WebStandardIsolate` application target.

## Current Local Baseline And Worktree Resume Rules

The current visible worktree already contains relevant progress from the
completed Node runtime wave. At the time this plan was created, the repo had
dirty changes in:

- `crates/neovex-runtime/` across bootstrap, loader, runtime capabilities,
  Node22 bootstrap files, and tests
- `crates/neovex-bin/` across codegen, `dev`, and Node integration
- `packages/codegen/` across parser and Cloud Functions support
- runtime compatibility docs including
  `docs/architecture/runtime/node-compat-surface-matrix.md`
- the completed `docs/plans/node-compatible-runtime-plan.md`

Resume rules:

- treat those changes as progress state, not noise
- do not reset the tree to fabricate a clean starting point
- NLC0 must checkpoint the current worktree against the new generated matrix
  and public contract before deeper parity work resumes

## Research-Derived Compatibility Truth

Primary-source research completed before this plan established these facts:

- Node 22 adds a real built-in `node:sqlite` module that does not exist in
  Node 20.
- Node 22 expands `node:module` materially with:
  - `registerHooks()`
  - compile-cache APIs
  - `findPackageJSON()`
  - `stripTypeScriptTypes()`
- Node 22 adds `fs.glob()` and `fs/promises.glob()`.
- Node 22 adds `process.finalization.register()`,
  `process.finalization.registerBeforeExit()`, and
  `process.finalization.unregister()`.
- `process.loadEnvFile()` already exists in Node 20.
- `require(esm)` becomes enabled by default in Node 22.12, making loader
  fidelity a cross-cutting compatibility requirement.
- Deno already carries broad module-level surface for many difficult modules,
  including `http2`, `inspector`, `worker_threads`, and `sqlite`; the hard
  remaining work is behavioral parity, not empty-module scaffolding.

## Supplemental Reference Inputs

These inputs may inform implementation strategy, but they are **not** allowed
to override the primary truth sources above.

Use them this way:

- `rustyscript`
  - good reference for how another embedder wires `deno_core`/Deno-family
    crates together behind a Rust API
  - its README explicitly describes Node support as experimental, so it is a
    wiring reference, not a compatibility truth source
- `deno_runtime`
  - good reference for Deno's own composed runtime layer and `MainWorker`
  - docs explicitly state the crate API is subject to rapid and breaking
    changes, so it is a composition reference, not a stable architectural
    owner for Neovex
- `unenv`
  - useful for selective JS-side polyfills where the gap is genuinely
    polyfillable
  - not a blanket compatibility answer: official docs show that some modules
    are polyfilled while many others are mocked or stubbed, and Cloudflare's
    docs explicitly warn that mocked methods may noop or throw
  - therefore unenv-backed coverage must still pass the same matrix/test gates
    as native implementations before being labeled `Supported`
- `nodejs/ncrypto`
  - strong candidate reference for `node:crypto` fidelity work because it is
    Node's extracted crypto implementation
  - use as an explicit implementation spike option under the crypto family,
    not as an assumed dependency choice
  - current Deno-family baseline already carries substantial crypto
    implementation in `deno_node_crypto` and `deno_crypto`, backed by
    `aws-lc-rs`/`aws-lc-sys` plus Rust crypto crates; `ncrypto` is therefore
    a potential fidelity upgrade path for specific gaps, not proof that the
    current stack is shallow or disposable
- `cloudflare/workers-nodejs-compat-matrix`
  - useful supplemental benchmark and comparison harness
  - its own README calls it a "quick and dirty audit", so it is not a primary
    completion gate
- `runtime-compat.unjs.io`
  - useful background comparison site for runtime trends
  - not suitable as a primary source for Node built-in parity: it says its
    data is auto-generated, not 100% accurate, and is based on
    `runtime-compat-data`/MDN-style API metadata
- `deno_lib`
  - historical packaging reference only
  - docs describe it as unofficial, and some published versions were yanked
  - do not make it a new architectural dependency or truth source

Planning rule:

- if one of these supplemental inputs disagrees with Node docs, Node
  changelogs, Node upstream tests, Deno-family source, or measured Neovex
  behavior, the supplemental input loses

## Generated Artifact Contract

This plan does **not** rely on a hand-authored symbol list in prose. NLC1
must create and maintain generated truth artifacts, checked into the repo,
under a stable runtime-compatibility doc root.

Canonical locations:

- generator scripts and helpers:
  `scripts/node_compat/`
- generated machine-owned artifacts:
  `docs/architecture/runtime/node-lts-compat/`
- runtime- or package-facing fixtures needed by tests:
  `crates/neovex-runtime/tests/node_compat/` or another concept-owned test root
  chosen during implementation and recorded in the execution log

Required artifacts:

- `node20-symbols.csv`
- `node22-symbols.csv`
- `node20-vs-node22-delta.csv`
- `deno-node-impl-inventory.csv`
- `node-lts-compat-matrix.csv`
- `node-lts-compat-summary.md`

Each row in the merged matrix must include:

- `module`
- `symbol`
- `kind`
- `node20_status`
- `node22_status`
- `added_in`
- `deprecated_in`
- `deno_coverage`
- `verification_status`
- `notes`

The checked-in markdown summary must be human-readable and suitable for public
docs and PR review, but the CSV artifacts remain the canonical machine-owned
truth.

Generation rules:

- Node symbol inventories must come from official Node docs and changelogs, not
  from hand-authored lists.
- Deno implementation inventory must come from the checked-out Deno-family
  source plus the Deno compatibility table, not from memory or marketing docs.
- Every generated artifact must record:
  - generation date
  - source family/version used for Node20, Node22, and Deno
  - generator script version or commit context
- If the generator cannot classify a symbol confidently, it must emit
  `NeedsVerification`, not infer support.

## Support-State Taxonomy

This plan needs a richer support model than a flat
`Supported` / `Partial` / `Not Supported` label, because Neovex already has
runtime profiles with different capability envelopes.

The matrix and public docs must use these support-state labels:

- `Supported`
  - the API is implemented and verified for the relevant compatibility target
    and runtime profile
- `SupportedToolingOnly`
  - the API is intentionally supported in `RuntimeProfile::Tooling` but not in
    `RuntimeProfile::Application`
- `Partial`
  - some surface exists, but there are known missing APIs or behavior gaps
- `StubOnly`
  - surface exists mainly to throw or no-op
- `NotSupported`
  - Neovex does not support the API in the named target/profile
- `NeedsVerification`
  - implementation likely exists, but verification evidence is not yet strong
    enough to make a public claim

Required matrix columns beyond symbol metadata:

- `compatibility_target`
- `runtime_profile`
- `support_state`
- `verification_lane`

Public-contract rule:

- Neovex may only claim **full Node built-in compatibility** for a target and
  profile pair if every in-scope built-in module for that target/profile pair
  is `Supported`.
- If any built-in module for a target/profile pair is `SupportedToolingOnly`,
  `Partial`, `StubOnly`, `NotSupported`, or `NeedsVerification`, Neovex must
  use a narrower contract label such as:
  - `Node22 compatibility target with documented profile-scoped exclusions`
  - `Tooling-only Node22 support for selected built-ins`
  - `Partial Node22 compatibility`
- If `child_process`, addon loading, raw sockets, or other host-sensitive APIs
  remain capability-scoped to `RuntimeProfile::Tooling`, the docs must say so
  explicitly instead of collapsing them into a generic Node22 support claim

## Module Family Execution Map

This plan executes by module family rather than by ad hoc missing methods.

| Family | Modules | Priority | Why it matters |
|--------|---------|----------|----------------|
| Core semantics | `assert`, `events`, `buffer`, `path`, `url`, `console`, `querystring`, `punycode`, `string_decoder` | P0 | Foundation for most npm packages and higher-level modules |
| Process and timing | `process`, `timers`, `util`, `diagnostics_channel`, `perf_hooks` | P0 | Required for loaders, frameworks, observability, and correct runtime behavior |
| Streams and local I/O | `stream`, `fs`, `readline`, `tty`, `os` | P0 | Streams and fs correctness dominate Node compatibility in practice |
| Networking | `dns`, `net`, `dgram`, `tls`, `http`, `https`, `http2` | P0/P1 | Required for servers, clients, agents, and major frameworks |
| Crypto and compression | `crypto`, `zlib` | P0/P1 | Blocks package installs, integrity checks, auth flows, and many libraries |
| Loader and async context | `module`, `async_hooks` | P0/P1 | Required for `require(esm)`, transpilers, request context, and modern frameworks |
| Host process and toolchain | `child_process`, `test`, `repl` | P1 | Required for serious tooling and CLI parity; must be truthful if capability-scoped profiles intentionally restrict them |
| VM/runtime internals | `vm`, `v8`, `worker_threads`, `inspector` | P1/P2 | Needed for advanced tools, sandboxes, and deep Node parity |
| Long-tail / host-heavy | `cluster`, `sqlite`, `wasi`, `sea`, `domain`, `trace_events`, `sys`, `constants` | P2/P3 | Important for completeness, but lower priority than the foundation path |

## Dependency Graph

Execution order must respect these major dependencies:

1. `events`, `buffer`, `process`, `timers`, `util`, `path`, `url`
2. `stream`, `string_decoder`, `console`, `querystring`
3. `fs`, `os`, `tty`, `readline`
4. `dns`, `net`, `dgram`
5. `crypto`, `zlib`, `tls`
6. `http`, `https`
7. `async_hooks`, `diagnostics_channel`, `perf_hooks`
8. `module` and loader fidelity, plus `child_process`
9. `vm`, `v8`, `worker_threads`
10. `http2`, `inspector`, `test`, `repl`
11. `cluster`, `sqlite`, `wasi`, `sea`, `domain`, `trace_events`, `sys`,
    `constants`

Critical path notes:

- `http` and `https` depend on `net`, `stream`, `buffer`, and `process`.
- `http2` depends on `tls`, `net`, `stream`, and event-loop correctness.
- `module` and `async_hooks` are cross-cutting blockers for framework claims.
- `child_process` depends on correct `process`, `stream`, and host-handle
  semantics even when runtime profiles intentionally scope capability access.
- `worker_threads`, `vm`, and `inspector` depend on the final context,
  loader, and V8 integration seam.

## Family Closeout Contract

Roadmap items NLC3 through NLC9 are not complete unless they satisfy all of
these conditions for the module families they own:

- every symbol in the generated matrix for the owned modules has a non-empty
  support-state classification for `Node22` and the relevant runtime profile
- every owned module has at least one mapped upstream Node test slice recorded
  in a checked-in manifest
- the Node22 upstream test slice for the family either:
  - reaches at least **95% pass rate** for P0/P1 families, or
  - has every remaining failure listed in a checked-in failure inventory with a
    disposition of `intentional_profile_restriction`, `known_runtime_gap`,
    `upstream_deno_gap`, `harness_issue`, or `upstream_node_delta`
- the Node20 lane is run for the same family and any divergence from Node22 is
  reflected in the matrix or failure inventory
- every applicable package/framework canary mapped to the family is run and its
  result recorded
- no unexplained failures remain

Canonical checked-in evidence for family closeout:

- upstream test manifest:
  `docs/architecture/runtime/node-lts-compat/manifests/<family>.md`
- failing-test inventory:
  `docs/architecture/runtime/node-lts-compat/failures/<family>.md`

P2/P3 families may close below 95% pass rate only when the matrix, failure
inventory, and public docs all state the narrower support claim explicitly.

## Validation Strategy

Every item in this plan must validate against at least one of these layers:

1. **Runtime unit and integration tests**
   Local Neovex crate tests for focused behavior.
2. **Node upstream module tests**
   `nodejs/node/test` subsets matched to the module family under change.
3. **Package compatibility canaries**
   Stable package set covering common Node APIs and loader expectations.
4. **Framework smoke tests**
   End-to-end validation for the most important ecosystem claims.

Pinned upstream test corpus contract:

- Node upstream tests must not be fetched from a floating branch at runtime.
- The plan must pin a specific Node source family for the test corpus,
  preferably by vendoring or checking out a pinned git tag under a stable
  repo-owned location recorded in the execution log.
- The pinned corpus version for the Node22 primary lane and Node20 validation
  lane must be recorded in:
  - the generated compatibility summary
  - the upstream test manifests
  - the execution log for the item that updates the corpus
- If the pinned upstream corpus changes, the matrix and failure inventories
  must be regenerated in the same change.

Required stable framework/package canaries:

- `express`
- `fastify`
- `socket.io`
- `undici`
- `axios`
- `jest`
- `tsx`
- `ts-node`
- `prisma`
- `next`

The package canary set may grow, but it must stay version-pinned and checked
into the repo so the compatibility story is reproducible.

Canonical package-canary contract:

- checked-in manifests and harness code should live under a stable root such as
  `demos/node-compat/` or `tests/node-compat/`, chosen once and then recorded
  in the execution log
- package versions must be pinned, not floating
- every framework/package claim in docs must map to a checked-in canary lane

Required canary mapping contract:

| Canary | Compatibility target | Runtime profile | Minimum assertion depth |
|--------|----------------------|-----------------|-------------------------|
| `express` | `Node22` primary, `Node20` validation | `Application` | server boots, route responds, middleware chain runs, error path returns expected status |
| `fastify` | `Node22` primary, `Node20` validation | `Application` | server boots, route responds, plugin registration works |
| `socket.io` | `Node22` primary | `Application` | handshake succeeds, event roundtrip works |
| `undici` | `Node22` primary | `Application` | request succeeds, response body is consumed correctly |
| `axios` | `Node22` primary | `Application` | basic HTTP request succeeds and error path is asserted |
| `jest` | `Node22` primary | `Tooling` | test discovery runs, one test passes, process exits successfully |
| `tsx` | `Node22` primary | `Tooling` | TypeScript entrypoint executes and prints asserted output |
| `ts-node` | `Node22` primary | `Tooling` | TypeScript entrypoint executes and prints asserted output |
| `prisma` | `Node22` primary | `Tooling` | client boots and one query/mutation smoke succeeds or the precise unsupported addon restriction is documented |
| `next` | `Node22` primary | `Tooling` plus any required `Application` sub-lane | `next build` succeeds and the documented runtime slice under test boots with an asserted route response |

Canary closeout rule:

- a green process exit alone is not enough
- every canary must assert at least one user-visible success condition and one
  relevant error or edge condition when practical
- if a canary only passes in `RuntimeProfile::Tooling`, the public docs must
  not imply the same package/framework is supported in
  `RuntimeProfile::Application`

## Phase Status Ledger

| Phase | Status | Items | Done when |
|-------|--------|-------|-----------|
| P0: Truth and control-plane activation | `in_progress` | NLC0-NLC2 | Generated matrix exists, active plan wired, public contract versioned and truthful |
| P1: Foundation built-ins | `pending` | NLC3-NLC7 | P0 module families reach target support and upstream test coverage |
| P2: Deep runtime and host parity | `pending` | NLC8-NLC9 | Loader, workers, VM, inspector, and long-tail host modules have truthful support state |
| P3: Full validation and public closeout | `pending` | NLC10 | CI/dashboard/regression lanes exist and docs can defend the compatibility claim |

## Roadmap Items

### P0 Work Queue: Truth And Control-Plane Activation

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------|
| NLC0 Active control-plane activation and worktree checkpoint | `done` | none | This plan is added to `docs/plans/README.md` as an active execution plan, `AGENTS.md` points to it as the active owner for Node built-in compatibility work, and the execution log records the current dirty-tree baseline plus the first resumable implementation seam. |
| NLC1 Generated Node 20 / Node 22 / Deno compatibility artifacts | `pending` | NLC0 | Checked-in generated symbol inventories, Node20↔Node22 delta, Deno implementation inventory, merged compatibility matrix, and a human-readable summary exist under a stable docs/runtime path. No hand-maintained spreadsheet claims remain the primary source of truth. |
| NLC2 Public contract, versioning, and support-state baseline | `pending` | NLC1 | README/runtime docs/public matrices explicitly define Node22 as the primary target, Node20 as a compatibility lane, and the initial module support states without overclaiming full parity. |

### P1 Work Queue: Foundation Built-Ins

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------|
| NLC3 Core semantics family | `pending` | NLC2 | `assert`, `events`, `buffer`, `path`, `url`, `console`, `querystring`, `punycode`, and `string_decoder` satisfy the family closeout contract, with Node22 pass rate >=95%, Node20 divergence documented, and no unexplained failures. |
| NLC4 Process and timing family | `pending` | NLC3 | `process`, `timers`, `util`, `diagnostics_channel`, and `perf_hooks` satisfy the family closeout contract, including explicit handling of `process.finalization.*`, `loadEnvFile()`, util MIME types, and performance APIs. |
| NLC5 Streams and local I/O family | `pending` | NLC4 | `stream`, `fs`, `readline`, `tty`, and `os` satisfy the family closeout contract, including Node22 `glob()` support and documented platform-specific limitations where applicable. |
| NLC6 Networking family | `pending` | NLC5 | `dns`, `net`, `dgram`, `tls`, `http`, `https`, and `http2` satisfy the family closeout contract, with mapped package canaries for request/response/server/socket behavior and no unexplained failures. |
| NLC7 Crypto, compression, and loader-context foundation | `pending` | NLC6 | `crypto`, `zlib`, `async_hooks`, and the foundation of `module` compatibility satisfy the family closeout contract, with `AsyncLocalStorage` and loader interoperability treated as blocking framework claims. |

### P2 Work Queue: Deep Runtime And Host Parity

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------|
| NLC8 Loader, host-process, VM, workers, and V8 family | `pending` | NLC7 | `module`, `child_process`, `vm`, `v8`, `worker_threads`, and `inspector` satisfy the family closeout contract, and `require(esm)` plus Node22 loader APIs are either supported or explicitly documented as narrower support states by runtime profile. |
| NLC9 Long-tail and host-heavy family | `pending` | NLC8 | `cluster`, `repl`, `test`, `sqlite`, `wasi`, `sea`, `domain`, `trace_events`, `sys`, and `constants` each reach a truthful support state with checked-in manifests, failure inventories, and docs matching reality. |

### P3 Work Queue: Full Validation And Public Closeout

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------|
| NLC10 Upstream test dashboard, npm canaries, framework smoke, and closeout | `pending` | NLC9 | CI has per-module Node test slices, nightly compatibility dashboards, package canary lanes, framework smoke lanes, pinned upstream Node test corpus metadata, and public docs/matrices synchronized to the measured results. The final support claim is evidence-backed rather than aspirational. |

## Detailed Item Notes

### NLC0: Active Control-Plane Activation And Worktree Checkpoint

This item exists to prevent plan drift and repeated rediscovery.

Implementation notes:

- promote this file into `docs/plans/README.md`
- update `AGENTS.md` so future agents discover this plan first for Node built-in
  compatibility work
- checkpoint the current dirty tree as the starting execution baseline
- identify the concrete source roots for generated compatibility artifacts

### NLC1: Generated Compatibility Artifacts

This item is the cornerstone of the whole plan.

Implementation notes:

- prefer a checked-in generator over prose-maintained tables
- use official Node docs and changelogs as the sole truth source for Node
  symbol inventory
- use Deno source plus the Deno compatibility table as the implementation
  inventory
- mark unresolved symbols `NeedsVerification`, not `Supported`
- choose and document the final stable artifact locations described above
- record the precise source version identifiers used for the first generated
  baseline
- make the generator rerunnable without requiring chat-history context

Verification expectations:

- a focused script or test validates the generated CSV schema
- at least one diff-based verification lane proves the generator is stable when
  rerun against the same source inputs
- public markdown summary and CSV artifacts agree on counts

### NLC2: Public Contract And Support-State Baseline

This item prevents support-state drift and overclaiming.

Implementation notes:

- update public docs to reflect the support-state taxonomy above
- make the profile-scoped posture explicit for host-sensitive APIs
- do not allow a plain "Node22 supported" claim that hides
  `SupportedToolingOnly` modules
- synchronize the existing `node-compat-surface-matrix.md` contract with the
  richer target/profile support-state model

Verification expectations:

- public docs, matrices, and any README summary use the same support-state
  vocabulary
- if any module remains `SupportedToolingOnly`, `Partial`, `StubOnly`,
  `NotSupported`, or `NeedsVerification`, the plan does not permit a "full
  Node built-in compatibility" claim for that target/profile pair

### NLC7-NLC8: Framework-Claim Gate

These items control whether Neovex can honestly claim framework compatibility.

Before claiming success for frameworks such as Next.js, Jest, Prisma, or tsx,
the plan must close or explicitly narrow:

- `AsyncLocalStorage` correctness
- loader/`require(esm)` semantics
- `child_process` posture for tooling-profile execution
- worker lifecycle semantics
- native addon posture via `deno_napi`
- `process`, `stream`, `fs`, `http`, and `crypto` behavior fidelity

### NLC7: Crypto Strategy Decision Gate

`node:crypto` is important enough, and implementation options are expensive
enough, that agents must not jump straight from "there are gaps" to "replace
the crypto stack."

Current spike result:

- Neovex's current Deno-family baseline is already substantial:
  - `deno_node_crypto` implements a large Node-facing crypto op surface
  - `deno_crypto` implements the Web Crypto base
  - the current stack is backed by `aws-lc-rs` / `aws-lc-sys` plus a broad set
    of Rust crypto crates rather than a thin shim layer
- The Deno family also carries meaningful crypto-focused test coverage:
  - dedicated `tests/unit_node/crypto/*`
  - separate random/entropy tests
  - Node compatibility runner coverage for crypto-adjacent Node tests
- Deno's public compatibility table still records real `node:crypto` caveats,
  including stubs and behavior limits for some symbols, so "broad
  implementation" is still not the same thing as "full fidelity"
- `nodejs/ncrypto` is a credible fidelity candidate:
  - it is a real Node-owned extraction of the internal crypto implementation
  - it supports OpenSSL and BoringSSL linkage
  - public Cloudflare material says it is used to improve Node crypto fidelity
    in Workers, and that Bun uses it as well
- However, the first spike does **not** justify immediate adoption:
  - the current Deno-family crypto baseline is already deep enough that
    replacing it wholesale would be a major architecture decision
  - integrating `ncrypto` from Rust would introduce C++/FFI/build-system and
    release-process complexity inside `agentstation/deno`
  - no measured Neovex failure inventory yet proves that the remaining gaps
    are better solved by `ncrypto` than by targeted fixes to the current stack

Decision rule:

- Neovex starts from the Deno-family crypto baseline already in use:
  `deno_node_crypto` + `deno_crypto`
- `nodejs/ncrypto` is a candidate **only** if measured evidence shows that the
  current Deno-family stack cannot reach the NLC7 family gate cleanly enough
  with targeted fixes

Trigger conditions for the `ncrypto` spike:

- Node22 `crypto` family pass rate remains below the plan threshold after the
  ordinary Deno-family fix path has been exhausted
- or critical package/framework canaries remain blocked by documented
  `node:crypto` fidelity issues
- or the remaining failures cluster around behavior that is plausibly better
  solved by Node's own extracted crypto implementation than by further
  Rust-side adaptation

Required outputs of the `ncrypto` spike:

- a checked-in failure inventory for the remaining `node:crypto` gaps
- a mapping from each gap to the current Deno-family implementation root
  (`deno_node_crypto`, `deno_crypto`, JS polyfill, or surrounding loader glue)
- a feasibility assessment for `ncrypto` integration from Rust, including:
  - C++/FFI boundary shape
  - CMake/Bazel build implications
  - OpenSSL/BoringSSL linkage posture
  - impact on the `agentstation/deno` fork and release process
- a written go/no-go recommendation:
  - keep fixing the existing Deno-family stack
  - adopt `ncrypto` for a narrow sub-area
  - or pursue broader `ncrypto` integration

Non-goal of the first spike:

- the first spike does **not** require productionizing `ncrypto`
- it is sufficient to produce a decision memo grounded in measured Neovex
  failures and source-level feasibility
- only if that memo recommends adoption should a follow-on implementation spike
  attempt a small proof-of-viability binding

Initial recommendation from the completed research spike:

- keep the existing Deno-family crypto stack as the default NLC7 path
- treat `ncrypto` as a targeted fidelity escalation option, not the primary
  implementation plan
- only revisit the default after NLC7 produces a checked-in `node:crypto`
  failure inventory showing that the remaining blockers cluster around
  behavior where running Node's extracted crypto implementation is plausibly
  lower-risk than continuing Rust-side adaptation

## Final Closeout Rule

This plan is only complete when all of the following are true:

- the generated compatibility artifacts exist and are reproducible
- every built-in module in the Node22 contract has a truthful support-state
  classification by runtime profile
- the Node20 lane is measured and documented as a compatibility lane, not left
  implicit
- upstream Node test slices are wired into CI or documented as an explicit
  remaining blocker
- package and framework canaries are checked in, version-pinned, and runnable
- public docs use the same support-state vocabulary as the generated matrix
- any remaining unsupported modules or profile-scoped restrictions are stated
  explicitly enough that an enterprise buyer could understand the contract
  without reading the source

## Execution Log

| Date | Item | Status | Files touched | Description | Verification |
|------|------|--------|---------------|-------------|--------------|
| 2026-04-29 | NLC0 | `done` | `docs/plans/node-lts-compatibility-plan.md`, `docs/plans/README.md`, `AGENTS.md` | Activated the new Node LTS control plane, promoted it into the active plan index, pointed AGENTS at it as the active owner for Node built-in compatibility work, and checkpointed the current dirty-tree runtime baseline plus the first resumable seam: generated compatibility artifacts and matrix tooling. | `git diff --check -- AGENTS.md docs/plans/README.md docs/plans/node-lts-compatibility-plan.md` |
| 2026-04-29 | — | `done` | `docs/plans/node-lts-compatibility-plan.md` | Clarified the control-plane contract after review: added canonical locations for generated artifacts and canaries, introduced a profile-aware support-state taxonomy, tightened NLC1/NLC2 expectations, and added an explicit final closeout rule so future agents cannot overclaim Node22 compatibility while host-sensitive APIs remain tooling-scoped. | `git diff --check -- docs/plans/node-lts-compatibility-plan.md AGENTS.md docs/plans/README.md` |
| 2026-04-29 | — | `done` | `docs/plans/node-lts-compatibility-plan.md` | Added a measured `nodejs/ncrypto` decision gate under NLC7: documented the current Deno-family crypto baseline, limited `ncrypto` to an evidence-backed crypto-family spike, and recorded the required go/no-go outputs before any broader crypto-stack replacement can be justified. | `git diff --check -- docs/plans/node-lts-compatibility-plan.md` |
| 2026-04-29 | — | `done` | `docs/plans/node-lts-compatibility-plan.md` | Completed the first crypto research spike: verified that the current Deno-family baseline already uses a substantial `deno_node_crypto` + `deno_crypto` stack backed by `aws-lc-rs` and many Rust crypto crates, confirmed `nodejs/ncrypto` is a real Node-owned fidelity candidate, and recorded the current recommendation to keep the Deno-family stack as the default until measured NLC7 failures justify a narrower or broader `ncrypto` move. | `git diff --check -- docs/plans/node-lts-compatibility-plan.md` |

## External Truth Sources

- Node.js 20 docs: `https://nodejs.org/docs/latest-v20.x/api/`
- Node.js 22 docs: `https://nodejs.org/docs/latest-v22.x/api/`
- Node.js 20 `process` docs:
  `https://nodejs.org/download/release/latest-v20.x/docs/api/process.html`
- Node.js 22 `process` docs:
  `https://nodejs.org/download/release/latest-v22.x/docs/api/process.html`
- Node.js 20 `module` docs:
  `https://nodejs.org/download/release/latest-v20.x/docs/api/module.html`
- Node.js 22 `module` docs:
  `https://nodejs.org/download/release/latest-v22.x/docs/api/module.html`
- Node.js 22 `fs` docs:
  `https://nodejs.org/download/release/latest-v22.x/docs/api/fs.html`
- Node.js 22 `sqlite` docs:
  `https://nodejs.org/download/release/latest-v22.x/docs/api/sqlite.html`
- Node.js 22 `test` docs:
  `https://nodejs.org/download/release/latest-v22.x/docs/api/test.html`
- Node.js v22.12 release notes:
  `https://nodejs.org/en/blog/release/v22.12.0`
- Node.js v20 changelog:
  `https://github.com/nodejs/node/blob/main/doc/changelogs/CHANGELOG_V20.md`
- Node.js v22 changelog:
  `https://github.com/nodejs/node/blob/main/doc/changelogs/CHANGELOG_V22.md`
- Node.js test suite:
  `https://github.com/nodejs/node/tree/main/test`
- Deno Node compatibility table:
  `https://docs.deno.com/runtime/reference/node_apis/`
- Deno Node API docs index:
  `https://docs.deno.com/api/node/`
- Deno `ext/node` source:
  `https://github.com/denoland/deno/tree/main/ext/node`
- LLRT API reference:
  `https://github.com/awslabs/llrt/blob/main/API.md`
- LLRT module implementations:
  `https://github.com/awslabs/llrt/tree/main/llrt_modules`
