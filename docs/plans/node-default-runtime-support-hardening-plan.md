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
| `node20` | legacy | `v20.20.2` | 902 / 1308 | 401 | 5 | 69.0% |
| `node22` | supported | `v22.22.3` | 1000 / 4748 | 3728 | 20 | 21.1% |
| `node24` | default | `v24.16.0` | 1002 / 5198 | 4149 | 47 | 19.3% |
| `node26` | current | `v26.2.0` | 0 / 5578 | 5529 | 49 | 0.0% |

The package canary matrix is useful and real, but the default lane still has a
large official-fixture gap and Node26 has no official fixture passes yet. A
default runtime should be able to show more than "the packages we selected pass";
it should show broad, repeatable, lane-local support evidence and an explicit
denominator for what "well-supported" means.

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
   - out-of-scope or upstream/platform fixtures
2. **No vague default denominator remains.**
   Node24 has zero `Requires Unpromoted Node Surface` entries. Every former
   unpromoted entry is either passed, V8-isolate-required gap,
   V8-isolate-optional gap, diagnostic-only non-isolate, or
   explicitly out-of-scope with a reason.
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
   claims across at least 12 categories, with zero required canary gaps. Native,
   binary, child-process, raw-listen, and persistent-filesystem packages remain
   diagnostic-only non-isolate behavior and do not count as positive support.
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

## Goal Control Plane Objective

When this plan is activated as a goal, use this objective:

Complete `docs/plans/node-default-runtime-support-hardening-plan.md`
autonomously end to end. Success means Nimbus raises Node24 from bounded
V8-isolate-compatible default to verifier-backed well-supported default, keeps
Node22 as a supported LTS peer with comparable evidence, gives Node26 real
Current-line fixture evidence, expands positive Application package evidence to
at least 50 claims across at least 12 categories, proves at least 5 realistic
Convex-compatible `"use node"` app suites, preserves fail-closed diagnostics
for non-isolate behavior, uses truthful compatibility shims where they improve
V8-isolate compatibility, documents emulated capabilities and their limits,
audits `nimbus/nimbus` and `nimbus/deno` for every shim/emulation/stub,
rejects fake-success stubs, regenerates Deno-style docs from the new posture,
wires PR/nightly gates, and passes
`bash scripts/verify-node-default-runtime-support-hardening.sh`. Execution must
follow the wide-then-focused loop: run broad vendored corpora first to capture
failure inventory, fix or classify clustered failures with isolated tests, then
rerun the same broad groups and close rows only on measured coverage gains or
verified non-isolate diagnostics.

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
| NDS0 | Baseline and verifier scaffold. Capture current Node20/22/24/26 fixture pass rates, package canaries, non-isolate diagnostics, Node26 0-pass posture, transferred lessons, and the NFRC boundary. Mark the older cron-greening plan as subsumed. | `nds0-baseline.md` exists; verifier script exists and fails on every unimplemented gate; baseline records Node24 `1002 / 5198`, Node22 `1000 / 4748`, Node26 `0 / 5578`, current package claim count, the wide-then-focused rule, and the transferred lessons above; docs refs and `git diff --check` pass. | pending |
| NDS1 | Default-support posture model. Build JSON/Markdown plus schema for full corpus, V8-isolate-required, V8-isolate-optional, diagnostic-only non-isolate, out-of-scope, and upstream/platform denominators. | Posture generator validates schema; Node24 has zero `Requires Unpromoted Node Surface`; every moved fixture has category, reason, evidence path, and shim/emulation classification where relevant; status dashboard reports full-corpus and V8-isolate-required metrics separately; wide pre/post inventories are recorded. | pending |
| NDS2 | Foundation-slice greening. Complete the useful cron-greening work inside this plan: lane-aware process metadata and module/async loader-context fixes. | Broad foundation slice run across Node22/Node24 is captured before fixes; focused tests name and fix each failing fixture; final broad rerun is green for Node22/Node24; any intentional divergence has an ignored watchpoint plus failure-inventory entry; no silent quarantine. | pending |
| NDS3 | High-value official fixture promotion. Raise Node24/Node22 support by clusters: module/loader, assert/buffer, events/util, URL/querystring, streams, timers/AbortController, crypto/WebCrypto, DNS/TLS/client networking, selected `fs/promises`, process metadata, and diagnostics_channel. | Each cluster proof has initial broad failure list, focused fixes, and final broad rerun; Node24 full-corpus pass count reaches at least 2000; Node22 stays within 5 percentage points of Node24 or has proven version-specific upstream deltas; V8-isolate-required pass rate is 100% on Node22 and Node24; remaining V8-isolate-optional gaps are inventoried instead of treated as the target. | pending |
| NDS4 | Node26 Current evidence. Run the same foundation and V8-isolate-required fixture sets against Node26 and fix current-line metadata/bootstrap drift. | Node26 official fixture pass count reaches at least 1000; Node26 passes the V8-isolate-required surface shared with Node24 except fixture-by-fixture proven upstream removals; Node26 no longer blanket-classifies the default-support surface as known gap; Node26 package and fixture docs show observed Current/non-LTS evidence separately from LTS support; final broad Node26 run is recorded. | pending |
| NDS5 | Package and framework canary expansion. Add positive Application canaries for realistic app packages across AI, HTTP, auth/JWT, validation, payments, email, object storage, HTTP database clients, observability, webhooks/signing, loader edge cases, and request/response adapters. | At least 50 positive Application claims pass on Node22 and Node24 across at least 12 categories; the harness reports all failures in a lane; required canary gaps are 0; Node26 observations are recorded separately; deterministic mocks model real SDK paths; diagnostic-only non-isolate packages are excluded from positive counts. | pending |
| NDS6 | Real Convex app suites. Add realistic Convex-compatible `"use node"` app suites beyond single canary actions. | At least 5 app suites pass on Node22 and Node24; suites cover package actions, callee-lane selection for nested runtime calls, `ctx.runQuery`/`ctx.runMutation`/intended `ctx.runAction`, generated APIs, scheduled/background action flow, ESM/CJS/conditional exports, value serialization, and SaaS SDK usage; Convex guidelines are followed. | pending |
| NDS7 | Permission and non-isolate boundary plus shim audit. Keep unsupported OS-owned behavior explicit while expanding support, and audit every compatibility shim/emulation/stub in `nimbus/nimbus` plus the `nimbus/deno` fork. | Child process, worker threads, raw listen, native addons, package-owned binaries, persistent filesystem assumptions, and CLI/test-runner surfaces fail closed with useful diagnostics inside the V8 isolate runtime unless explicitly classified as truthful shim or documented emulation; diagnostics pass on Node22/Node24/Node26; diagnostics carry `evidence_kind=diagnostic` or equivalent and are not counted as positive support; tests prove fake-success stubs are rejected; `node-isolate-shim-inventory` or equivalent records source locations, capability limits, evidence, documentation anchors, and owner repo for every shim/emulation/stub. | pending |
| NDS8 | Deno-style docs from posture and shim inventory. Regenerate public compatibility, API, package, evidence, and shim/emulation docs from the new posture. | Docs show per-version full-corpus, V8-isolate-required, package, non-isolate diagnostic metrics, and explicit "native", "shimmed", "emulated", "test-harness-only", and "unsupported" classifications; Node24 says well-supported default only after gates pass; Node26 is Current/non-LTS with real evidence; support numbers and shim claims are generated or guarded against stale prose; `make node-compat-publish-docs CHECK=1`, docs guard, and strict docs refs pass. | pending |
| NDS9 | PR and nightly gates. Keep the raised support true over time. | PR CI includes the default-support verifier, Node24 posture, Node22 parity, package canaries, docs claims, and non-isolate diagnostics; nightly includes broad official fixture groups, release-train drift from official Node feeds, latest-suite drift, watchpoint validation, Node26 Current evidence, and posture trends; structural verifier proves the workflow wiring. | pending |
| NDS10 | Closeout and archive. Finish all rows and prove the final state. | Every row is `done`; execution log records commands and counts; final verifier prints `24 passed, 0 failed`; generated docs are current; `cargo fmt --all --check`, strict docs refs, and `git diff --check` pass; plan moves to archive and routing points to the archived baseline. | pending |

## Completion Gate

`bash scripts/verify-node-default-runtime-support-hardening.sh` exits 0 with a
summary line `24 passed, 0 failed`. The verifier must check at least:

1. Plan is active or archived and every ledger row is `done` at closeout.
2. Baseline proof exists and records the current low Node24/Node26 posture.
3. Default-support posture JSON and Markdown exist and validate against schema.
4. Node24 has zero `Requires Unpromoted Node Surface` entries.
5. Node24 and Node22 V8-isolate-required official fixture pass rate is 100%.
6. Node26 has at least 1000 official fixture passes, passes the shared
   V8-isolate-required surface, and has no blanket known-gap treatment for the
   default-support surface.
7. Foundation slices pass on Node22 and Node24.
8. Node24 full-corpus official pass count is at least 2000.
9. Positive Application package claims are at least 50 across at least 12
   categories on Node22 and Node24.
10. Required Application package canary gaps are 0.
11. At least 5 Convex-compatible real app suites pass on Node22 and Node24.
12. Non-isolate diagnostics pass and are excluded from positive support counts.
13. Generated public docs match checked-in posture and evidence.
14. Package reference contains per-version support, not only aggregate support.
15. API reference contains per-version support and non-isolate boundaries.
16. Shim/emulation inventory covers `nimbus/nimbus` and `nimbus/deno`, has no
    unclassified Node compatibility surfaces, and records source annotations or
    source comments for Nimbus-owned shims where practical.
17. User-facing docs disclose native, shimmed, emulated, test-harness-only,
    diagnostic, and unsupported capability classes.
18. Release-train and latest-suite drift checks pass.
19. PR CI includes the new default-support gate.
20. Nightly workflow includes broad fixture, package, and Node26 Current lanes.
21. `cargo fmt --all --check`, strict docs refs, and `git diff --check` pass.
22. Every NDS row proof records the wide-then-focused loop: broad pre-run,
    failure inventory, focused fixes/classifications, and broad final rerun.
23. The verifier rejects diagnostic canaries counted as positive support and
    fake-success stubs that claim unsupported OS side effects happened.
24. The verifier rejects stale hand-written support numbers that disagree with
    generated evidence.

## Execution Log

| Date | NDS | Status | Files touched | Verification | Notes |
| --- | --- | --- | --- | --- | --- |
| _pending NDS0_ | NDS0 | _pending_ | | | |

## Risks

| Risk | Mitigation |
| --- | --- |
| Raw official Node pass rate includes CLI and OS-owned behavior that should not run in-process. | Split the denominator and show both full-corpus and V8-isolate-required metrics. Do not hide the full-corpus number. |
| Package canaries overfit mocks and miss real app behavior. | Add multi-package Convex app suites and preserve the official fixture corpus as the broad feedback loop. |
| Node26 churn consumes default-lane effort. | Node26 is observed for the default-support surface but remains non-LTS and non-default until upstream LTS and lane-local gates pass. |
| `nimbus/deno` fixes become local shims in Nimbus. | Promote fixes to the fork when they duplicate Node/Deno builtin semantics or would create long-term hot-path shims. |
| The 2000-pass Node24 target requires lower-level engine work. | Keep the target, preserve failing fixtures as active blockers, and route the implementation to Nimbus runtime, `nimbus/deno`, `rusty_v8`, or a follow-up engine plan. If a fixture requires non-isolate OS ownership, keep the fail-closed diagnostic and full-corpus gap visible. |

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
