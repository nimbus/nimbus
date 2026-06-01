# Node Default Runtime Support Hardening Plan (NDS)

Status: `active`
Owner: `node-compat`
Verifier: `scripts/verify-node-default-runtime-support-hardening.sh` (scaffolded in NDS0)
Baseline proof: `docs/plans/proof/node-default-runtime-support-hardening/nds0-baseline.md`

## Why this plan exists

The completed NFRC plan made Nimbus honest about Node compatibility: Node24 is
the product default, Node22 is a supported LTS peer, Node20 is legacy-grace
only, Node26 is Current/non-LTS, all official fixture corpora are classified,
and the Deno-style public docs distinguish supported, diagnostic-only, and
unsupported behavior.

That is necessary, but it is not enough to inspire trust in the default runtime.
The current dashboard says:

| Lane | Role | Upstream | Official fixtures passed | Known gap / expected failure | Skipped | Full-corpus pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| `node20` | legacy | `v20.20.2` | 893 / 1308 | 407 | 8 | 68.3% |
| `node22` | supported | `v22.22.3` | 1023 / 4773 | 3730 | 20 | 21.4% |
| `node24` | default | `v24.16.0` | 892 / 5198 | 4256 | 50 | 17.2% |
| `node26` | current | `v26.2.0` | 0 / 5578 | 5529 | 49 | 0.0% |

The package canary matrix is useful and real, but the default lane still has a
large official-fixture gap and Node26 has no official fixture passes yet. A
default runtime should be able to show more than "the packages we selected pass";
it should show broad, repeatable, lane-local support evidence and an explicit
denominator for what "well-supported" means.

NDS0 and NDS1 preserve the historical `1002 / 5198` Node24 source-topology
baseline for auditability. NDS3 corrected the evidence generator so the current
dashboard counts only matching-lane non-ignored Rust tests that execute official
fixtures; ignored watchpoints and classified gaps are not pass claims.

This plan raises the bar from **bounded V8-isolate compatibility** to a
**well-supported Node24 default** for the V8 isolate runtime. Classification is
allowed as diagnosis, but classification alone is not completion. Closeout
requires raising measured compatibility coverage.

## Decision

Nimbus keeps Node24 as the product default while this plan runs, but public docs
must not imply that product default means broad Node parity. This plan introduces
a new default-support gate. Node24 earns "well-supported default" only when the
new support posture artifacts and verifier prove it.

This plan is about Node compatibility inside Nimbus's V8 isolate runtime. If
implementation work shows a gate requires lower-level engine work, the plan
continues through the correct implementation path: Nimbus runtime fix,
`nimbus/deno` fork fix, `rusty_v8` fix, or a follow-up engine plan with the
failing fixtures preserved as active blockers. If a fixture requires real OS
process ownership, package-owned binaries, native addons, or other behavior
that cannot honestly run inside the isolate, this plan records a fail-closed
diagnostic and a non-isolate classification. It stays inside the isolate
runtime. Public docs must stay truthful while work is in progress, but truthful
wording is not a substitute for completing the compatibility raise required by
this plan.

## Guiding Strategy

Use wide tests to learn, focused tests to fix, then wide tests to prove. This is
the controlling execution strategy for every NDS row.

1. Vendor or enable the broadest relevant Node fixture, package, or Convex app
   group first.
2. Run the wide group before making fixes and write the complete failure
   inventory.
3. Group failures by root cause and fix or classify one cluster at a time with
   isolated tests.
4. When isolated tests pass, rerun the same wide group and compare the before
   and after counts.
5. Close the row only when the wide rerun shows the expected pass-count
   increase, required pass rate, or verified fail-closed diagnostic for
   non-isolate behavior.
6. Never improve a support percentage by hiding a failure in a vague bucket.
7. Never spend multiple loops on isolated passing tests without returning to the
   wide inventory that selected the work.

This is especially important for Node compatibility. Small green tests can look
good while the official corpus remains mostly classified as gaps. The control
plane must force the larger picture back into view on every row.

NDS3 also had an upstream-alignment pause gate after the `v2.8.0-nimbus.15`
checkpoint. That gate is now satisfied by
`docs/plans/deno-rusty-v8-upstream-alignment-plan.md` through DUA6:
`nimbus/deno` is published as `v2.8.1-nimbus.1`, `nimbus/rusty_v8` is consumed
as `v149.2.0-nimbus.1`, Nimbus is repinned to immutable tags, and the promoted
Node24 foundation groups have been rebaselined against that stack. NDS3 should
resume from this upstream-aligned baseline after DUA7/DUA8 close the handoff,
not from the stale `v2.8.0-nimbus.15` fork.

## Proof Contract

Every NDS row has required proof files. The proof files are not supporting
notes; they are the resume state and the audit trail for autonomous execution.

| Row | Required proof file |
| --- | --- |
| NDS0 | `docs/plans/proof/node-default-runtime-support-hardening/nds0-baseline.md` |
| NDS0 | `docs/plans/proof/node-default-runtime-support-hardening/nds0-control-plane.md` |
| NDS1 | `docs/plans/proof/node-default-runtime-support-hardening/nds1-posture-model-and-feasibility.md` |
| NDS2 | `docs/plans/proof/node-default-runtime-support-hardening/nds2-foundation-slices.md` |
| NDS3 | `docs/plans/proof/node-default-runtime-support-hardening/nds3-official-fixture-promotion.md` |
| NDS4 | `docs/plans/proof/node-default-runtime-support-hardening/nds4-node26-current-evidence.md` |
| NDS5 | `docs/plans/proof/node-default-runtime-support-hardening/nds5-package-canaries.md` |
| NDS6 | `docs/plans/proof/node-default-runtime-support-hardening/nds6-convex-app-suites.md` |
| NDS7 | `docs/plans/proof/node-default-runtime-support-hardening/nds7-permissions-and-shim-audit.md` |
| NDS8 | `docs/plans/proof/node-default-runtime-support-hardening/nds8-generated-docs.md` |
| NDS9 | `docs/plans/proof/node-default-runtime-support-hardening/nds9-ci-and-nightly-gates.md` |
| NDS10 | `docs/plans/proof/node-default-runtime-support-hardening/nds10-closeout.md` |

Each row proof must use this template:

1. **Row and status.** Name the row, status, date, branch, PR URL, and
   verifier version.
2. **Broad pre-run.** Record the widest relevant command, exact lane/package
   scope, pass/fail/skipped counts, output artifact paths, and failure
   inventory.
3. **Failure grouping.** Group every failure by root cause, classification,
   owner repository, and intended fix path.
4. **Focused work.** Record focused tests, fixtures, code/docs changed, and
   classifications or diagnostics added.
5. **Broad final rerun.** Rerun the same wide group and compare before/after
   counts. A row cannot close on focused tests alone.
6. **Evidence links.** Link generated dashboards, registry rows, fixture
   reports, shim inventory entries, docs anchors, and CI runs.
7. **Residual risks.** Name remaining gaps and prove they are allowed by this
   plan's denominators rather than hidden regressions.

## Canonical Foundation Slice Set

When this plan says "foundation slices," it means this exact five-slice
denominator from the currently manifested official fixture set:

| Family | Slice |
| --- | --- |
| `core-semantics` | `assert-and-buffer-foundation` |
| `process-and-timing` | `process-foundation` |
| `streams-and-local-io` | `os-tty-readline-foundation` |
| `networking` | `dns-net-foundation` |
| `loader-context` | `module-and-async-foundation` |

NDS0 must record this denominator in the baseline proof. NDS2 must run all five
slice groups across Node22 and Node24 before and after fixes; the process and
loader-context rows are called out because they were the historical red cron
cells, not because the other three slices are optional.

## Compatibility Ambition And Shim Policy

The Node24 `2000` full-corpus pass gate is a minimum closeout threshold, not the
target. The target is the highest practical compatibility for the V8 isolate
runtime: 100% of the V8-isolate-required surface and continued promotion of
V8-isolate-optional fixtures whenever the behavior can be truthful, tested, and
least-authority.

Compatibility shims are welcome when they faithfully model observable Node
behavior inside the isolate and are backed by official fixtures or package
canaries. Good shim examples include:

- `process.version`, `process.versions`, `process.platform`, and
  `process.arch`.
- `process.env` from explicitly configured Nimbus environment variables.
- `process.cwd()` returning the virtual app root.
- Read-only `os` facts such as `os.type()`, `os.platform()`,
  `os.release()`, constrained CPU information, and constrained memory
  information.
- `os.tmpdir()` when backed by an explicit virtual or ephemeral temp area.
- Event loop and JavaScript-level behavior such as `process.nextTick`, timers,
  buffers, streams, crypto, URL, and Web APIs.

Emulation is also acceptable when the capability is intentionally implemented
with isolate-compatible machinery and the docs describe what is and is not
real. For example, a test harness may emulate `process.execPath` respawn through
another Nimbus runtime invocation, or a runtime may emulate selected cluster or
worker lifecycle events with worker-thread machinery. Those can be useful and
trustworthy only when they are explicitly classified as emulated, tested against
their claimed semantics, and excluded from claims that require real OS process,
signal, fd, port, or native-addon ownership.

Diagnostic stubs are welcome when the API cannot honestly work inside the V8
isolate. They must fail closed, explain the unsupported boundary, and stay out
of positive support counts. Diagnostic examples include:

- `child_process.spawn()` / `exec()` when no child process is launched.
- Filesystem writes that claim durability outside an explicitly configured
  virtual or ephemeral filesystem.
- `net.Server.listen()` when no port is actually bound.
- `cluster.fork()` when no worker process exists.
- Native addon loading when native code is not executed.
- `process.kill()` or signal APIs when Nimbus cannot provide the requested
  process-control semantics.

Fake-success stubs are forbidden. A migration-friendly runtime may avoid
crashing through truthful shims, but it must not tell an application that a
binary ran, a file persisted, a port opened, a worker forked, or native code
loaded when that side effect did not happen. Enterprise trust comes from
faithful compatibility where possible and early, specific diagnostics where it
is not possible.

This plan must create a checked-in shim and emulation inventory covering both
the `nimbus/nimbus` runtime/test harness and the `nimbus/deno` fork. Each entry
must classify the surface as one of:

- `native_isolate`: implemented directly by the V8/Deno substrate.
- `compatibility_shim`: faithful isolate-safe compatibility behavior.
- `isolate_emulation`: emulated behavior that performs the documented side
  effects inside the isolate.
- `test_harness_emulation`: fixture-only emulation used to measure another
  surface and never counted as user-facing runtime support.
- `diagnostic_stub`: import or API shape exists only to fail closed with a
  clear diagnostic.
- `unsupported`: no import, call, or compatibility claim.

Each entry must name the source location, affected Node lanes, claimed
capability, side effects actually performed, side effects not performed,
evidence paths, public documentation anchor, and owner repository
(`nimbus/nimbus` or `nimbus/deno`). Prefer a Rust-owned registry or typed
annotation in `nimbus-runtime` for Nimbus-owned shims, plus concise source
comments near the Rust/JS implementation. For fork-owned behavior, use a
checked-in audit record that points at exact `~/src/github.com/nimbus/deno`
files and records whether the fix should remain in the fork or move upstream.

## Denominator Rubric And Feasibility Checkpoint

NDS1 must turn the default-support denominator into checked data before any
large greening row proceeds. The classification is not a free-form judgment
call. Every official fixture row must carry exactly one support denominator and
one reason from a schema-controlled vocabulary:

- `v8_isolate_required`: public Node API behavior that can be implemented
  faithfully inside the Nimbus V8 isolate without claiming host process, port,
  fd, native-addon, or durable filesystem ownership. Examples include module
  loading, URL, buffer, assert, events, util, timers, Web APIs, JavaScript
  crypto/WebCrypto, stream semantics, process metadata that Nimbus can truthfully
  provide, and explicitly virtualized environment/temp behavior.
- `v8_isolate_optional`: isolate-safe behavior that is useful but not required
  for the default FaaS contract in this wave. Optional fixtures remain visible
  and are promoted whenever a truthful fix is available.
- `diagnostic_only_non_isolate`: APIs that require host-owned side effects such
  as child processes, raw listening sockets, signals, native addons, package
  binaries, pseudo terminals, or durable host filesystem mutation. These require
  fail-closed diagnostics and never count as positive V8-isolate support.
- `test_harness_only`: upstream tests that require Node's own test runner,
  pummel/stress harnesses, pseudo terminal harnesses, WPT harnesses, or
  sequential host-state orchestration rather than runtime API support.
- `upstream_or_platform_boundary`: fixtures blocked by upstream bugs,
  version-specific removals, platform-only behavior, or unsupported host
  platforms, with a linked source.

The rubric must cross-check public docs: if docs claim an API/package is
supported for Application runtimes, fixture rows for that API cannot be placed
outside `v8_isolate_required` without a proof explaining why the fixture tests a
different host-owned behavior than the documented claim.

NDS1 must also produce an honest feasibility checkpoint for the `2000` Node24
full-corpus pass gate. The checkpoint estimates the maximum reachable pass count
in this wave from:

- current passed fixtures,
- fixtures newly classified as `v8_isolate_required`,
- fixtures newly classified as promotable `v8_isolate_optional`, and
- fixtures blocked by `nimbus/deno` or `rusty_v8` owner work that this plan can
  realistically land through the fork publish/repin flow.

If the checkpoint proves that `2000` cannot be reached truthfully inside this
plan, the agent must not silently lower the target or reclassify failures to
close the plan. It must mark the goal blocked, preserve the exact fixture list,
and create or update a follow-up engine/fork plan whose completion remains a
blocker for NDS closeout. The NDS plan closes only when the measured targets are
met.

NDS3 may re-enter the same blocked path if implementation disproves the NDS1
estimate. A broad/focused greening loop that proves the remaining `2000` gap
requires engine work outside this plan must stop in a documented blocked state
instead of grinding indefinitely or weakening the denominator.

## Application Canary Category Rubric

NDS5 must add a schema-controlled `compat_category` field to the package canary
registry. The initial vocabulary is:

- `ai-sdk`
- `http-client`
- `http-framework`
- `auth-jwt`
- `validation`
- `payments`
- `email`
- `object-storage`
- `database-http`
- `observability`
- `webhooks-signing`
- `loader-edge`
- `request-response-adapter`
- `convex-use-node`
- `runtime-builtins`

A canary may have one `compat_family` and many `canary_surfaces`, but exactly
one `compat_category`. Adding a new category is allowed only by updating the
schema, docs generation, and NDS5 proof; otherwise category-count support would
be too easy to inflate.

## Transferred Lessons

These lessons are carried forward from the NLRT/NFRC work and are requirements
for this plan:

- **Product default is not evidence priority.** Node24 can be the routing
  default only because the registry says so; it earns stronger support language
  only through lane-local evidence.
- **Node lanes are compatibility contracts.** Node22, Node24, and Node26 are
  profiles on the current `v8_deno_core` engine unless a separate new-engine
  proof says otherwise. Do not imply embedded official Node or `libnode`.
- **Official fixture counts and app canaries answer different questions.**
  Package canaries prove realistic developer workflows; official fixtures prove
  breadth. NDS needs both, with separate denominators.
- **Diagnostic evidence is not positive support.** The NFRC dashboard had to
  add `evidence_kind` and `support_status` so `Passed` diagnostics for
  `child_process`, native addons, raw listen, and package-owned binaries could
  not be mistaken for in-process support.
- **Broad canary harnesses must report every failure.** NFRC8 initially stopped
  at the first SDK failure; NDS harnesses must collect all package/fixture
  failures in a lane so one long run produces a useful inventory.
- **Real SDKs need least-authority fixes, not broad grants.** NFRC8 kept
  `os.homedir()` denied, added only read-only `os.release()`, and configured
  AWS explicitly. NDS must prefer package-specific configuration and narrow
  runtime facts over ambient filesystem or host authority.
- **Convex app evidence must cross runtime lanes.** NFRC7 found a real bug
  where nested `ctx.runAction` could run a callee under the default web lane.
  NDS Convex suites must prove callee-lane selection, nested `ctx.run*`, and
  scheduler behavior, not just package import.
- **Deterministic local mocks are useful only when they model real client
  behavior.** Mock base URLs, pipeline response shapes, user-agent paths, and
  auth headers must match the pinned SDKs closely enough to catch runtime
  integration bugs without live third-party credentials.
- **Watchpoints stay visible.** Ignored fatal, VM, or intentional-divergence
  tests are pinned watchpoints with catalog entries and unexpected-pass checks;
  they are never quiet skips or support evidence.
- **Official release feeds beat snippets.** Release-train metadata follows the
  official Node dist index and Release Working Group schedule JSON. Search
  snippets are advisory and cannot override machine-readable official feeds.
- **Generated docs are the public contract.** Numbers, package support, API
  support, and non-isolate boundaries must be generated from evidence or
  guarded against stale prose.
- **Local sandbox failures are not runtime failures.** Local canaries that bind
  loopback may require approved local bind/listen execution; proofs must record
  that distinction and still rerun the same broad command successfully.

## Non-Escape Rules

- A known-gap classification is an issue inventory item, not a done state.
- `Requires Unpromoted Node Surface` is not allowed in the final Node24 default
  posture.
- Non-isolate host behavior never counts as V8 isolate support. It must have a
  fail-closed diagnostic and explicit non-isolate classification. Any future
  host-capable execution backend belongs in a separate plan.
- Truthful compatibility shims are encouraged when they implement real
  V8-isolate semantics. Fake-success stubs are forbidden.
- Emulated surfaces are allowed only when the shim inventory and user-facing
  docs state the capability and limits plainly.
- No Node compatibility shim, emulation, or diagnostic stub may remain
  unclassified across `nimbus/nimbus` or the `nimbus/deno` fork.
- Truthful interim docs are required while work is in progress, but wording
  changes cannot satisfy the final verifier.
- A hard fixture may move to a fork or engine follow-up only with a proof file
  that names the exact fixtures, owner repository, required change, and the
  verifier condition that remains blocked until it lands.
- The plan closes only after the measured coverage targets are met.

## Definition Of Well-Supported Default

Node24 is a well-supported default only when all of these are true:

1. **Default support posture exists.**
   `docs/architecture/runtime/node-default-support-posture.json` and `.md`
   separate at least these denominators:
   - full official Node fixture corpus
   - V8-isolate-required official fixtures
   - V8-isolate-optional official fixtures
   - diagnostic-only non-isolate fixtures
   - test-harness-only fixtures
   - upstream/platform boundary fixtures
2. **No vague default denominator remains.**
   Node24 has zero `Requires Unpromoted Node Surface` entries. Every former
   unpromoted entry is either passed, V8-isolate-required gap,
   V8-isolate-optional gap, diagnostic-only non-isolate, or
   test-harness-only/upstream-platform boundary with a schema-controlled reason.
3. **V8-isolate-required fixtures are green.**
   Node24 and Node22 pass 100% of the V8-isolate-required official fixture set.
   Node26 runs the same set where the Current line still exposes the same API,
   with pass/fail evidence instead of blanket known-gap classification.
4. **Foundation slices are green.**
   The currently manifested foundation slices pass on Node22 and Node24 with no
   unexpected failures and no silent quarantines. Any intentional divergence has
   an ignored watchpoint, a failure-inventory entry, and a public explanation.
5. **Full-corpus support visibly improves.**
   Node24 full-corpus official fixture pass count increases from 1002 to at
   least 2000. This is a minimum closeout gate, not an aspirational target.
   Node22 must remain within 5 percentage points of the Node24 full-corpus pass
   rate unless a version-specific upstream difference is proven. The closeout
   proof records the remaining V8-isolate-optional gap inventory and why each
   remaining gap cannot be promoted in this wave.
6. **Package evidence is broad enough for realistic apps.**
   Node22 and Node24 pass at least 50 positive Application package/framework
   claims across at least 12 schema-controlled `compat_category` values, with
   zero required canary gaps. The canary registry must distinguish
   `compat_family`, `compat_category`, and `canary_surfaces`; a broad `sdk` or
   `networking` family cannot satisfy multiple category counts by itself.
   Native, binary, child-process, raw-listen, and persistent-filesystem packages
   remain diagnostic-only non-isolate behavior and do not count as positive
   support.
7. **Convex apps are first-class evidence.**
   At least 5 real Convex-compatible `"use node"` app suites pass on Node22 and
   Node24, including package actions, nested `ctx.run*` calls, ESM/CJS package
   loading, scheduled/background action flow, and realistic SaaS SDK usage.
8. **Node26 is not a paper lane.**
   Node26 Current/non-LTS has real fixture and package evidence for the same
   default-support surface. It remains non-LTS, but the dashboard must not show
   0 official fixture passes for Node26 after this plan closes. Node26 must
   pass at least 1000 official fixtures and the same V8-isolate-required
   surface as Node24, unless an upstream Current-line removal is proven fixture
   by fixture.
9. **Docs match evidence.**
   Deno-style public docs show version-by-version API and package support using
   the new posture metrics. Interim docs may say "bounded V8-isolate-compatible
   default"; the plan closes only when docs can truthfully describe Node24 as
   the well-supported default using the verifier-backed metrics.
10. **Shim and emulation disclosure is complete.**
    A generated or verifier-checked shim inventory covers both `nimbus/nimbus`
    and `nimbus/deno`, records all compatibility shims, isolate emulations,
    test-harness-only emulations, diagnostic stubs, and unsupported surfaces,
    and links each user-facing claim to source annotations plus evidence.
11. **CI and nightly keep it true.**
    PR CI gates the Node24 default support posture, Node22 LTS parity, package
    canaries, and docs claims. Nightly runs the broad official fixture groups
    and Node26 Current evidence.

## In Scope

- Runtime builtins and bootstrap behavior needed to pass high-value official
  fixtures.
- Truthful compatibility shims for isolate-safe Node metadata, event-loop,
  module, and JavaScript-level behavior.
- Codebase and fork audit of compatibility shims, isolate emulations,
  test-harness-only emulations, diagnostic stubs, and unsupported Node surfaces
  across `nimbus/nimbus` and `~/src/github.com/nimbus/deno`.
- Source-level records for shims and emulations, preferably Rust-owned typed
  annotations or registry entries in `nimbus-runtime` plus concise comments near
  JS or fork-owned implementations.
- `nimbus/deno` fork fixes when the correct implementation belongs below
  Nimbus. Fork work follows the canonical unpin, prove, publish, repin flow.
- Official fixture classification schema and generated evidence dashboards.
- Node24 and Node22 V8-isolate-required fixture greening.
- Node26 Current fixture evidence for the same default-support surface.
- Package canary breadth, lockfiles, generated package references, and support
  docs.
- Real Convex-compatible `"use node"` app suites.
- Permission and non-isolate boundary diagnostics that keep unsupported host
  behavior explicit.
- CI and nightly gates for the new posture.

## Execution Mode And PR Contract

This plan is large enough that the execution surface is part of the control
plane. Do not execute NDS work directly on `main`.

1. **Dedicated worktree.** Create or reuse a dedicated worktree for this plan,
   preferably `../nimbus-worktrees/node-default-runtime-support-hardening`, on a
   `codex/node-default-runtime-support-hardening` branch. If a different path or
   branch is used, record it in `nds0-control-plane.md`.
2. **Draft PR as the public review surface.** Open a draft pull request before
   implementation work proceeds past NDS0. Keep the PR draft while any NDS row
   is pending. The PR description must link this plan, the active proof
   directory, the latest dashboard evidence, and the current verifier command.
3. **Row-owned proof files.** Every NDS row must write or update a proof file
   under `docs/plans/proof/node-default-runtime-support-hardening/` before the
   row can move to `done`. Each proof records broad pre-run command output,
   failure inventory, focused fixes or classifications, broad final rerun,
   files touched, and residual risks.
4. **Ledger is progress state.** The plan ledger and proof files are the resume
   protocol after compaction. A fresh agent resumes the first `in_progress` row,
   or the first `pending` row if none is in progress, after reading `AGENTS.md`,
   this plan, `nds0-control-plane.md`, and the draft PR.
5. **Verifier-first discipline.** NDS0 must create
   `scripts/verify-node-default-runtime-support-hardening.sh` and make it fail
   clearly on unimplemented gates. Later rows add checks before marking work
   done. The final verifier is the local completion gate; green GitHub Actions
   on the draft PR are the remote completion gate.
6. **Fork changes are published before Nimbus closeout.** Any `nimbus/deno`
   owner fix follows the canonical flow: temporarily unpin Nimbus to the local
   Deno worktree, prove the fix, commit/tag/push the Deno fork, repin Nimbus to
   the immutable tag, then rerun the relevant Nimbus verifier lanes against the
   tag.
7. **No quiet direct-to-main completion.** The PR becomes ready for review only
   after NDS0..NDS10 are `done`, the final verifier reports zero failed required
   checks, local required gates pass, and GitHub CI/coverage/nightly-relevant
   checks are green. Merge or direct `main` updates require explicit developer
   approval.

### Active Execution Pointer

This active plan is the main-visible pointer for in-flight NDS work. Before
implementation proceeds past NDS0, these fields must be filled either in this
section or in a linked `docs/plans/README.md` entry that is discoverable from
`origin/main`. If the fields are empty, a fresh agent must treat the plan as not
yet activated and must not create a duplicate worktree without first checking
GitHub PRs and the local worktree list.

The pointer is satisfied only when the chosen pointer artifact is committed and
visible from `origin/main`, unless the developer explicitly approves a fallback
that relies on the draft PR and local goal state. A narrow pointer-only update to
`main` is allowed only with explicit developer approval and is not plan
completion.

| Field | Value |
| --- | --- |
| Active worktree | `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening` |
| Active branch | `codex/node-default-runtime-support-hardening` |
| Draft PR | `https://github.com/nimbus/nimbus/pull/10` |
| Active goal objective | `019e7f94-cc55-7862-b9ec-be1103d7aea1` |
| Last completed row | `NDS2` |
| Current row | `NDS3` |
| Latest verifier output | `15 passed, 19 failed (NDS3 in progress after Node24 networking plus loader/context checkpoint; Node24 is 892 / 5198 and current-row/future-row gates fail as expected). DUA verifier after DUA6: 18 passed, 5 failed with only DUA7/DUA8 closeout gates remaining.` |

## Ledger Status Values

Rows use `pending`, `in_progress`, `done`, or `blocked`.

- `pending`: work has not started.
- `in_progress`: this is the row a fresh agent should resume first.
- `done`: row proof, ledger, Active Execution Pointer, and row verifier checks
  are complete.
- `blocked`: the row hit a verified blocker that cannot be resolved by more
  local work in this plan. A blocked row must name the exact fixture/package
  list, owner repository, follow-up plan, draft PR or issue, and verifier gate
  that remains unsatisfied. `blocked` is a valid autonomous terminal state for a
  `/goal`, but it is not NDS completion and the plan must not be archived.

## Goal Control Plane Objective

When this plan is activated as a goal, use this objective:

Complete `docs/plans/node-default-runtime-support-hardening-plan.md`
autonomously end to end in a dedicated worktree and draft PR. Success means
Nimbus raises Node24 from bounded V8-isolate-compatible default to
verifier-backed well-supported default, keeps Node22 as a supported LTS peer
with comparable evidence, gives Node26 real Current-line fixture evidence,
expands positive Application package evidence to at least 50 claims across at
least 12 schema-controlled `compat_category` values, proves at least 5
realistic Convex-compatible `"use node"` app suites, preserves fail-closed
diagnostics for non-isolate behavior, uses truthful compatibility shims where
they improve V8-isolate compatibility, documents emulated capabilities and their
limits, audits `nimbus/nimbus` and `nimbus/deno` for every shim/emulation/stub,
rejects fake-success stubs, regenerates Deno-style docs from the new posture,
wires PR/nightly gates, and passes
`bash scripts/verify-node-default-runtime-support-hardening.sh` with zero failed
required checks. Execution must follow the wide-then-focused loop: run broad
vendored corpora first to capture failure inventory, fix or classify clustered
failures with isolated tests, then rerun the same broad groups and close rows
only on measured coverage gains or verified non-isolate diagnostics. Each NDS
row must update its required proof file, the plan ledger, and the Active
Execution Pointer before handoff, and the draft PR must remain the remote
review/CI control surface until final closeout.

A separate valid terminal state is `blocked`: if NDS1's feasibility checkpoint
or NDS3 implementation evidence proves the Node24 `2000` full-corpus pass gate
is unreachable truthfully inside the V8 isolate/runtime/fork scope of this plan,
stop in a documented blocked state with the exact fixture list preserved, a
follow-up engine or fork plan created, the blocker recorded in the ledger and
Active Execution Pointer, and the verifier gate left unsatisfied. Do not loop on
an unsatisfiable green-verifier objective, and do not lower the target without a
developer-approved plan revision.

Before setting the goal, create the worktree/branch or record the existing one,
open the draft PR or record the intended PR bootstrap step in NDS0, and include
the worktree path plus PR URL in `nds0-control-plane.md` and the Active
Execution Pointer.

## Out Of Scope

- Claiming full Node CLI parity for the in-process runtime.
- Expanding this plan beyond V8 isolate runtime semantics.
- Counting diagnostic-only non-isolate behavior as in-process package support.
- Adding fake-success stubs that claim unsupported OS side effects happened.
- Landing undocumented shims or emulations in `nimbus/nimbus` or `nimbus/deno`.
- Hiding unsupported Node behavior behind import-compatible stubs without a
  diagnostic test.
- Promoting Node26 to enterprise LTS before upstream Node26 enters LTS and the
  supported-LTS gates pass.
- Lowering the Node24 default standard because the current engine makes a
  fixture hard.

## Ledger

| NDS | Work | Verifiable success criteria | Status |
| --- | --- | --- | --- |
| NDS0 | Baseline, verifier scaffold, and execution control plane. Capture current Node20/22/24/26 fixture pass rates, package canaries, non-isolate diagnostics, Node26 0-pass posture and cause, transferred lessons, and the NFRC boundary. Mark the older cron-greening plan as subsumed. Create the worktree/branch/PR control surface before implementation rows proceed. | `nds0-baseline.md` and `nds0-control-plane.md` exist; `nds0-control-plane.md` records worktree path, branch, draft PR URL or approved bootstrap substitute, current goal objective, resume protocol, and Deno fork publish/repin protocol; the Active Execution Pointer is updated and made visible from `origin/main` or the proof records the developer-approved fallback; verifier script exists and fails on every unimplemented gate; baseline records Node24 `1002 / 5198`, Node22 `1000 / 4748`, Node26 `0 / 5578`, why Node26 currently has zero official passes, the canonical five foundation slices, package/framework canary claims `37`, package/framework canary checks `101`, diagnostic canary claims `11`, required canary gaps `0`, registry split `32` Application / `5` Tooling, the wide-then-focused rule, and the transferred lessons above; docs refs and `git diff --check` pass. | done |
| NDS1 | Default-support posture model and feasibility checkpoint. Build JSON/Markdown plus schema for full corpus, V8-isolate-required, V8-isolate-optional, diagnostic-only non-isolate, test-harness-only, and upstream/platform denominators. Prove whether the Node24 `2000` pass gate is truthfully reachable inside this plan before large greening work proceeds. | `nds1-posture-model-and-feasibility.md` exists; posture generator validates schema; Node24 has zero `Requires Unpromoted Node Surface`; every moved fixture has denominator, schema-controlled reason, evidence path, public-doc cross-check, and shim/emulation classification where relevant; status dashboard reports full-corpus, V8-isolate-required, and V8-isolate-optional metrics separately; feasibility checkpoint names the reachable Node24 pass ceiling and exact blockers if below `2000`; if the ceiling is below `2000`, the goal is marked blocked instead of rebaselining the target; wide pre/post inventories are recorded. | done |
| NDS2 | Foundation-slice greening. Complete the useful cron-greening work inside this plan: lane-aware process metadata and module/async loader-context fixes inherited from archived NCG. | `nds2-foundation-slices.md` exists; broad runs for all five canonical foundation slices across Node22/Node24 are captured before fixes; the proof explicitly covers `process-and-timing:process-foundation` × Node24 `test/parallel/test-process-features.js`; the proof enumerates the 10 `loader-context:module-and-async-foundation` fixtures from NCG and names the 4 failing fixtures from local JSON reports; each failing fixture is classified as `bootstrap-shim`, `runtime-op`, `fork-bump`, or `explicit-divergence`; focused tests fix or justify each fixture; final broad rerun is green for all five canonical foundation slices on Node22/Node24; any intentional divergence has an ignored watchpoint plus failure-inventory entry; no silent quarantine. | done |
| NDS3 | High-value official fixture promotion. Raise Node24/Node22 support by clusters: module/loader, assert/buffer, events/util, URL/querystring, streams, timers/AbortController, crypto/WebCrypto, DNS/TLS/client networking, selected `fs/promises`, process metadata, and diagnostics_channel. | Each cluster proof has initial broad failure list, focused fixes, and final broad rerun; Node24 full-corpus pass count reaches at least 2000; Node22 stays within 5 percentage points of Node24 or has proven version-specific upstream deltas; V8-isolate-required pass rate is 100% on Node22 and Node24; remaining V8-isolate-optional gaps are inventoried instead of treated as the target; if implementation disproves the NDS1 feasibility estimate, the row moves to `blocked` with exact fixtures, owner repo, and follow-up engine/fork plan instead of looping or weakening the target. | in_progress |
| NDS4 | Node26 Current evidence. Run the same foundation and V8-isolate-required fixture sets against Node26 and fix current-line metadata/bootstrap drift. | `nds4-node26-current-evidence.md` exists; proof explains the NDS0 zero-pass cause and the exact manifest/classification/harness changes that made Node26 runnable; Node26 official fixture pass count reaches at least 1000; Node26 passes the V8-isolate-required surface shared with Node24 except fixture-by-fixture proven upstream removals; Node26 no longer blanket-classifies the default-support surface as known gap; Node26 package and fixture docs show observed Current/non-LTS evidence separately from LTS support; final broad Node26 run is recorded. | pending |
| NDS5 | Package and framework canary expansion. Add positive Application canaries for realistic app packages across AI, HTTP, auth/JWT, validation, payments, email, object storage, HTTP database clients, observability, webhooks/signing, loader edge cases, and request/response adapters. | `nds5-package-canaries.md` exists; canary registry schema has `compat_family`, exactly one schema-controlled `compat_category` per claim, and `canary_surfaces`; at least 50 positive Application claims pass on Node22 and Node24 across at least 12 distinct `compat_category` values; the harness reports all failures in a lane; required canary gaps are 0; Node26 observations are recorded separately; deterministic mocks model real SDK paths; diagnostic-only non-isolate packages are excluded from positive counts. | pending |
| NDS6 | Real Convex app suites. Add realistic Convex-compatible `"use node"` app suites beyond single canary actions. | At least 5 app suites pass on Node22 and Node24; suites cover package actions, callee-lane selection for nested runtime calls, `ctx.runQuery`/`ctx.runMutation`/intended `ctx.runAction`, generated APIs, scheduled/background action flow, ESM/CJS/conditional exports, value serialization, and SaaS SDK usage; Convex guidelines are followed. | pending |
| NDS7 | Permission and non-isolate boundary plus shim audit. Keep unsupported OS-owned behavior explicit while expanding support, and audit every compatibility shim/emulation/stub in `nimbus/nimbus` plus the `nimbus/deno` fork. | Child process, worker threads, raw listen, native addons, package-owned binaries, persistent filesystem assumptions, and CLI/test-runner surfaces fail closed with useful diagnostics inside the V8 isolate runtime unless explicitly classified as truthful shim or documented emulation; diagnostics pass on Node22/Node24/Node26; diagnostics carry `evidence_kind=diagnostic` or equivalent and are not counted as positive support; tests prove fake-success stubs are rejected; `node-isolate-shim-inventory` or equivalent records source locations, capability limits, evidence, documentation anchors, and owner repo for every shim/emulation/stub. | pending |
| NDS8 | Deno-style docs from posture and shim inventory. Regenerate public compatibility, API, package, evidence, and shim/emulation docs from the new posture. | Docs show per-version full-corpus, V8-isolate-required, package, non-isolate diagnostic metrics, and explicit "native", "shimmed", "emulated", "test-harness-only", and "unsupported" classifications; Node24 says well-supported default only after gates pass; Node26 is Current/non-LTS with real evidence; support numbers and shim claims are generated or guarded against stale prose; `make node-compat-publish-docs CHECK=1`, docs guard, and strict docs refs pass. | pending |
| NDS9 | PR and nightly gates. Keep the raised support true over time. | PR CI includes the default-support verifier, Node24 posture, Node22 parity, package canaries, docs claims, and non-isolate diagnostics; nightly includes broad official fixture groups, release-train drift from official Node feeds, latest-suite drift, watchpoint validation, Node26 Current evidence, and posture trends; structural verifier proves the workflow wiring. | pending |
| NDS10 | Closeout and archive. Finish all rows and prove the final state. | `nds10-closeout.md` exists; every row is `done`; execution log records commands and counts; final verifier prints zero failed required checks and an explicit pass count for all checks currently defined by the verifier; generated docs are current; `cargo fmt --all --check`, strict docs refs, and `git diff --check` pass; draft PR checks are green and the PR is ready for review; plan moves to archive and routing points to the archived baseline after merge approval. | pending |

## Completion Gate

`bash scripts/verify-node-default-runtime-support-hardening.sh` exits 0 with a
summary line that includes `0 failed` and the actual number of required checks
passed. The verifier must check at least:

1. Plan is active or archived and every ledger row is `done` at closeout.
2. Baseline proof exists and records the current low Node24/Node26 posture.
3. Control-plane proof and Active Execution Pointer record the dedicated
   worktree, branch, draft PR URL or approved substitute, active goal objective,
   resume protocol, Deno fork publish/repin protocol, and main-visible pointer
   path or developer-approved fallback.
4. Default-support posture JSON and Markdown exist and validate against schema.
5. NDS1 feasibility proof records the Node24 `2000` pass-gate ceiling and does
   not allow target lowering without a blocked goal plus follow-up engine/fork
   plan.
6. The denominator schema rejects fixture rows without exactly one support
   denominator, schema-controlled reason, evidence path, and docs cross-check.
7. Public support docs and posture rows agree on which APIs/packages are
   V8-isolate-required.
8. Node24 has zero `Requires Unpromoted Node Surface` entries.
9. Node24 and Node22 V8-isolate-required official fixture pass rate is 100%.
10. Node22 full-corpus pass rate remains within 5 percentage points of Node24
   or the proof records version-specific upstream deltas.
11. Node26 has at least 1000 official fixture passes, passes the shared
   V8-isolate-required surface, and has no blanket known-gap treatment for the
   default-support surface.
12. All five canonical foundation slices pass on Node22 and Node24.
13. NDS2 proof names the Node24 `test-process-features.js` failure, the 10
   module-and-async foundation fixtures, the 4 failing loader-context fixtures,
   and each fixture's required classification.
14. Node24 full-corpus official pass count is at least 2000.
15. Package registry schema includes `compat_family`, exactly one
   schema-controlled `compat_category` per claim, and `canary_surfaces`.
16. Positive Application package claims are at least 50 across at least 12
   distinct `compat_category` values on Node22 and Node24.
17. Required Application package canary gaps are 0.
18. At least 5 Convex-compatible real app suites pass on Node22 and Node24.
19. Non-isolate diagnostics pass and are excluded from positive support counts.
20. Generated public docs match checked-in posture and evidence.
21. Package reference contains per-version support, not only aggregate support.
22. API reference contains per-version support and non-isolate boundaries.
23. Shim/emulation inventory covers `nimbus/nimbus` and `nimbus/deno`, has no
    unclassified Node compatibility surfaces, and records source annotations or
    source comments for Nimbus-owned shims where practical.
24. User-facing docs disclose native, shimmed, emulated, test-harness-only,
    diagnostic, and unsupported capability classes.
25. Release-train and latest-suite drift checks pass.
26. PR CI includes the new default-support gate.
27. Nightly workflow includes broad fixture, package, and Node26 Current lanes.
28. `cargo fmt --all --check`, strict docs refs, and `git diff --check` pass.
29. Every required row proof file exists and follows the Proof Contract
    template.
30. Every NDS row proof records the wide-then-focused loop: broad pre-run,
    failure inventory, focused fixes/classifications, and broad final rerun.
31. The verifier rejects diagnostic canaries counted as positive support, and
    the inventory plus targeted tests reject known fake-success stubs that claim
    unsupported OS side effects happened. This is a registry/test-backed gate,
    not a promise to prove arbitrary future source code statically.
32. The verifier rejects stale hand-written support numbers that disagree with
    generated evidence.
33. Closeout proof records green local verifier output, green draft PR checks,
    and the explicit approval path used before merge or direct `main` update.
34. If any row is `blocked`, the verifier rejects archive/closeout and requires
    exact blockers, owner repo, follow-up plan, Active Execution Pointer update,
    and unsatisfied verifier gates to be recorded.

## Execution Log

| Date | NDS | Status | Files touched | Verification | Notes |
| --- | --- | --- | --- | --- | --- |
| 2026-06-01 | NDS0 | done | `docs/plans/node-default-runtime-support-hardening-plan.md`, `docs/plans/proof/node-default-runtime-support-hardening/nds0-baseline.md`, `docs/plans/proof/node-default-runtime-support-hardening/nds0-control-plane.md`, `scripts/verify-node-default-runtime-support-hardening.sh` | `bash scripts/verify-node-default-runtime-support-hardening.sh` -> `8 passed, 26 failed`; `git diff --check` pass; strict docs refs pass | Dedicated worktree/branch created; draft PR `https://github.com/nimbus/nimbus/pull/10` opened. |
| 2026-06-01 | NDS1 | done | `scripts/runtime/node/default_support_posture.py`, `tests/runtime/node/schemas/node-default-support-posture.schema.json`, `docs/architecture/runtime/node-default-support-posture.json`, `docs/architecture/runtime/node-default-support-posture.md`, `docs/plans/proof/node-default-runtime-support-hardening/nds1-posture-model-and-feasibility.md`, `scripts/verify-node-default-runtime-support-hardening.sh` | `python3 scripts/runtime/node/default_support_posture.py --check` pass; `bash scripts/verify-node-default-runtime-support-hardening.sh` -> `13 passed, 21 failed`; `git diff --check` pass; strict docs refs pass | Node24 feasibility ceiling estimated at `2849`; remaining unpromoted denominator count is `0` in the generated posture. |
| 2026-06-01 | NDS2 | done | `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`, `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`, `docs/plans/proof/node-default-runtime-support-hardening/nds2-foundation-slices.md`, `docs/plans/node-default-runtime-support-hardening-plan.md` | five baseline `make node-compat-report` runs under `target/node-compat-nds2/baseline`; focused `assert-and-buffer-foundation` and `process-foundation` reruns under `target/node-compat-nds2/focused`; descriptor-focused `process-foundation` rerun under `target/node-compat-nds2/descriptor-audit`; five final `make node-compat-report` runs under `target/node-compat-nds2/final-after-descriptor-audit`; `bash scripts/verify-node-default-runtime-support-hardening.sh` -> `15 passed, 19 failed` | Closed current foundation failures without quarantine: `test-assert-checktag.js` now passes via Node-shaped `Symbol.toStringTag`, and `test-process-features.js` now passes via lane-aware, descriptor-matched `process.features`. Current loader-context local JSON reports are green; archived NCG did not preserve the historical four failure names, so the proof records that absence rather than fabricating names. |
| 2026-06-01 | NDS3 | in_progress | `Cargo.toml`, `Cargo.lock`, `crates/nimbus-runtime/src/module_loader/builtins/fs_promises.js`, `crates/nimbus-runtime/src/module_loader/builtins/module_fs_modules.js`, `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`, `crates/nimbus-runtime/src/runtime/bootstrap/js/perf_hooks.js`, `crates/nimbus-runtime/src/runtime/bootstrap/ops/test_runtime/bundle.rs`, `crates/nimbus-runtime/src/runtime/tests/node/cases/loader_context_foundation.rs`, `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_core.rs`, `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs`, `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_loader_and_tools.rs`, `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/fixtures/process-and-timing.json`, `crates/nimbus-runtime/src/runtime/tests/node/manifest_topology.rs`, `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`, `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/test/common/index.js`, `scripts/runtime/node/classifications.py`, `scripts/runtime/node/status.py`, generated Node compatibility evidence/docs, `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`, `docs/plans/proof/node-default-runtime-support-hardening/nds3-official-fixture-promotion.md` | Broad pre-run `cargo test -p nimbus-runtime node24_default_lane_ -- --ignored --nocapture --test-threads=1`; focused console/events/URL/dotenv/util/perf-hooks/fs/stream/networking/loader-context/V8 tests pass; promoted broad rerun `node24_default_lane_executes_core_semantics_subset` -> `122 passed, 1 skipped, 0 failed`; promoted broad rerun `node24_default_lane_executes_process_and_timing_subset` -> `48 passed, 0 skipped, 0 failed`; promoted broad rerun `node24_default_lane_executes_streams_and_local_io_subset` -> `308 passed, 0 skipped, 0 failed`; networking broad rerun -> `268 passed, 0 skipped, 0 failed`; loader/context broad checkpoint -> `173 passed, 0 skipped, 4 failed`; focused `node24_loader_context_global_paths_preserve_local_precedence_regression` -> `1 passed`; focused `loader_context_followup_v8_green_batch_fixture` -> `3 passed`; `cargo check -p nimbus-runtime --lib` pass on `v2.8.0-nimbus.15#1f101bf0`; `make node-compat-status`, `make node-compat-dashboard`, `make node-compat-publish-evidence`, and `python3 scripts/runtime/node/default_support_posture.py` pass; `bash scripts/verify-node-default-runtime-support-hardening.sh` -> `15 passed, 19 failed` | Corrected pass accounting from source-topology to execution-only evidence; promoted Node24 core-semantics, process/timing, streams/local-I/O, and networking from ignored red watchpoints to regular green coverage; Deno fork tags `v2.8.0-nimbus.10` through `.15` landed and Nimbus is repinned to `.15`; current Node24 posture is `892 / 5198` with estimated reachable ceiling `2791`; the remaining loader/context checkpoint is honest: three async_hooks exact-count fixtures plus the `test-v8-serdes.js` V8 wire-format boundary remain. Pause deeper NDS3 fixture promotion for `docs/plans/deno-rusty-v8-upstream-alignment-plan.md` before building more claims on this fork stack. |
| 2026-06-01 | NDS3 | in_progress | `Cargo.toml`, `Cargo.lock`, `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`, `docs/architecture/runtime/deno-fork-bump-ledger.md`, `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`, `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua5-nimbus-repin.md`, `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua6-node-compat-rebaseline.md`, `scripts/verify-deno-fork-provenance.sh`, `scripts/verify-deno-fork-upstream-policy.sh` | DUA5: Deno fork check passed; `cargo check -p nimbus-runtime --lib` passed on `v2.8.1-nimbus.1` / `v149.2.0-nimbus.1`; fork provenance verifier `5 passed, 0 failed`; upstream policy verifier `27 passed, 0 failed`. DUA6: Node24 core `122 passed, 1 skipped, 0 failed`; process/timing restored to `48 passed, 0 skipped, 0 failed`; streams/local-I/O restored to `308 passed, 0 skipped, 0 failed`; networking `268 passed, 0 skipped, 0 failed`; loader/context remains `173 passed, 0 skipped, 4 failed`; `make node-compat-status` generated output identical to checked-in `status-summary.md`; `python3 scripts/runtime/node/default_support_posture.py --check` passed; DUA verifier after DUA6 -> `18 passed, 5 failed`. | Upstream-alignment pause gate is satisfied through DUA6. Nimbus now consumes `nimbus/deno v2.8.1-nimbus.1#18f76a9a19ab` and `nimbus/rusty_v8 v149.2.0-nimbus.1#ce6663111a3f`. Two repin regressions were fixed locally (`process.loadEnvFile` host-policy fallback and Node-shaped `fs.watch` missing-entry throw), then the same broad groups reran green. Public generated Node counts did not change: Node24 remains `892 / 5198`; NDS3 should resume from this upstream-aligned baseline after DUA7/DUA8 close the handoff. |

## Risks

| Risk | Mitigation |
| --- | --- |
| Raw official Node pass rate includes CLI and OS-owned behavior that should not run in-process. | Split the denominator and show both full-corpus and V8-isolate-required metrics. Do not hide the full-corpus number. |
| Package canaries overfit mocks and miss real app behavior. | Add multi-package Convex app suites and preserve the official fixture corpus as the broad feedback loop. |
| Node26 churn consumes default-lane effort. | Node26 is observed for the default-support surface but remains non-LTS and non-default until upstream LTS and lane-local gates pass. |
| `nimbus/deno` fixes become local shims in Nimbus. | Promote fixes to the fork when they duplicate Node/Deno builtin semantics or would create long-term hot-path shims. |
| The 2000-pass Node24 target requires lower-level engine work. | NDS1 must produce the feasibility checkpoint before large greening work proceeds, and NDS3 may re-enter the same blocked path if implementation disproves the estimate. Keep the target, preserve failing fixtures as active blockers, and route the implementation to Nimbus runtime, `nimbus/deno`, `rusty_v8`, or a follow-up engine plan. If a fixture requires non-isolate OS ownership, keep the fail-closed diagnostic and full-corpus gap visible. |

## References

- `docs/plans/archive/node-faas-runtime-compatibility-plan.md`
- `docs/plans/archive/node-lts-runtime-trust-plan.md`
- `docs/plans/archive/node-compat-cron-greening-plan.md`
- `docs/runtimes/nodejs/compatibility.md`
- `docs/runtimes/nodejs/reference/node-apis.md`
- `docs/runtimes/nodejs/reference/packages.md`
- `docs/runtimes/nodejs/evidence/latest.md`
- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/architecture/runtime/node-faas-compatibility-profile.json`
- `.github/workflows/node-compat-nightly.yml`
- `~/src/github.com/nodejs/node`
- `~/src/github.com/nimbus/deno`
