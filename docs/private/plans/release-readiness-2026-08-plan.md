# Nimbus Release Readiness 2026-08

Status: `active`. Owner: this plan. Created: 2026-08-27.
Baseline: `codex/storage-review-repairs` @ `1403bc780` (`origin/main` @ `b57a2d680`)
Proof root: `proof/release-readiness-2026-08/`
Next action: run all nine application and adapter cases in RRC3 with the
provisional integrated binary and supported Node 24

## Outcome

> Nimbus ships only after every advertised capability and release control has
> direct evidence from the candidate build, with no accepted release-blocking
> defect and no required lane reported green through a skip.

## Architecture

Before:

```text
[unit and CI gates] + [separate app, machine, desktop, and package proofs]
  -> no one release-candidate verdict
```

After:

```text
[candidate commit]
  -> [macOS product and app matrix]
  -> [Linux minicloud platform matrix]
  -> [desktop shell matrix]
  -> [package and install matrix]
  -> [46-condition evidence verifier] -> [GO or NO-GO verdict]
```

## Scope

- Owns: product-wide QA, local application smoke tests, desktop validation,
  cross-platform validation, defect repair, and the release-candidate verdict.
- Owns: Nimbus fixes found by this campaign and coordinated desktop fixes in
  `/Users/jack/src/github.com/nimbus/desktop`.
- Consumes: the storage repairs through `1403bc780` and current `origin/main`.
- Coordinates with: `distribution-plan.md`, which keeps ownership of public
  apt, COPR, cloud-image, tag, and release publication work.
- Does not own: multi-node clustering, continuous PITR, MongoDB change streams,
  or automatic server updates. The capability page marks these as not built.
- Non-goal: publish a tag, push a branch, or open a pull request. Do not change
  release credentials or public package channels without owner approval.

## Invariants

1. A required condition is `pass` only when its named proof exists and contains
   its evidence anchor.
2. A skipped, absent, unsupported, or credential-gated lane stays
   `unverified` or `blocked`. It does not count as green.
3. Product smoke tests use the candidate build. Use installed releases only
   for explicit upgrade and distribution comparisons.
4. Tests use disposable data, control, application, and credential roots.
5. A confirmed defect gets a regression test at its owning seam before it
   closes.
6. Keep the three client mutation paths and all repository architecture
   invariants. A confirmed defect can require an explicit design task.
7. Proof files contain no credential, token, private key, or user data.
8. Public release actions require separate owner approval.

## Release Matrix

The machine-readable matrix has 46 fixed conditions in
`proof/release-readiness-2026-08/matrix.json`. The verifier rejects a removed,
renamed, duplicated, or unsupported condition.

| Dimension | Required coverage | Owning tasks |
|---|---|---|
| Core data | Tenant lifecycle, CRUD, pagination, schema, indexes, subscriptions, scheduler, auth | RRC2, RRC4 |
| Runtime | TypeScript functions, integrity, subscriptions, Node targets, permissions, diagnostics | RRC2, RRC3 |
| Adapters | Convex, Cloud Functions, Firestore, MongoDB, DynamoDB, S3, Cloudflare KV, RESP KV, native APIs, JavaScript SDK | RRC3 |
| Storage | SQLite, PostgreSQL, MySQL, libSQL, redb, encryption, backup, object plane | RRC4 |
| Workloads | Network control, resources, sandboxes, machines, Compose services | RRC5 |
| Desktop | Discovery, spawn, navigation, recovery, packaging, security, updater behavior | RRC6 |
| Distribution | Archives, install channels, OCI image, current published-release comparison | RRC7 |
| Closeout | Docs, dependency security, full CI, independent reviews | RRC1, RRC8 |

## Status Ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| RRC0 | Pin both repositories, hosts, published state, and the red release verifier. | `done` | `proof/release-readiness-2026-08/rrc0-baseline.md` |
| RRC1 | Audit source, advertised claims, dependency security, workflows, and release configuration. | `blocked(Deno WebSocket hook needs a reachable ref)` | `proof/release-readiness-2026-08/rrc1-audit.md`, `proof/release-readiness-2026-08/rrc1-capability-trace.md` |
| RRC2 | Smoke-test the candidate CLI, server, operator UI, core data, auth, scheduler, and diagnostics on macOS. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc2-product-smoke.md` |
| RRC3 | Run every application and protocol-adapter smoke lane, including browser-visible app flows. | `in_progress` | |
| RRC4 | Test storage providers, encryption, backup/restore, object storage, consistency, and restart recovery. | `todo` | |
| RRC5 | Test services, sandboxes, network policy, Compose, macOS machines, and Linux execution on `nimbus@minicloud`. | `todo` | |
| RRC6 | Test and repair the desktop app against the candidate server, including packaging and local Mac UI operation. | `todo` | |
| RRC7 | Validate archives, install paths, packages, OCI artifacts, upgrades, and current-release drift without publication. | `todo` | |
| RRC8 | Run final repository gates, repeat critical smoke tests, run Sol and Opus reviews, and issue the GO or NO-GO report. | `todo` | |
| RRC99 | Clean up this plan after the final repair pull request merges. | `todo` | Trigger: merge of the final release-readiness repair pull request. |

## Tasks

### RRC0 Pin the release baseline

- Problem: the repository has many independent proofs but no single candidate
  matrix for every advertised release surface.
- Owning seam and paths: this plan and
  `proof/release-readiness-2026-08/`.
- Steps:
  1. Pin Nimbus, desktop, macOS, Linux, toolchain, release, and hosted-CI state.
  2. Create the fixed 46-condition matrix and verifier.
  3. Run the verifier before any release evidence exists.
  4. Record the red count and candidate ancestry.
- Acceptance: the verifier reports `0 passed, 46 unverified` and exits nonzero.
- Acceptance: the baseline proof names both commit SHAs, both hosts, tool
  versions, latest published release, and current hosted check state.
- Fail-before: all 46 conditions start `unverified`.
- Verification: run
  `python3 docs/private/plans/proof/release-readiness-2026-08/verify.py`.

### RRC1 Audit code, claims, and release configuration

- Problem: tests cannot find every stale claim, unreachable branch, unsafe
  default, dependency issue, or workflow gap.
- Owning seam and paths: advertised source map, critical composition roots,
  manifests, release workflows, install scripts, and desktop security seams.
- Steps: trace each advertised capability to source and tests. Inspect all
  three mutation paths. Run dependency and secret gates. Compare claims to
  release configuration.
- Acceptance: each finding has a severity, source evidence, owner, and terminal
  verdict. No accepted release blocker remains open.
- Fail-before: record all initial mismatches before a repair.
- Verification: run `make deny`, attribution gates, workflow lint, desktop
  secret checks, and focused source audits.

### RRC2 Smoke-test the local candidate

- Problem: repository tests do not prove that an operator can start and use
  the release candidate as one system.
- Owning seam and paths: `nimbus-cli`, `nimbus-server`, the operator UI, and
  core engine routes.
- Steps: build the release candidate in debug and release forms. Use disposable
  roots. Test CLI discovery, lifecycle, and core data contracts. Use Playwright
  to operate the embedded UI.
- Acceptance: all documented product-smoke results are correct. This includes
  health, tenant lifecycle, CRUD, query, pagination, schema, indexes, WebSocket
  push, scheduling, auth rejection, diagnostics, shutdown, and restart.
- Fail-before: capture the first failing command or mark the baseline clean.
- Verification: run focused CLI/server checks and the recorded local smoke
  script against the candidate binary.

### RRC3 Test applications and protocol adapters

- Problem: the public protocols and example apps can drift while unit tests
  stay green.
- Owning seam and paths: `scripts/examples-verify-cases.json`, adapter crates,
  packages, and example applications.
- Steps: run all nine repository application cases. Exercise each advertised
  adapter with its official client. Use Playwright for browser flows.
- Acceptance: every example anchor passes and cleanup succeeds. Each adapter,
  native API, and JavaScript SDK has direct candidate evidence.
- Fail-before: retain the first failing application artifact tree.
- Verification: run `NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL=5 make examples-verify`
  plus each repository-owned adapter lane.

### RRC4 Test storage and recovery

- Problem: release readiness needs live provider, recovery, encryption, backup,
  object, and consistency evidence.
- Owning seam and paths: `nimbus-storage`, `nimbus-object-storage`,
  `nimbus-blob`, backup commands, encryption commands, and consistency routes.
- Steps: test embedded providers locally. Use repository fixtures for external
  providers. Test backup, restore, key rotation, and object administration.
  Inject supported crash or durability faults.
- Acceptance: all available storage rows in the capability page have direct
  evidence or a release-blocking `UNVERIFIED` state. Restart and restore retain
  the expected durable head and materialized position.
- Fail-before: capture each provider or recovery failure before a repair.
- Verification: run the full storage matrix with explicit provider features,
  physical durability tests, storage harnesses, and operator commands.

### RRC5 Test workloads on macOS and Linux

- Problem: workload claims depend on host capabilities that unit tests cannot
  fully model.
- Owning seam and paths: network, services, sandbox, machine, Compose, node,
  proxy, and SDK resource seams.
- Steps: test the macOS machine path, test Linux container and libkrun paths on
  `nimbus@minicloud`, verify deny-by-default egress, and test lifecycle cleanup.
- Acceptance: network control, resource APIs, sandbox backends, machines, and
  Compose services have live candidate evidence. Unsupported host cells stay
  explicit.
- Fail-before: retain the first failed host proof and cleanup status.
- Verification: run host checks and repository-owned machine, service, VMM,
  egress, and node proof lanes on their supported host.

### RRC6 Test the desktop app

- Problem: the separate Electron shell can fail discovery, process lifecycle,
  navigation, recovery, packaging, security, or update behavior.
- Owning seam and paths: `/Users/jack/src/github.com/nimbus/desktop` and the
  candidate server's discovery and UI routes.
- Steps: run desktop static and packaged-shell gates. Start the app against a
  disposable candidate server. Operate the Mac app. Test reconnect and error
  states.
- Acceptance: lint, typecheck, unit, E2E, package, fuse, bounds, updater, and
  secret gates pass. Computer-use evidence proves local launch, discovery,
  operator navigation, server loss, recovery, and clean quit.
- Fail-before: record the first failed desktop gate or UI action.
- Verification: run the desktop package scripts and packaged Playwright tests,
  then inspect the real app with local computer control.

### RRC7 Validate distribution artifacts

- Problem: source correctness does not prove archives, installers, packages,
  images, and upgrade paths.
- Owning seam and paths: local release workflow inputs and distribution helper
  scripts. Public channel ownership stays in `distribution-plan.md`.
- Steps: build candidate archives and packages. Verify archive and license
  layout. Smoke-test the OCI image. Test local install and upgrade flows.
  Compare the candidate with the latest published release.
- Acceptance: every locally testable release artifact passes its owning
  verifier. Public apt, COPR, cloud-image, and tag work stays routed to the
  distribution plan with an explicit status.
- Fail-before: retain the first invalid artifact and verifier output.
- Verification: run release archive, install, Linux package, OCI image, and
  desktop installer gates.

### RRC8 Close the release candidate

- Problem: repaired slices need one final integrated result and an independent
  review.
- Owning seam and paths: both candidate repositories and this proof root.
- Steps: rerun critical smoke cases and complete gates on macOS and Linux.
  Update all matrix evidence. Run Sol xhigh and Opus reviews. Write a release
  verdict.
- Acceptance: all 46 matrix conditions pass. Nimbus and desktop gates pass.
  Required host lanes pass. No accepted P0 through P2 review finding remains.
- Fail-before: the matrix stays red until the last required proof closes.
- Verification: run the matrix verifier, `make ci`, affected nightly harnesses,
  desktop checks, Nimbus autoreview, and the independent Opus review.

### RRC99 Cleanup

- Problem: a merged plan must not remain an active control plane.
- Owning seam and paths: this plan, its proof root, and the plans index.
- Steps: confirm the final merge, then delete or archive the plan and update the
  index.
- Acceptance: the plans index has no active entry for this completed campaign.
- Fail-before: not applicable because the merge triggers this task.
- Verification: search for `release-readiness-2026-08` and confirm the final
  routing.

## Goal

```text
Execute docs/private/plans/release-readiness-2026-08-plan.md to completion.
This is a whole-plan goal, not a single-task goal. Read the plan fully, then
read AGENTS.md, README.md, ARCHITECTURE.md,
docs/private/operating/verification.md, docs/reference/current-capabilities.md,
docs/private/plans/distribution-plan.md, and both repositories' release and
security instructions. Work in /Users/jack/src/github.com/nimbus/nimbus on
branch codex/release-readiness-2026-08. Coordinate desktop fixes in
/Users/jack/src/github.com/nimbus/desktop on a matching codex branch when a
confirmed defect requires a change. Chat history is not progress state. Resume
from the status ledger, execution log, matrix, and git state. If compaction
happens, continue from those files rather than restarting. Loop: keep one task
in_progress, test the owning seam, capture fail-before evidence, fix each
confirmed defect, run the verification commands, commit the work, write the
proof file, append the execution log, mark the task terminal with evidence,
commit the plan update, then advance to the next task. Decide rather than ask.
Mark a wrong or already-satisfied task no-action with a one-line reason. Record
a blocker and continue with the next eligible task. Binding constraints: keep
all eight invariants, do not count skips as passes, and do not publish, tag,
push, open a pull request, change credentials, or alter public package channels
without owner approval. Commit policy: local commits are permitted in both
repositories. Keep unrelated work unchanged. Stop only at a valid stop state
from the plans skill. Before stopping, update the ledger, log, matrix, and next
action. The goal is met when RRC0 through RRC8 are terminal, all 46 conditions
pass with durable evidence, no accepted release blocker remains, final reviews
are clean, and RRC99 waits only for merge.
```

## Execution Log

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-27 | RRC0 | started | Created the stacked candidate branch from storage-repair head `1403bc780`. No production behavior changed. |
| 2026-08-27 | RRC0 | done | Pinned both repositories, both hosts, toolchains, release state, hosted failures, and the red verifier in `proof/release-readiness-2026-08/rrc0-baseline.md`. |
| 2026-08-27 | RRC1 | started | Began the source, claim, security, workflow, and release-configuration audit. |
| 2026-08-27 | RRC1 | finding | Recorded eleven source, dependency, workflow, canary, and cleanup findings in `proof/release-readiness-2026-08/rrc1-audit.md`. RRC1-010 confirms that `new WebSocket()` does not use the production tenant `EgressGateway`; repair is in progress. |
| 2026-08-27 | RRC1 | checkpoint | Committed the workflow, JavaScript, UI, canary, desktop dependency, and vendored warning repairs in Nimbus commits `adcfa82ce`, `2f43b116f`, and `0e15e58aa`, desktop commit `4ee5c83`, and Deno commit `4136492f7f`. The capability trace covers all 42 advertised rows and all three client mutation paths. |
| 2026-08-27 | RRC1 | blocked | The WebSocket egress repair passes focused tests and full Clippy with local Deno paths. A clean Nimbus dependency cannot consume Deno commits `5d07e09121` and `4136492f7f` until they have a reachable ref. This plan does not authorize a push or tag. |
| 2026-08-27 | RRC2 | started | Began the macOS candidate build and end-to-end product smoke while the independent RRC1 publication blocker remains recorded. |
| 2026-08-27 | RRC2 | finding | The embedded-UI smoke exposed three stale test contracts: an incomplete system-service fixture, old Operator System identifiers, and an implicit only-tenant assumption. The repaired test uses the current schema, Operator Nodes page, and real tenant selector. TypeScript and the 10-step Chromium smoke pass. See `proof/release-readiness-2026-08/rrc2-product-smoke.md`. |
| 2026-08-27 | RRC2 | finding | Public-bind refusals returned Rust enum debug text instead of the existing actionable operator messages. Commit `c7a27c74b` fixes top-level CLI error rendering and adds a subprocess regression test. |
| 2026-08-27 | RRC2 | blocked | Debug and optimized provisional binaries pass the full product smoke, graceful shutdown, same-root restart, and cleanup. Exact-candidate replay waits only for the RRC1 Deno reference. See `proof/release-readiness-2026-08/rrc2-product-smoke.md`. |
| 2026-08-27 | RRC3 | started | Began all nine repository application cases and adapter lanes with the repaired provisional binary and Node 24.19.0. |
