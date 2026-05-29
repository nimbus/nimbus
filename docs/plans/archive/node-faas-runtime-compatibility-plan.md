# Node FaaS Runtime Compatibility Plan (NFRC)

Status: `done`
Owner: `runtime / tenant / bridge / convex node-compat / docs`
Research baseline:
`docs/plans/research/node-faas-runtime-compatibility-2026.md`
Proof directory: `docs/plans/proof/node-faas-runtime-compatibility/`
Verifier: `scripts/verify-node-faas-runtime-compatibility.sh`

## Goal

Build a developer-usable and enterprise-trustworthy Node.js compatibility
program for Nimbus functions-as-a-service and Convex-compatible `"use node"`
actions across the targeted Node lines: Node22, Node24, and Node26 Current,
with Node20 retained only as legacy-grace regression coverage.

The plan succeeds when Nimbus uses Node24 as the product default, Node22 and
Node24 have 100% passing evidence for the declared FaaS app profile, Node26 has
a fully classified Current-line lane with lane-local app canaries, the
latest official Node fixture corpus is tracked per lane, and public docs explain
supported, partial, service-routed, local-dev-only, unsupported, and
not-applicable behavior per version in a Deno-style reference.

## Control Plane

This plan is the authoritative execution state. Chat history is useful context,
but an agent resuming after compaction must be able to continue from this file,
the research baseline, proof artifacts, and the current working tree.

### Resume Protocol

1. Read `AGENTS.md`, then this plan, then the research baseline.
2. Read `docs/adapters/convex/ai-guidelines.md` before touching
   Convex-compatible runtime behavior, `packages/convex/`, `demos/convex/`, or
   Convex API surface.
3. Check `git status --short` and treat unrelated dirty files as user or prior
   work. Do not revert them.
4. Read `docs/plans/archive/node-lts-runtime-trust-plan.md` for the completed
   lane-registry and Deno-fork baseline before editing Node compatibility
   ownership.
5. Inspect the ledger and execution log. Continue the single `in_progress` row
   if one exists. If none exists, start the lowest-numbered `pending` row.
6. Load only the row's relevant code, tests, docs, generated artifacts, and
   proof files.
7. For compatibility work, start each row with the widest practical evidence
   run for that row, write the complete failure inventory, then use isolated
   fixtures only to close specific failures. Rerun the wide group before moving
   the row to `done`.
8. Before handoff, context loss, or final response, update the row status,
   execution log, and proof artifact for completed work.

### State Rules

- At most one ledger row may be `in_progress`.
- A row may move to `done` only after every listed acceptance criterion passes,
  every proof artifact exists, and the execution log records exact verification
  output.
- Do not lower support claims to hide failures. Move a behavior out of the
  FaaS support profile only with a named status, a public doc update, and a
  regression test for the diagnostic or service route.
- Official Node fixture failures are allowed only when they are fully
  classified and do not contradict a public FaaS support claim.
- FaaS app profile canaries must pass 100% for every supported LTS lane before
  docs may call the profile supported.
- Node26 remains Current/non-LTS until the registry says it is LTS and the
  supported-LTS gates pass. It may be selectable as a Current-line
  compatibility target, but docs must not describe it as Active LTS,
  Maintenance LTS, or enterprise LTS support before that promotion.
- Generated support pages are source-of-truth for pass rates and API support
  claims. Hand-written prose may summarize but must not duplicate stale
  numbers.
- Compatibility implementation must follow the wide-then-focused loop: vendor
  or enable the broad corpus first, run it to get a failure inventory, classify
  or fix each failure with focused tests, then rerun the broad corpus to prove
  the row's final state. Do not spend repeated loops on small passing tests
  without a current wide-run failure list.
- The final closeout is not complete until the verifier exists and passes from
  a fresh shell.

### Proof Artifact Contract

Each NFRC row writes one proof file:

```text
docs/plans/proof/node-faas-runtime-compatibility/nfrc<N>-<slug>.md
```

Each proof file must include:

- Date, authoring agent, git status summary, and relevant Node tags/SHAs.
- Files changed.
- Decisions made and alternatives rejected.
- Verification commands run with concrete pass/fail counts or command output
  summaries.
- Remaining risks, if any, tied to a later NFRC row or explicitly resolved.

## Current Baseline

As of 2026-05-28:

- Node20 is EOL and stays legacy-grace only.
- Node22 is Maintenance LTS and remains supported until EOL.
- Node24 is Active LTS and should become the product default.
- Node26 is Current, released on 2026-05-05, and should become a Current-line
  compatibility lane now. It enters LTS in October 2026 according to current
  official schedule sources.
- The lane registry already records Node26 metadata but has no selectable
  compatibility target, fixture corpus, or canary results for Node26.
- Node22 and Node24 have lane-local package/framework canaries, but the Convex
  claim is packaging metadata only, not a complete app action flow.
- Public docs explain bounded support, but they still read as general
  compatibility pages rather than Deno-style "how to use Node" plus generated
  API/package reference pages.
- The production in-process implementation remains the existing
  `v8_deno_core` engine. Node20, Node22, Node24, and Node26 are compatibility
  targets/profiles on that engine, not separately compiled official Node or
  `libnode` runtimes.

## Principles

- Product default is not evidence priority.
- FaaS app support is not Node CLI parity.
- Full official Node suite pass is not required; full official Node suite
  classification is required.
- Declared FaaS support must pass 100% on supported LTS lanes.
- Node26 Current evidence is valuable now, but Current is not an enterprise LTS
  promise.
- Node version lanes are compatibility contracts, not claims that Nimbus is
  executing that major version's official Node binary or `libnode`.
- Unsupported host authority must fail clearly or route to a service/microVM
  profile, never appear as a silent in-process shim.
- Docs must be generated from evidence wherever a support claim can drift.
- Wide feedback beats tiny green loops. The plan should deliberately pull in
  broad upstream and app canary coverage early, tolerate long runs when they
  produce a useful issue inventory, and reserve isolated tests for proving
  individual fixes before rerunning the broad lane.

## Execution Strategy

### Wide-Then-Focused Compatibility Loop

Every implementation row that touches runtime compatibility, fixture corpora,
classification, canaries, package behavior, or docs claims must use this loop:

1. Vendor, sync, or enable the broadest relevant Node/FaaS corpus first.
2. Run the broad group once to produce a complete issue inventory, even if the
   run is long.
3. Classify each failure as supported-bug, unsupported, service/microVM
   required, local-dev-only, not applicable, flaky, or harness bug.
4. Fix supported-bugs one at a time with the smallest isolated fixture or
   canary that reproduces the failure.
5. Rerun the isolated fixture after each fix.
6. Rerun the broad group after the failure inventory is closed or explicitly
   classified.
7. Record both the initial broad-run inventory and the final broad-run result
   in the proof artifact.

This is required so the team sees the larger compatibility picture before
optimizing for small green checks. A row may not be completed by running only
isolated tests unless the proof explains why a broad run is not applicable to
that row.

## Compatibility Model

### Engine Framing

The NFRC plan strengthens Node compatibility on top of the current
`v8_deno_core` engine. In production in-process FaaS, Nimbus constructs and
pools the existing Deno/V8 runtime substrate, then exposes verified Node
compatibility profiles for each supported lane.

`Node22`, `Node24`, and `Node26` therefore mean "the Nimbus in-process runtime
meets this declared Node-lane FaaS contract." They do not mean Nimbus has
compiled Node22, Node24, or Node26 into the binary, nor that user code is
running inside the official Node executable or `libnode`.

Embedding real Node through `libnode`, or shipping per-major Node runner
artifacts, is a separate new-engine project. It must satisfy
`docs/architecture/runtime/new-engine-proof-harness.md` before it can become a
selectable production runtime, and it must not be introduced as a hidden
implementation detail of this plan.

### Version Roles

| Lane | Required role after this plan | Default? | Public support promise |
| --- | --- | --- | --- |
| Node20 | Legacy-grace regression | No | Not active enterprise support |
| Node22 | Maintenance LTS supported | No | Supported until EOL with lane-local app evidence |
| Node24 | Active LTS supported | Yes | Default supported lane with lane-local app evidence |
| Node26 | Current-line compatibility | No | Current, non-LTS compatibility after lane-local gates; promote to LTS support only after Node enters LTS and supported-LTS gates pass |

### FaaS Support Statuses

| Status | Required behavior |
| --- | --- |
| Supported in-process | Passes the production in-process FaaS app canary suite on supported LTS lanes. |
| Supported local-dev only | Covered for local tooling but excluded from production in-process docs. |
| Service/microVM required | Available only through explicit service or microVM routing with host authority. |
| Import-compatible stub | Module import succeeds for package probing, but runtime use throws a clear Nimbus diagnostic. |
| Unsupported | Rejected with a documented diagnostic and no support claim. |
| Not applicable to FaaS | CLI, interactive, daemon, or process-wide behavior that does not map to function invocation. |

## Scope

In scope:

- Latest official Node fixture corpus refresh for Node22, Node24, and Node26.
- Node26 compatibility target metadata, process metadata, and Current-line evidence.
- Product default migration from Node22 to Node24.
- Deno-style Node docs for fundamentals, compatibility, Node API reference,
  package reference, and generated evidence.
- Machine-readable FaaS API support manifest and package canary manifest.
- Real Convex `"use node"` app action canaries, not only package metadata.
- App SDK canaries for realistic SaaS/AI/payment/email/GitHub/JWT packages.
- Clear unsupported/service-route diagnostics for child process, workers,
  native addons, inspector, test runner, persistent filesystem, and other
  host-heavy behavior.
- CI/nightly checks that notice new Node patch/minor/LTS releases and require
  evidence refreshes before public claims drift.

Non-goals:

- Claiming complete Node CLI parity.
- Supporting odd-numbered Node release lines as enterprise targets.
- Running arbitrary native addons in the production in-process runtime.
- Granting production in-process child process, worker, inspector, run, FFI, or
  wildcard networking authority.
- Embedding `libnode`, compiling one Node major per Nimbus runtime artifact, or
  replacing the current `v8_deno_core` in-process engine as part of this plan.
- Replacing the Deno/V8 embedded runtime with an external Node process for
  normal in-process FaaS execution.

## Ledger

| NFRC | Description | Status |
| --- | --- | --- |
| NFRC0 | Activate this plan. Confirm the research baseline, proof directory, `docs/plans/README.md` routing, current dirty-worktree caveat, owner-crate map, and wide-then-focused execution strategy. | done |
| NFRC1 | Define a machine-readable FaaS compatibility profile. Include version roles, support statuses, Node API families, package classes, service/microVM routing, local-dev-only behavior, and docs-generation fields. | done |
| NFRC2 | Refresh targeted official Node suite metadata to the latest tags: Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`, and Node20 legacy `v20.20.2`. Record tag object, commit, sync date, and selection command. | done |
| NFRC3 | Add Node26 as a real Current-line compatibility target on the current in-process engine. Wire registry parsing, process metadata, ABI/module metadata, runtime lane diagnostics, and `nimbus-convex` lane selection without making Node26 default, describing it as LTS, or implying an official Node26 runtime is embedded. | done |
| NFRC4 | Sync or vendor the latest official fixture corpora for Node22, Node24, and Node26. Keep Node20 legacy unchanged unless provenance is missing. Require refresh tooling to fail on missing provenance or unclassified published results, and produce an initial wide-run issue inventory before focused fixes. | done |
| NFRC5 | Classify Node26 official fixtures and refresh Node22/Node24 classifications after corpus updates. Preserve zero unclassified fixtures for every targeted lane, using wide-run inventories first and isolated tests only to close specific failures. | done |
| NFRC6 | Move the product default from Node22 to Node24 across runtime registry, tenant policy, bridge execution admission, Convex manifest selection, docs, generated evidence, examples, and diagnostics. | done |
| NFRC7 | Build real Convex `"use node"` app canaries. Cover action deployment metadata, npm package imports, `ctx.runQuery`, `ctx.runMutation`, `ctx.runAction` where runtime crossing is intended, scheduler interaction, value serialization, fetch, env/secrets, Buffer/crypto/stream/path/fs temp behavior, and dangling promise diagnostics. | done |
| NFRC8 | Expand realistic app package canaries. Cover OpenAI, Anthropic, Vercel AI SDK, Stripe, Resend or SendGrid, AWS SDK v3, Slack, Octokit, Jose, Zod, uuid, nanoid, and a database HTTP client. | done |
| NFRC9 | Add negative canaries and diagnostics for host-heavy packages and APIs. Cover `child_process`, worker threads, native addons, inspector, REPL, `node --test`, persistent filesystem assumptions, raw server listen behavior, Prisma engine routing, sharp/esbuild native boundaries, and service/microVM-required outcomes. | done |
| NFRC10 | Generate Deno-style Node docs. Add `docs/runtimes/nodejs/reference/node-apis.md`, `docs/runtimes/nodejs/reference/packages.md`, updated fundamentals/configuration pages, per-version support tables, and stale-prose guards. | done |
| NFRC11 | Add release-train automation. Detect new Node patch/minor/LTS tags, require lane registry drift review, run latest official suite checks per targeted lane, and publish a dashboard summary that distinguishes LTS support from Node26 Current/non-LTS support. | done |
| NFRC12 | Add CI and nightly lanes. Supported LTS FaaS app canaries must gate PRs; official suite refresh and Node26 Current-line checks can run nightly or scheduled with watchpoint output. | done |
| NFRC13 | Closeout. Add `scripts/verify-node-faas-runtime-compatibility.sh`, run final verification, update proof links, and archive this plan only after every ledger row is done. | done |

## Per-Phase Acceptance Criteria

| NFRC | Required proof artifact | Acceptance criteria |
| --- | --- | --- |
| NFRC0 | `nfrc0-baseline-and-control-plane.md` | Plan status flips to `active`; research baseline and proof README exist; `docs/plans/README.md` routes here; dirty worktree caveat captured; owner-crate map matches `nimbus-runtime`, `nimbus-tenant`, `nimbus-bridge`, and `nimbus-convex`; wide-then-focused strategy is explicit in the control plane; `npm run docs:validate-refs:strict` passes. |
| NFRC1 | `nfrc1-faas-compat-profile.md` | A checked-in schema or manifest defines FaaS support statuses, Node API families, package classes, local-dev-only behavior, service/microVM route requirements, and doc-generation fields; tests reject unknown statuses and docs claims without backing evidence. |
| NFRC2 | `nfrc2-latest-node-suite-tags.md` | Lane registry and fixture provenance know the latest official tags listed above; proof records Node release schedule source, tag object, commit, and sync command; stale fixture tags for supported lanes fail a verifier or targeted test. |
| NFRC3 | `nfrc3-node26-current-target.md` | Node26 parses as a Current-line compatibility target where appropriate; process metadata and ABI/module metadata are truthful for the compatibility contract; tenant and Convex selection reject Node26 for enterprise-LTS-only policies but allow explicit Current-line policies; docs and diagnostics do not imply the official Node26 binary or `libnode` is embedded; focused runtime metadata tests pass. |
| NFRC4 | `nfrc4-latest-fixture-corpora.md` | Node22, Node24, and Node26 fixture corpora are synced or deliberately compared against latest official tags; provenance manifests include tag, commit, tag object, sync date, and selection command; refresh dry-run and apply/check outputs are recorded; proof includes the first wide-run issue inventory for each affected lane before focused fixes. |
| NFRC5 | `nfrc5-node26-and-refresh-classification.md` | Node26, Node24, and Node22 have zero unclassified official fixtures in published evidence; expected failures are classified by support status; Node26 failures are marked Current/non-LTS and cannot silently lower LTS support claims; proof includes initial wide-run inventory, focused fixture fixes/classifications, and final wide-run result. |
| NFRC6 | `nfrc6-node24-default.md` | Registry product default is Node24; docs, examples, tenant policy, bridge admission, Convex manifest defaults, runtime diagnostics, generated evidence, and tests no longer treat Node22 as default; Node22 remains supported Maintenance LTS. |
| NFRC7 | `nfrc7-convex-app-canaries.md` | Real Convex app canaries pass 100% for Node22 and Node24; Node26 Current-line canaries are reported separately and must pass before docs claim Node26 Current support; coverage includes action invocation, package imports, context calls, scheduler, serialization, fetch/env/crypto/stream/path/fs temp, and dangling promise diagnostics; proof uses a broad Convex-app canary batch before isolated action-level fixes. |
| NFRC8 | `nfrc8-realistic-sdk-canaries.md` | Each selected app SDK has a lane-local canary on Node22 and Node24 with 100% pass; Node26 Current-line results are recorded and must pass before docs claim Node26 Current support; failed package classes are either fixed, service-routed, or documented as unsupported with tests; proof starts from a broad SDK canary batch and records every package failure. |
| NFRC9 | `nfrc9-host-heavy-diagnostics.md` | Negative canaries prove unsupported/service-routed APIs fail with actionable diagnostics; production in-process profile still has no generic child process, worker, inspector, run, FFI, wildcard listen, or ambient TLS-disable grants; proof starts from a broad host-heavy negative batch before focused diagnostic fixes. |
| NFRC10 | `nfrc10-deno-style-docs.md` | Public docs include a fundamentals page, compatibility page, generated API reference, generated package reference, per-version support tables, and evidence links; docs guard rejects stale pass rates, stale default-lane prose, and unsupported API overclaims. |
| NFRC11 | `nfrc11-release-train-automation.md` | Automation detects new Node tags and lane lifecycle changes; drift requires proof update before support docs change; dashboard distinguishes Node24 default, Node22 support, Node20 legacy, and Node26 Current/non-LTS support. |
| NFRC12 | `nfrc12-ci-nightly-lanes.md` | PR CI gates supported-LTS FaaS app canaries and docs guard; scheduled/nightly lanes run official suite refresh checks and Node26 Current-line reporting; watchpoints are pinned, counted, and not treated as green support. |
| NFRC13 | `nfrc13-closeout.md` | `scripts/verify-node-faas-runtime-compatibility.sh` exists and passes; all rows are `done`; docs, generated evidence, proofs, and plan archive are consistent; final verification commands pass from a fresh shell. |

## Completion Gate

Create `scripts/verify-node-faas-runtime-compatibility.sh` during NFRC13. The
verifier must pass these conditions:

1. Plan exists in active or archived location and the ledger has no `pending`
   rows.
2. Research baseline exists and is linked from this plan.
3. Lane registry marks Node24 as product default, Node22 as supported
   Maintenance LTS, Node20 as legacy-grace/EOL, and Node26 as Current/non-LTS.
4. Node22, Node24, and Node26 fixture provenance records latest official tags,
   tag objects, commits, sync dates, and selection commands.
5. Official suite evidence has zero unclassified fixtures for every targeted
   lane.
6. Supported LTS FaaS app profile canaries pass 100% for Node22 and Node24.
7. Node26 Current-line app canaries run lane-local and publish Current/non-LTS
   status separately from LTS support.
8. Real Convex `"use node"` app canaries pass for supported LTS lanes.
9. Realistic SDK canaries pass or have explicit unsupported/service-routed
   diagnostics backed by tests.
10. Host-heavy APIs and package classes have clear negative tests and public
    docs.
11. Public Deno-style docs are generated from the compatibility manifest and
    evidence snapshots.
12. Docs guard rejects stale default, stale pass-rate, and unsupported API
    overclaim prose.
13. Release-train automation detects Node patch/minor/LTS drift.
14. Production in-process Node permission profile remains least-authority and
    does not gain child process, worker, inspector, run, FFI, wildcard listen,
    or ambient TLS-disable grants.
15. Public docs and generated runtime diagnostics identify Node lanes as
    compatibility targets on the current in-process engine and do not claim or
    imply that Nimbus embeds the official Node binary or `libnode` for each
    supported major.
16. Proof artifacts for corpus, classification, Convex app canaries, SDK
    canaries, and host-heavy diagnostics include an initial wide-run failure
    inventory, focused fix/classification evidence, and a final wide-run result.
17. `cargo fmt --all --check`, `npm run docs:validate-refs:strict`, the NFRC
    verifier, and focused runtime/Convex tests pass.

## Verification Commands

Expected final verification:

```bash
cargo fmt --all --check
npm run docs:validate-refs:strict
bash scripts/verify-node-faas-runtime-compatibility.sh
cargo test -p nimbus-runtime node_lts -- --nocapture
cargo test -p nimbus-runtime node26 -- --nocapture
cargo test -p nimbus-runtime node_faas -- --nocapture
cargo test -p nimbus-tenant node_profile -- --nocapture
cargo test -p nimbus-bridge runtime_execution_admission -- --nocapture
cargo test -p nimbus-convex runtime_access -- --nocapture
make node-compat-canaries PRESET=application
make node-compat-canaries PRESET=tooling
make node-compat-publish-docs CHECK=1
git diff --check
```

When a row touches official fixture corpora, also run the documented
`make node-compat-refresh` path for the affected lanes and record the generated
status/dashboard summaries in the proof artifact.

## Execution Log

| Date | NFRC | Status | Files touched | Verification | Notes |
| --- | --- | --- | --- | --- | --- |
| 2026-05-28 | preflight | done | `docs/plans/research/node-faas-runtime-compatibility-2026.md`, `docs/plans/node-faas-runtime-compatibility-plan.md`, `docs/plans/proof/node-faas-runtime-compatibility/README.md`, `docs/plans/README.md` | `npm run docs:validate-refs:strict`: pass, 223 working-tree Markdown files; trailing-whitespace scan: pass | Research and ready plan drafted after NLRT closeout; execution not yet activated. |
| 2026-05-28 | NFRC0 | done | `docs/plans/node-faas-runtime-compatibility-plan.md`, `docs/plans/research/node-faas-runtime-compatibility-2026.md`, `docs/plans/proof/node-faas-runtime-compatibility/nfrc0-baseline-and-control-plane.md`, `docs/plans/README.md` | `npm run docs:validate-refs:strict`: pass, 223 working-tree Markdown files; `git diff --check`: pass | Plan activated; goal-ready control plane now requires the wide-then-focused compatibility loop. |
| 2026-05-28 | NFRC1 | done | `docs/architecture/runtime/node-faas-compatibility-profile.json`, `docs/architecture/runtime/node-faas-compatibility-profile.md`, `tests/runtime/node/schemas/node-faas-compatibility-profile.schema.json`, `scripts/runtime/node/faas_profile.py`, `scripts/verify-node-faas-compat-profile.sh`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/runtimes/nodejs/compatibility.md`, `tests/runtime/node/README.md`, `docs/plans/proof/node-faas-runtime-compatibility/nfrc1-faas-compat-profile.md` | `bash scripts/verify-node-faas-compat-profile.sh`: pass, 6 statuses, 4 lanes, 11 API families, 7 package classes, 4 doc claims; `npm run docs:validate-refs:strict`: pass, 224 working-tree Markdown files; `bash scripts/verify-node-lts-docs.sh`: pass; `git diff --check`: pass | Machine-readable FaaS profile added with schema validation and negative self-tests for unknown statuses, evidence-free claims, unknown evidence refs, and disabled wide-rerun strategy. |
| 2026-05-28 | NFRC2 | done | `docs/architecture/runtime/node-lts-compat/node-latest-suite-tags.json`, `docs/architecture/runtime/node-lts-compat/node-latest-suite-tags.md`, `tests/runtime/node/schemas/node-latest-suite-tags.schema.json`, `scripts/runtime/node/latest_suite_tags.py`, `scripts/verify-node-latest-suite-tags.sh`, `docs/architecture/runtime/node-lts-compat/node-lts-lanes.md`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `tests/runtime/node/README.md`, `docs/plans/proof/node-faas-runtime-compatibility/nfrc2-latest-node-suite-tags.md` | `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 3 needing fixture sync; `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh`: expected fail for node22, node24, node26 stale/missing corpora; `bash scripts/verify-node-lts-lanes.sh`: pass; `npm run docs:validate-refs:strict`: pass, 225 working-tree Markdown files; `git diff --check`: pass | Latest official suite tags now have a checked-in registry with tag object, commit, current corpus tag, and intended sync command; stale corpus enforcement is opt-in until NFRC4 syncs corpora. |
| 2026-05-28 | NFRC3 | done | `crates/nimbus-runtime/src/limits/axes.rs`, `crates/nimbus-runtime/src/limits/resources.rs`, `crates/nimbus-runtime/src/module_loader.rs`, `crates/nimbus-runtime/src/runtime/bootstrap/transpile.rs`, `crates/nimbus-runtime/src/runtime/driver/construction.rs`, `crates/nimbus-runtime/src/runtime/tests/basic_invocation/node_bootstrap.rs`, `crates/nimbus-tenant/src/operator_policy.rs`, `crates/nimbus-convex/src/lib.rs`, `crates/nimbus-convex/src/registry/loading.rs`, `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`, Node runtime docs, lane registry verifier/docs, `docs/plans/proof/node-faas-runtime-compatibility/nfrc3-node26-current-target.md` | `cargo test -p nimbus-runtime node_lts -- --nocapture`: pass, 3 tests; `cargo test -p nimbus-runtime node26_current_target_exposes_truthful_process_metadata -- --nocapture`: pass, 1 test; `cargo test -p nimbus-tenant node_runtime_profiles_follow_lts_registry_targets -- --nocapture`: pass, 1 test; `cargo test -p nimbus-convex convex_node_runtime_lanes_follow_lts_registry_targets -- --nocapture`: pass, 1 test; `cargo test -p nimbus-convex convex_use_node_action_package_canary_node26_current -- --ignored --nocapture`: pass, 1 test; `bash scripts/verify-node-lts-lanes.sh`: pass; `bash scripts/verify-node-latest-suite-tags.sh`: pass; `npm run docs:validate-refs:strict`: pass, 225 working-tree Markdown files; `bash scripts/verify-node-lts-docs.sh`: pass; `cargo check --workspace`: pass; `git diff --check`: pass | Node26 is now a selectable Current/non-LTS target on the existing in-process engine. Initial checks found two stale exhaustive matches and one missing inspector option; all were fixed. Node26 remains outside supported-LTS target lists and is not product default. |
| 2026-05-28 | NFRC4 | done | Node22/Node24/Node26 vendored fixture corpora, lane manifests, lane registry metadata, latest-suite registry, sync/refresh tooling, manifest schema/tests, inventory/status scripts, docs refresh guide, docs-ref guard, `docs/plans/proof/node-faas-runtime-compatibility/nfrc4-latest-fixture-corpora.md` | `python3 scripts/runtime/node/sync.py --lane node22 --upstream-tag v22.22.3 --compare-upstream --source-root /Users/jack/src/github.com/nodejs/node`: pass; same compare for node24/node26: pass; same `--apply` for node22/node24/node26: pass; `python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc4-initial`: expected fail with stale classification warnings and initial unclassified inventory; `python3 scripts/runtime/node/inventory.py --lane node22|node24|node26 --output-root target/node-compat/inventory-nfrc4-initial`: pass with inventory warnings; `python3 scripts/runtime/node/fixture_provenance.py validate --status-summary target/node-compat/status-nfrc4-initial/status-summary.json`: expected fail for Node22 3,340 and Node24 3,623 unclassified fixtures; `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh`: pass; manifest metadata/resolution tests: pass; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass; `bash scripts/verify-node-lts-docs.sh`: pass; `git diff --check`: pass | Latest corpora are vendored from the canonical local Node checkout. Initial wide status inventory intentionally exposes Node22/Node24/Node26 unclassified remainders for NFRC5 instead of making narrow green claims. |
| 2026-05-28 | NFRC5 | done | `tests/runtime/node/classifications/node22.json`, `tests/runtime/node/classifications/node24.json`, `tests/runtime/node/classifications/node26.json`, generated `docs/architecture/runtime/node-compat-evidence/latest/`, generated `docs/runtimes/nodejs/evidence/`, `docs/plans/proof/node-faas-runtime-compatibility/nfrc5-node26-and-refresh-classification.md` | `python3 scripts/runtime/node/classifications.py sync --lane node22|node24|node26`: pass; `python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc5-classified`: pass, zero unclassified across Node20/22/24/26; `python3 scripts/runtime/node/inventory.py --lane node22|node24|node26 --output-root target/node-compat/inventory-nfrc5-classified`: pass, zero warnings; default status/inventory/dashboard/trends/publish scripts: pass; `bash scripts/runtime/node/validate-claims.sh`: pass, 12 mappings against 12 canaries; `python3 scripts/runtime/node/fixture_provenance.py validate`: pass; `bash scripts/verify-node-latest-suite-tags.sh`: pass; enforced latest-suite mode: pass; `bash scripts/verify-node-lts-lanes.sh`: pass; `bash scripts/verify-node-lts-docs.sh`: pass; `npm run docs:validate-refs:strict`: pass, 226 working-tree Markdown files; manifest metadata/resolution tests: pass, 3 and 7 tests; `cargo fmt --all --check`: pass; `git diff --check`: pass | Wide NFRC4 inventory drove bulk classification, then final wide status proved zero unclassified fixtures. Node26 remains Current/non-LTS with all official fixtures classified as known gaps or skips until NFRC7/NFRC8/NFRC12 add lane-local Current canaries and scheduled reporting. |
| 2026-05-28 | NFRC6 | done | Node lane registry/manifests, runtime/tenant/Convex default-selection tests, codegen defaults, watchpoint/classification catalogs, representative report artifacts, generated evidence, public Node runtime docs, node-lts compatibility narratives, `docs/plans/proof/node-faas-runtime-compatibility/nfrc6-node24-default.md` | `python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc6-default-swap`: pass after focused watchpoint/report regeneration; `python3 scripts/runtime/node/inventory.py --lane node20|node22|node24|node26 --output-root target/node-compat/inventory-nfrc6-default-swap`: pass; default status/dashboard/trends/publish scripts: pass; `bash scripts/verify-node-lts-lanes.sh`: pass, product default `node24`; `python3 scripts/runtime/node/fixture_provenance.py validate`: pass; latest-suite verifier normal and enforced modes: pass; `bash scripts/verify-node-lts-docs.sh`: pass; `bash scripts/runtime/node/validate-claims.sh`: pass; manifest metadata/resolution/topology/report tests: pass, 3/7/17/11 tests plus 1 ignored manual report entrypoint; `cargo test -p nimbus-runtime node_lts -- --nocapture`: pass, 3 tests; tenant and Convex lane registry tests: pass; `npm run test`: pass; `bash scripts/verify-node-lts-runtime-trust.sh`: pass, 16 sections; `npm run docs:validate-refs:strict`: pass, 226 Markdown files | Node24 is now the product default and Active LTS lane; Node22 remains supported Maintenance LTS; Node20 is legacy-grace/EOL; Node26 remains Current/non-LTS. Broad status first exposed stale role wording and report artifacts, focused fixes closed them, and final broad evidence regenerated cleanly. |
| 2026-05-28 | NFRC7 | done | Convex nested runtime dispatch, real Convex `"use node"` app canary, canary registry/dashboard gates, generated evidence, `docs/plans/proof/node-faas-runtime-compatibility/nfrc7-convex-app-canaries.md` | `cargo test -p nimbus-server convex_use_node_real_app_canary -- --nocapture --test-threads=1 --ignored`: pass, 3 tests; `make node-compat-canaries PRESET=application`: pass, 19 canary checks passed and 0 failed; `cargo test -p nimbus-server convex_runtime_only_action_can_run_runtime_only_mutation -- --nocapture`: pass, 1 test; `bash scripts/verify-node-lts-canaries-and-oracles.sh`: pass, 12 checks and 0 failures; `bash scripts/runtime/node/validate-claims.sh`: pass, 13 mappings against 13 canaries; `python3 scripts/runtime/node/fixture_provenance.py validate`: pass; `make node-compat-publish-docs CHECK=1`: pass; `cargo fmt --all --check`: pass; `git diff --check`: pass; `npm run docs:validate-refs:strict`: pass, 226 Markdown files | Broad Application canary proof now includes real Convex app action flow for Node22/Node24 plus separate Node26 Current evidence. Initial wide feedback exposed a real nested-runtime lane-selection bug where `ctx.runAction` could execute the callee under the default WebStandard lane; dispatch now resolves the callee function lane before invocation. Final dashboard has 13 canary claims, 29 checks, 2 oracle reports, 0 required gaps, and no stale Node22-default/Node24-supported role labels. |
| 2026-05-28 | NFRC8 | done | Realistic SDK canary app, SDK batch harness, Application permission profile, canary registry/dashboard gates, generated evidence, `docs/plans/proof/node-faas-runtime-compatibility/nfrc8-realistic-sdk-canaries.md` | `make node-compat-canaries PRESET=application`: pass, 58 canary checks passed and 0 failed; `cargo test -p nimbus-runtime application_node22_sdk_package_canary_batch -- --nocapture --test-threads=1 --ignored`: pass, 1 test; same Node24 and Node26 SDK batch tests: pass, 1 test each; `bash scripts/verify-node-lts-canaries-and-oracles.sh`: pass, 12 checks and 0 failures; `bash scripts/runtime/node/validate-claims.sh`: pass, 26 mappings against 26 canaries; `python3 scripts/runtime/node/fixture_provenance.py validate`: pass; `make node-compat-publish-docs CHECK=1`: pass; `cargo test -p nimbus-runtime application_preset_supports_node_lts_targets -- --nocapture`: pass, 1 test; `cargo test -p nimbus-runtime node_permission_profiles_are_separated_by_deployment_intent -- --nocapture`: pass, 1 test; `cargo fmt --all --check`: pass; `git diff --check`: pass; `npm run docs:validate-refs:strict`: pass, 227 Markdown files | Broad SDK batch first exposed Anthropic base URL, AWS shared-config/homedir, Slack `os.release`, UUID expectation, and Upstash pipeline-shape failures. Focused fixes kept `homedir` denied, allowed only read-only `osRelease`, configured AWS explicitly, corrected mocks/expectations, then reran the full Application preset. Final dashboard has 26 canary claims, 68 checks, 2 canary reports, 2 oracle reports, and 0 required gaps. |
| 2026-05-28 | NFRC9 | done | Host-heavy diagnostic canary app, diagnostic evidence metadata in registry/dashboard/public docs, native-addon and unsupported-builtin diagnostics, generated evidence, `docs/plans/proof/node-faas-runtime-compatibility/nfrc9-host-heavy-diagnostics.md` | `cargo test -p nimbus-runtime application_node22_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored`: pass, 1 test after initial broad diagnostic inventory; same Node24 and Node26 diagnostic batch tests: pass, 1 test each; `make node-compat-canaries PRESET=application`: pass, 91 canary checks passed and 0 failed; `bash scripts/verify-node-host-heavy-diagnostics.sh`: pass, 7 checks and 0 failures; `bash scripts/verify-node-lts-canaries-and-oracles.sh`: pass, 12 checks and 0 failures; `bash scripts/runtime/node/validate-claims.sh`: pass, 37 mappings against 37 canaries; `bash scripts/verify-node-faas-compat-profile.sh`: pass; `make node-compat-publish-docs CHECK=1`: pass | Initial host-heavy diagnostic batch exposed four concrete diagnostic-shape issues: empty `spawnSync` denial for child process and `node --test`, raw-listen net denial surfacing as invocation error, and esbuild's unsupported lifecycle path surfacing through `unref`. Focused fixes classified or improved each boundary, preserved production no-run/no-worker/no-listen/no-ffi posture, and the final dashboard now distinguishes 11 diagnostic claims from positive support claims with 37 total claims, 101 checks, and 0 gaps. |
| 2026-05-28 | NFRC10 | done | Deno-style public Node runtime docs, generated API/package/compatibility references, stale support guard, FaaS profile evidence-state refresh, `docs/plans/proof/node-faas-runtime-compatibility/nfrc10-deno-style-docs.md` | `make node-compat-publish-docs`: pass; `make node-compat-publish-docs CHECK=1`: pass; `bash scripts/verify-node-faas-compat-profile.sh`: pass, 6 statuses, 4 lanes, 11 API families, 7 package classes, 4 doc claims; `bash scripts/verify-node-lts-docs.sh`: pass; `npm run docs:validate-refs:strict`: pass, 231 Markdown files; `git diff --check`: pass; `cargo fmt --all --check`: pass | Broad generated-doc publication came first, then the stale guard exposed over-broad guard patterns and missing generated snippets. Focused fixes narrowed the guard, promoted already-proven API/package evidence to `current_evidence`, and final docs now explain fundamentals, per-version support, API/package boundaries, Node26 Current/non-LTS, and service/microVM diagnostics without stale pass-rate or overclaim prose. |
| 2026-05-28 | NFRC11 | done | Release-train drift automation, generated release-train summary, schema, proof digest gate, Make target, `docs/plans/proof/node-faas-runtime-compatibility/nfrc11-release-train-automation.md` | `python3 scripts/runtime/node/release_train.py probe-live`: pass with network approval, 4 lanes matched official release feeds; `python3 scripts/runtime/node/release_train.py publish --check-proof`: pass; `bash scripts/verify-node-release-train.sh`: pass, 4 lanes, 0 drift entries, negative self-tests passed; `make node-compat-release-train CHECK=1`: pass; `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 0 needing fixture sync; `bash scripts/verify-node-lts-lanes.sh`: pass, product default `node24`; `npm run docs:validate-refs:strict`: pass, 232 Markdown files; `git diff --check`: pass | Broad release-train automation now checks lane registry, latest official tags, status/dashboard role separation, source digests, and optional official live feeds. The first live probe caught a Node26 maintenance-date mismatch between a search snippet and official schedule JSON; the registry stayed aligned to the official schedule JSON and the final live probe passed. Future release metadata edits must update the NFRC11 proof digest markers before the release-train verifier passes. |
| 2026-05-28 | NFRC12 | done | PR Node FaaS compatibility gate, scheduled Node compatibility workflow expansion, CI/nightly lane verifier, `docs/plans/proof/node-faas-runtime-compatibility/nfrc12-ci-nightly-lanes.md` | `bash scripts/verify-node-ci-nightly-lanes.sh`: pass, 16 checks; `make node-compat-canaries PRESET=application LANE=node22`: pass with local bind/listen approval, 32 passed and 0 failed; same Node24 lane: pass, 32 passed and 0 failed; enforced latest-suite verifier: pass, all targeted corpora current; docs guard: pass; release-train verifier: pass; watchpoint validation: pass, 67 entries; LTS canary/oracle verifier: pass, 12 checks; host-heavy verifier: pass, 7 checks; `npm run docs:validate-refs:strict`: pass, 232 Markdown files; `git diff --check`: pass | PR CI now gates Node22/Node24 Application canaries, docs, latest-suite metadata, release-train metadata, canary claims, and host-heavy diagnostic boundaries. The scheduled Node compatibility workflow owns official corpus freshness, live release-feed drift, watchpoints, broad Application/Tooling canaries, and Node26 Current-line oracle reporting. Initial local canary runs exposed sandbox bind denial; rerunning with local-bind approval proved the CI commands themselves. |
| 2026-05-28 | NFRC13 | done | Final all-row verifier, archived plan, proof index closeout, FaaS profile archived-plan pointer, `docs/plans/proof/node-faas-runtime-compatibility/nfrc13-closeout.md` | `bash scripts/verify-node-faas-runtime-compatibility.sh`: pass, 26 checks and 0 failures; includes `cargo fmt --all --check`, `npm run docs:validate-refs:strict`, broad Application/Tooling canaries, and `git diff --check` | Closeout verifier now owns plan/proof/archive consistency plus every generated-doc, release-train, canary, watchpoint, runtime, tenant, bridge, Convex, formatting, Markdown reference, and diff-whitespace gate. |

## Risk Register

| Risk | Mitigation |
| --- | --- |
| Node26 Current changes quickly before LTS. | Keep Node26 Current/non-LTS, run lane-local evidence, and promote to LTS support only by registry data plus supported-LTS gates. |
| Public readers confuse official suite pass rate with support. | Split official suite classification from FaaS profile pass status in docs and generated dashboards. |
| Product default migration accidentally drops Node22 support. | NFRC6 requires default-only changes while preserving Node22 Maintenance LTS tests. |
| Convex action proof remains packaging-only. | NFRC7 requires real app action flows and context calls. |
| Package canaries test mocks instead of useful developer workflows. | NFRC8 requires representative SDK workflows and lane-local results. |
| Long tests consume time but produce too little actionable feedback. | The wide-then-focused loop requires broad corpus runs to produce complete issue inventories before isolated fix loops, then final broad reruns after fixes. |
| Host-heavy APIs become accidental ambient authority. | NFRC9 and completion gate require negative tests plus permission-profile checks. |
| Deno-style docs drift from generated evidence. | NFRC10 and NFRC13 require generated references and stale-prose guards. |
| Future agents treat Node lanes as compiled Node engines. | Engine framing, NFRC3, and the completion gate require docs and diagnostics to name Node lanes as compatibility targets on the current in-process engine; real `libnode` work must use the new-engine proof harness. |

## Readiness Audit

Audited on 2026-05-28.

Findings:

- Active as an execution control plane; NFRC0 captured activation and the
  wide-then-focused strategy.
- The research baseline names current official release/provider facts and
  separates LTS support claims from Current/non-LTS evidence.
- Every ledger row has a proof artifact and verifiable acceptance criteria.
- The completion gate requires both upstream-suite classification and real
  FaaS/Convex app success, which is the correct enterprise-trust boundary.
