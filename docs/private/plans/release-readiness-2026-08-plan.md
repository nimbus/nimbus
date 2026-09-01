# Nimbus Release Readiness 2026-08

Status: `active`.
Owner: this plan.
Created: 2026-08-27.
Baseline branch: `codex/storage-review-repairs`.
Baseline commit: `1403bc780`.
Baseline upstream: `origin/main` at `b57a2d680`.
Proof root: `proof/release-readiness-2026-08/`

Next action: record the Nimbus candidate commit that consumes public tags
`v2.9.6-nimbus.1` and `v150.4.0-nimbus.1`. Repeat the smoke, archive, OCI, and
higher-memory Linux full-LTO lanes from that commit, then update the release
verdict.

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
- Non-goal: publish a Nimbus product tag, release, or package. The owner
  authorized repository updates and the fork releases needed to produce a
  reproducible dependency graph. Do not change Nimbus release credentials or
  public product package channels without owner approval.

## Invariants

1. A required condition is `pass` only when its named proof exists and contains
   its evidence anchor.
2. A skipped, absent, unsupported, or credential-gated lane stays
   `unverified` or `blocked`. It does not count as green.
3. Product smoke tests use the candidate build. Use installed releases only
   for explicit upgrade and distribution comparisons.
4. Tests use disposable data, control, application, and credential roots.
5. A confirmed defect gets a regression test at its owning boundary before it
   closes.
6. Keep the three client mutation paths and all repository architecture
   invariants. A confirmed defect can require an explicit design task.
7. Proof files contain no credential, token, private key, or user data.
8. Public Nimbus product release actions require separate owner approval.

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
| RRC1 | Audit source, advertised claims, dependency security, workflows, and release configuration. | `done` | Deno PR #1 at `1c17e86b29`; Nimbus `f0725ac57`; `proof/release-readiness-2026-08/rrc1-audit.md`, `proof/release-readiness-2026-08/rrc1-capability-trace.md` |
| RRC2 | Smoke-test the candidate CLI, server, operator UI, core data, auth, scheduler, and diagnostics on macOS. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc2-product-smoke.md` |
| RRC3 | Run every application and protocol-adapter smoke lane, including browser-visible app flows. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc3-app-adapter-smoke.md` |
| RRC4 | Test storage providers, encryption, backup/restore, object storage, consistency, and restart recovery. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc4-storage-recovery.md` |
| RRC5 | Test services, sandboxes, network policy, Compose, macOS machines, and Linux execution on `nimbus@minicloud`. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc5-workload-hosts.md` |
| RRC6 | Test and repair the desktop app against the candidate server, including packaging and local Mac UI operation. | `blocked(exact-candidate replay and notarization remain; provisional pass)` | `proof/release-readiness-2026-08/rrc6-desktop.md` |
| RRC7 | Validate archives, install paths, packages, OCI artifacts, upgrades, and current-release drift without publication. | `blocked(exact-candidate replay and public apt/COPR proofs remain; provisional pass)` | `proof/release-readiness-2026-08/rrc7-distribution.md` |
| RRC8 | Run final repository gates, repeat critical smoke tests, run Sol-only reviews, and issue the GO or NO-GO report. | `in_progress` | `proof/release-readiness-2026-08/rrc8-release-verdict.md` |
| RRC99 | Clean up this plan after the final repair pull request merges. | `todo` | Trigger: merge of the final release-readiness repair pull request. |

## Tasks

### RRC0 Pin the release baseline

- Problem: the repository has many independent proofs but no single candidate
  matrix for every advertised release surface.
- Owning components and paths: this plan and
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
- Owning components and paths: advertised source map, critical composition roots,
  manifests, release workflows, install scripts, and desktop security boundaries.
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
- Owning components and paths: `nimbus-cli`, `nimbus-server`, the operator UI, and
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
- Owning components and paths: `scripts/examples-verify-cases.json`, adapter crates,
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
- Owning components and paths: `nimbus-storage`, `nimbus-object-storage`,
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
- Owning components and paths: network, services, sandbox, machine, Compose, node,
  proxy, and SDK resource boundaries.
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
- Owning components and paths: `/Users/jack/src/github.com/nimbus/desktop` and the
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
- Owning components and paths: local release workflow inputs and distribution helper
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
- Owning components and paths: both candidate repositories and this proof root.
- Steps: rerun critical smoke cases and complete gates on macOS and Linux.
  Update all matrix evidence. Run Sol xhigh reviews while the owner restriction
  applies. Write a release verdict.
- Acceptance: all 46 matrix conditions pass. Nimbus and desktop gates pass.
  Required host lanes pass. No accepted P0 through P2 review finding remains.
- Fail-before: the matrix stays red until the last required proof closes.
- Verification: run the matrix verifier, `make ci`, affected nightly harnesses,
  desktop checks, and Nimbus autoreview with Sol. Do not use Opus 5 or Fable
  until the owner explicitly lifts the restriction.

### RRC99 Cleanup

- Problem: a merged plan must not remain an active control plane.
- Owning components and paths: this plan, its proof root, and the plans index.
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
in_progress, test the owning boundary, capture fail-before evidence, fix each
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
| 2026-08-27 | RRC3 | finding | Fixed default-SQLite Cloudflare KV support and exact final-page reporting in `4de2b28bb`, `b63addb52`, and `6ca9321ee`; live auth, metadata, pagination, delete, and restart-durability checks pass. |
| 2026-08-27 | RRC3 | finding | Fixed the split CORS/origin policy in `5caf5c2cf` and portable browser packaging, origin defaults, Convex provisioning, run instructions, and favicon cleanup in `ef589a825`. All three Playwright task apps pass their lifecycle with zero console diagnostics. |
| 2026-08-27 | RRC3 | blocked | All nine application cases, direct S3, RESP, Cloudflare KV, and three browser flows pass on the provisional integrated binary. Exact-candidate replay waits only for the RRC1 Deno reference. See `proof/release-readiness-2026-08/rrc3-app-adapter-smoke.md`. |
| 2026-08-27 | RRC4 | started | Began the storage-provider, encryption, backup/restore, object, consistency, and restart-recovery matrix while the independent RRC1 publication blocker remains recorded. |
| 2026-08-27 | RRC4 | finding | Encrypted SQLite backup produced a false corruption diagnostic, and quiesce stopped the committer before the trigger-candidate worker. Commit `965bfd379` adds the encrypted cold-copy rejection and repairs the shutdown order with regressions. |
| 2026-08-27 | RRC4 | finding | redb backup and restore ignored a separate control root. Commit `464e30596` adds the explicit control-root contract, regression coverage, and public operator documentation. |
| 2026-08-27 | RRC4 | finding | Object placement, backup, restore, and removal ignored a separate control root. Commit `652fb93b6` repairs all four administration paths and adds split-root live and automated recovery evidence. |
| 2026-08-27 | RRC4 | blocked | All provider, encryption, backup/restore, object, consistency, durability, restart, and process-fence lanes pass provisionally. Exact-candidate replay waits only for RRC1. See `proof/release-readiness-2026-08/rrc4-storage-recovery.md`. |
| 2026-08-27 | RRC5 | started | Began the macOS and `nimbus@minicloud` workload-host matrix while the independent RRC1 publication blocker remains recorded. |
| 2026-08-27 | RRC5 | finding | The macOS host pinned machine-os 0.1.30 while Nimbus 0.1.45 and machine-os 0.1.45 were published. Commit `e03a010a4` pins the exact matching image and digest. |
| 2026-08-27 | RRC5 | finding | Direct `machine stop` always withheld canonical Engine authority, so failed starts could not release their SSH lease. Commit `e03a010a4` supplies the selected persistence contract only for fenced effects and isolates all proof roots. |
| 2026-08-27 | RRC5 | finding | Bootc guests already consume the mounted authority and start the baked machine API, but the host re-entered legacy SSH stop/install/restart convergence. Commit `e0a68be28` waits for the bootc-owned API and keeps SSH convergence only for legacy images. The published 0.1.45 image then passed the complete Mac lifecycle proof. |
| 2026-08-27 | RRC5 | hardening | machine-os created a passwordless `nimbus` account in `wheel` without a noninteractive sudo rule. Local machine-os commit `d5752a4` adds the explicit sudo package, 0440 rule, package inventory, and recipe checks. No artifact was published. |
| 2026-08-27 | RRC5 | finding | Live KVM tests found that resource limits used an obsolete rootfs sidecar, image users were not applied inside the guest, reserved-before-spawn teardown could exhaust retry, and provider-assigned ingress treated port zero as a private bridge and terminal-publication value. Commit `6ca8eb981` fixes all four production contracts and adds exact unit and live regressions. |
| 2026-08-27 | RRC5 | hardening | Commit `6ca8eb981` gives the KVM smoke lane isolated supernet and host-port inputs, complete scratch-image runtime libraries, guest-driven liveness transitions, failure compensation, process-qualified identities, and exact teardown retry recovery. |
| 2026-08-27 | RRC5 | evidence | The serial Linux KVM matrix passed 8/8 in 185.05 seconds with the staged current crun/libkrun tuple. It proves image and direct-rootfs launches, USER/STOPSIGNAL, CPU/RAM, three concurrent provider-assigned ports, readiness, liveness withdrawal/recovery without restart, and exact cleanup. Local `nimbus-sandbox` evidence passed 1,215 tests with zero failures. |
| 2026-08-27 | RRC5 | finding | The container-only live egress target failed under warning denial because shared support exposed a KVM-only address accessor. A method-local allowance records the cross-target ownership; the target now compiles on macOS and Linux. |
| 2026-08-27 | RRC5 | evidence | The root-only Linux container matrix passed 2/2 in 49.61 seconds. It proved direct-egress denial, exact L7 admission, denied bypasses, live policy reload, and exact process/mount/namespace cleanup. |
| 2026-08-27 | RRC5 | finding | The archived SDK resource verifier had six stale ownership/layout anchors, and the compiler-authority refresh mixed a small tmpfs with a shared feature-variant Cargo target. The SDK verifier now passes 23/23; compiler scan isolation, free-space preflight, and 18/18 mutation self-tests pass. Portable baseline refresh remains routed to the existing RRC1 reachable-Deno blocker. |
| 2026-08-28 | RRC5 | finding | The public resource API returned a truthful pending receipt but discarded its driver task, stop returned before terminal teardown, stopped-successor recovery rejected an already-retired source, empty endpoint withdrawal was rejected, KVM attachment adoption could not resume, and pre-activation compensation crossed a missing Drain command. The current RRC5 repair set adds retained supervision, settlement waits, exact recovery and empty-publication rules, resumable adoption, and durable no-execution stop fences for both backends. |
| 2026-08-28 | RRC5 | finding | `nimbus start --compose-file` resolved and logged an exact workload boot plan but never submitted it. The server now applies the validated ordered service plan after durable workload recovery and before it begins serving. A fresh Linux root automatically launched the declared KVM service without a lifecycle POST. |
| 2026-08-28 | RRC5 | evidence | The final provisional binary (`283295f9d80558ec55e5c0523b40e3d04b0b5d29a803c2a504ed932ccac6285d`) passed public sandbox and session lifecycle, GET-only convergence, automatic Compose boot, exact port release, focused compensation tests, 1,218 sandbox library tests, and 2/2 live node/systemd D-Bus tests. Controlled provider faults remained quarantined with no creator admitted. |
| 2026-08-28 | RRC5 | blocked | Every supported workload-host lane has a provisional pass. Exact-candidate replay waits only for RRC1's reachable immutable Deno references. See `proof/release-readiness-2026-08/rrc5-workload-hosts.md`. |
| 2026-08-28 | RRC6 | started | Began desktop static, unit, packaged-shell, installer, and real Mac UI validation against the provisional integrated Nimbus candidate. |
| 2026-08-28 | RRC6 | finding | Fixed fixed-port startup, early child-exit reporting, packaged GUI data-root failure, stale-session recovery, stale DS1 and shortcut expectations, missing fuse invocation, native branding, and incomplete DS3 child cleanup. Nimbus commits `9fb527763` and `92fbd821f` and desktop commits `618707d`, `21800be`, `de63d23`, and `bbc103f` contain the repairs and regressions. |
| 2026-08-28 | RRC6 | evidence | Nimbus UI passed lint, typecheck, 845 tests, and 14 local-UI server tests. Desktop passed lint, typecheck, 186 unit tests, 5 E2E tests, DS1 through DS6, six fuse checks, strict signing verification, universal-architecture inspection, DMG verification, real sign-in/navigation/loss/recovery/quit, and exact owned-child cleanup. See `proof/release-readiness-2026-08/rrc6-desktop.md`. |
| 2026-08-28 | RRC6 | review | Opus 5 found two Nimbus P3 test gaps and three accepted desktop P3 hardening gaps. All were repaired. Follow-up reviews of Nimbus `92fbd821f` and desktop `bbc103f` accepted no P0 through P3 finding. |
| 2026-08-28 | RRC6 | blocked | The repaired desktop matrix has a provisional pass. Exact-candidate replay waits for RRC1's reachable immutable Deno references. Apple notarization and stapling also remain unverified because authorized API credentials are unavailable locally. |
| 2026-08-28 | RRC6 | cleanup | Preserved the verified provisional server binary by SHA-256 and removed its rebuildable 51.2 GiB temporary Cargo target. Free macOS data-volume space increased from 56 GiB to 105 GiB. Source, proofs, and desktop installers were unchanged. |
| 2026-08-28 | RRC7 | started | Began archive, direct-install, package, OCI-image, upgrade, and current-published-release validation without publication. |
| 2026-08-28 | RRC7 | finding | The live verifier required an unsupported Windows asset; direct installs discarded release documents and could use a different PATH channel for same-version and document checks; the repository release version collided with public v0.1.45; and the scaffold-lock version gate had unsafe scan boundaries. Commits `dfea25523`, `52b8fc93b`, `e771e5fac`, `7e9a48ad6`, and `0c8bdd363` contain the repairs and regressions. |
| 2026-08-28 | RRC7 | evidence | All deterministic archive, installer, package, OCI, version, syntax, ShellCheck, and Homebrew gates pass. Public v0.1.45 passed checksum, archive, license, attestation, SBOM, vulnerability, pull, and OCI runtime verification. Fresh Debian 13 and Fedora 42 containers installed real v0.1.44 package tuples and upgraded to v0.1.45. A fresh Debian container passed direct install, verification, upgrade, and uninstall. See `proof/release-readiness-2026-08/rrc7-distribution.md`. |
| 2026-08-28 | RRC7 | review | Opus 5 found and drove repairs for changelog preservation, prefix cleanup ownership, version-lock scan scope, document ownership, negative fixtures, malformed tracked locks, and nested dependency false positives. The final review of `0c8bdd363` accepted no actionable finding, and secret scans were clean. |
| 2026-08-28 | RRC7 | blocked | Every locally testable distribution artifact has a provisional pass. Exact v0.1.46 replay waits for reachable immutable Deno references. Public apt and COPR install proofs remain with the distribution plan and require separate publication authority. No public state changed. |
| 2026-08-28 | RRC8 | started | Began the final integrated repository, host, evidence-matrix, and independent-review closeout. The fail-before matrix has 0 passed, 43 unverified, and 3 blocked conditions with zero structural errors. |
| 2026-08-29 | RRC8 | finding | The final egress audit found a DNS-rebinding gap after hostname preflight. The Deno and Nimbus integration now reauthorizes every concrete fetch and WebSocket address before connect, rejects unverifiable proxy and custom-client paths, and proves denied loopback connections never reach the listener. |
| 2026-08-29 | RRC8 | evidence | Exact local-Deno focused tests, runtime and bridge checks, the 14-condition connection-broker verifier, formatting, attribution, and provenance gates pass. The final Deno integration revision is `1c17e86b296af380f67c48f3b9a89876db154604`. |
| 2026-08-29 | RRC8 | review | Opus 5 high and Sol xhigh independently reviewed the final uncommitted Nimbus integration after the last correction. Both reported no accepted or actionable P0 through P3 finding, and secret scans were clean. |
| 2026-08-29 | RRC8 | blocked | Issued a NO-GO verdict in `proof/release-readiness-2026-08/rrc8-release-verdict.md`. The fixed matrix is 3 pass, 43 blocked, 0 unverified, 0 failed, and 0 structurally invalid. A reachable immutable Deno ref, exact clean candidate and CI replay, notarization, and public apt/COPR proof remain required. No public state changed. |
| 2026-08-29 | RRC8 | cleanup | Removed the disposable exact-test worktree and 13 GiB rebuildable Cargo target after the reviews. Free filesystem space increased from 86 GiB to 100 GiB; candidate and desktop release artifacts remain preserved. |
| 2026-08-29 | RRC8 | resumed | The owner authorized updates to all repositories under `~/src/github.com/nimbus/`. Pushed Deno branch `codex/release-readiness-websocket-egress`; remote revision `1c17e86b296af380f67c48f3b9a89876db154604` removes the exact-candidate dependency blocker. |
| 2026-08-29 | RRC1 | done | Opened Deno PR #1 and pinned every Nimbus Deno patch to remote commit `1c17e86b296af380f67c48f3b9a89876db154604`. Nimbus commit `f0725ac57` contains the reviewed connection-broker repair. The remote exact graph passes locked runtime and bridge checks, 2/2 denial-before-connect tests, 3/3 gateway allow-path tests, 2/2 bridge internal-address denials, 1/1 embedded-anchor install, both anchor provenance checks, and the 14/14 connection-broker verifier. |
| 2026-08-29 | RRC8 | finding | The first exact `make ci` run stopped in Clippy because direct Cloud Functions and Convex server test builders did not initialize the new resolved-address field. Commit `e39e4f020` adds the explicit `None` value to both fixtures. Focused Clippy passes, and both seven-test egress suites pass. |
| 2026-08-29 | RRC8 | fail-before | The next exact `make ci` reached the workspace Rust lane: 7,664 of 7,719 tests passed, 55 failed, and 111 skipped. Fifty-four CLI and server tests aborted on the standard 2 MiB test-thread stack; one compute cancellation test exposed a second foreground provision polling owner. The run also reported one sandbox leak marker. |
| 2026-08-29 | RRC8 | finding | LLDB showed large inline router preparation and service-retirement futures exhausting the standard stack. Router startup now heap-bounds its three preparation phases. Service lifecycle verbs have separate futures, and public retirement waits run as abort-on-drop tasks with caller cancellation while durable teardown remains retained. The server package passes 763 of 763 tests with no leak marker; the CLI package passes 1,070 of 1,070, and its one prior auth leak marker does not reproduce in the exact focused test. |
| 2026-08-29 | RRC8 | finding | The foreground service facade duplicated the provisioner's retained retry supervisor. It now joins the exact settlement channel instead of polling every 25 ms. The full compute package passes 514 of 514 tests with one intentional skip, and focused Clippy passes for compute, server, and CLI with warnings denied. |
| 2026-08-29 | RRC8 | fail-before | Full `make ci` from `a61fe308c` passed formatting, workspace Clippy, and dependency policy, then stopped in the isolated runtime lane. One subprocess host-start wait exceeded the 15-second local budget under parallel load; 524 tests passed, one failed, and 134 were intentionally ignored. The exact subprocess passed in 0.72 seconds. |
| 2026-08-29 | RRC8 | finding | The host-start helper already waits on the exact host-request notification and verifies the started-ID map. No missing readiness boundary exists. The 30-second local budget is a deliberate integrated-load allowance below the existing 60-second CI budget, not a product startup limit; the explicit environment override remains. The exact subprocess passes in 0.72 seconds, the full runtime lane passes 525 tests with 134 intentional ignores, and runtime Clippy passes with warnings denied. |
| 2026-08-29 | RRC8 | evidence | Exact `make ci` from `429e3afa4` exited zero. The runtime lane passed 525 tests with 134 intentional ignores. The workspace lane passed 7,719 of 7,719 tests with 111 intentional skips. Doctests, required generated-history and transport campaigns, JavaScript builds and typechecks, 846 UI tests, security and package proof helpers, and installer checks passed. |
| 2026-08-29 | RRC8 | investigation | The workspace lane reported two passed tests as leaky. Each passed without a leak marker in a package-only run and in five repeated workspace-feature runs, for 12 clean focused executions. Treat the markers as unconfirmed runner observations; the final full gate must report zero leak markers. |
| 2026-08-29 | RRC8 | cleanup | Node 26 exposed two non-blocking diagnostics outside the supported Node 22 and 24 application range. The NodeFull snapshot mismatch is the expected `cfg(test)` safety path, and its production provenance check passes. Firebase code generation now disables the unused experimental Web Storage global only for child tools when Node supports the opt-out; traced generation and the package typecheck pass. Tailwind 4.3.3 fixes its upstream loader deprecation, but this session cannot regenerate dependency locks because package-manager writes require unavailable approval. |
| 2026-08-29 | RRC8 | evidence | Exact `make ci` from `963af1d3b` exited zero. Formatting, workspace Clippy, dependency policy, production runtime provenance, 525 runtime tests, 7,719 workspace tests, doctests, required harnesses, JavaScript builds and typechecks, 846 UI tests, proof helpers, and 60 installer checks passed. The 134 runtime ignores and 111 workspace skips are the declared suite inventory, not silent release evidence. Firebase child tools emitted no Web Storage diagnostic. The remaining Tailwind loader warning occurs only on the unsupported local Node 26 runtime. |
| 2026-08-29 | RRC8 | investigation | The final workspace lane marked the passed `two_krun_drain_contenders_publish_one_barrier` test as leaky. Its drain path starts no child process and joins both contender threads before return. The exact test passed ten repeated workspace-feature runs without a leak marker. Three different passed tests have now received isolated, nonreproducible markers only under full-suite load; no verified product or test-resource leak remains. |
| 2026-08-29 | RRC8 | review | The bounded final-delta Sol xhigh and Opus 5 panel returned three distinct findings. The foreground-cancellation assertion was an accepted P3 test gap. The host timeout finding stopped one layer before the existing semantic notification and is refuted with the deliberate allowance recorded above. The Deno revision is valid for integration, but release reproducibility requires PR #1 to merge and a release-authorized annotated fork tag before shipment. |
| 2026-08-29 | RRC8 | finding | The foreground cancellation proof now observes a provider retry after the waiter returns, releases the controlled provider, requires `Observed` and `Projected` terminal truth, and verifies that the retained supervisor retires. The regression passes, the compute library passes 514 tests with one declared ignore, and all-feature compute Clippy passes with warnings denied. |
| 2026-08-29 | RRC8 | review | The exact repair follow-up found that a fast terminal supervisor could remove its registry entry before the test subscribed, which made the terminal projection assertion conditional. Both reviewers reported that one accepted race; Opus also requested explicit ordering for the final task-liveness assertion. |
| 2026-08-29 | RRC8 | finding | The test now takes the exact settlement receiver before provider release and requires its terminal result. Settlement publication and registry removal use one supervisor mutex, which orders the final task-liveness check. The focused regression, all 514 active compute tests, and warning-denied all-feature compute Clippy pass. |
| 2026-08-29 | RRC8 | review | The exact compute rereview is clean after Opus received the owning supervisor implementation as explicit unchanged evidence. The full storage-repair range panel then found two accepted P3 defects: two PITR proof callers supplied a silently discarded fault point, and candidate-only IMV benchmarking accepted but ignored `--full-samples`. |
| 2026-08-29 | RRC8 | finding | Non-advanced PITR cases now pass no committed fault point, while both advanced cases must select one. The IMV argument parser now tracks an explicit full-sample option and rejects it in candidate-only mode. Three parser regressions, four real PITR durable-outcome tests, benchmark compilation, and warning-denied benchmark and regression Clippy pass. |
| 2026-08-29 | RRC8 | review | The storage repair follow-up panel is clean. The release-campaign bootstrap panel re-reported three intermediate libSQL concerns that current commit `f7392e382` and RRC1 evidence already close. Its separate P1 candidate-binding finding is accepted: the matrix verifier checked evidence anchors but not exact candidate identity. |
| 2026-08-29 | RRC8 | finding | Every passing matrix proof must now contain the exact Nimbus, Deno, and upstream-baseline SHAs. Desktop and independent-review proofs must also contain the exact Desktop SHA. Four verifier mutation tests pass. The current matrix remains truthfully red at 3 passes and 43 blockers with zero structural errors. |
| 2026-08-29 | RRC8 | review | The candidate-binding follow-up found that a SHA anywhere in a proof file could bind every cited anchor and that the tests did not assert the verifier return code. Both findings are accepted. |
| 2026-08-29 | RRC8 | finding | Candidate binding is now scoped from the exact cited Markdown heading to the next peer section. The all-green test fixture must return zero, rejection fixtures must return nonzero, and an unrelated header SHA cannot bind evidence. Five verifier tests pass; the live red matrix retains zero structural errors. |
| 2026-08-29 | RRC8 | review | The section-scoping follow-up found that heading-like text inside fenced or indented code could create or truncate evidence sections. It also found that the revised all-pass test fixture no longer proved that blocked rows need no candidate binding. Both findings are accepted. |
| 2026-08-29 | RRC8 | finding | The verifier now tracks Markdown fence state and excludes indented code from heading discovery. Eight tests cover exact bindings, stale bindings, header-only SHAs, fenced and indented pseudo-headings, Desktop binding, malformed SHAs, exit codes, and blocked rows. The live matrix retains zero structural errors. |
| 2026-08-29 | RRC8 | review | The Markdown follow-up found three accepted parser edges: unclosed fences could hide peer headings, backtick fence info accepted forbidden backticks, and mixed space-tab indentation could create a false heading. |
| 2026-08-29 | RRC8 | finding | Unclosed fences now make proof structure invalid. Backtick fences reject forbidden backticks in their info strings, and heading indentation uses Markdown tab-stop columns. Eleven verifier tests pass, and the live matrix retains zero structural errors. |
| 2026-08-29 | RRC8 | review | The malformed-Markdown follow-up accepted one P3 diagnostic issue: an unterminated fence failed closed but reported an anchor-count error. |
| 2026-08-29 | RRC8 | finding | Unterminated fences now produce a specific proof-structure diagnostic. All 11 verifier tests pass, and the live matrix retains zero structural errors. |
| 2026-08-29 | RRC8 | review | Two additional Deno review rounds found and closed checker-less client churn, a tautological proxy regression, custom-client contract drift, checker-bearing client churn, and checker-equivalence reuse risks. `ResolvedAddressChecker` now supports an opaque equivalence key, Deno retains a bounded 16-entry client cache, Nimbus derives the key from the complete unresolved egress request, and unverifiable checker-bearing custom clients fail closed. The final disputed `WebSocketStream` custom-client finding was refuted from its JavaScript and TypeScript surface, which has no client option. |
| 2026-08-29 | RRC8 | evidence | Deno PR #1 merged at `d0a6b9094e0da6acbb53ecd0d88ed6b81a142e63`. Candidate, branch, and annotated-tag CI runs `33248587436`, `33249117117`, and `33249117869` passed. Public non-draft release `v2.9.3-nimbus.2` is immutable at that commit. Nimbus's tag-based checkpoint passes runtime compilation, 16 fetch egress tests, 5 runtime bootstrap tests, warning-denied runtime Clippy, both fork provenance policies, and the 14-condition connection-broker verifier. |
| 2026-08-29 | RRC8 | blocked | The previously unreachable Deno-ref blocker is closed, but the tracks-latest standardization gate detects upstream `v2.9.6` while Nimbus consumes `v2.9.3-nimbus.2`. Upstream also changed the runtime boundary to `deno_v8` 0.3.0 over V8 150.4. RRC8 remains NO-GO while the fork is uplifted, reviewed, released, repinned, and replayed as the exact candidate. |
| 2026-08-29 | RRC8 | decision | Independent source review accepted the proposed runtime-strategy lifecycle and kept RRC8 as the current owner. U3 will omit proven realm-only carries, pair the omission with Nimbus consumer cleanup before U4, and complete the exact replay-scaffolding A/B before U5. The lifecycle plan can activate only after terminal U6 evidence and owner approval. No RSL implementation started. |
| 2026-08-29 | RRC8 | evidence | U1 completed at rusty_v8 `dbb70a973d28cfe8cd6a2ea66d4f3d14fee488f0`. Source-built nextest passes 308 tests across 25 binaries; 13 documentation tests, warning-denied Clippy, formatting, 15 release-tool tests, action syntax, the 44-asset manifest, and the final Sol xhigh review pass. Two review P2 findings closed stale offline binding reuse and mutable release action references. |
| 2026-08-29 | RRC8 | constraint | The owner prohibited Opus 5 and Fable for review because credits are low. The active Opus process stopped with exit 130. Final V8 acceptance uses the exact-commit Sol review and local gates. U2 push, tag, and publication remain unstarted pending explicit authorization; U3 can continue from its clean worktree. |
| 2026-08-29 | RRC8 | evidence | U3 now has six clean local Deno commits ending at `e8fffe9029283b5f51111647ce5e2a79eadf5ef2`. The paired Nimbus cleanup removes the rejected fresh-realm product path, fixes guest-visible runtime versions and snapshot parsing, and updates current runtime verifiers. Focused exact-candidate suites and 43 shell-verifier checks pass. Controlled A/B evidence and the Sol-only final review remain. |
| 2026-08-29 | RRC8 | evidence | The selected candidate crossover passes with a live Node22 anchor: startup snapshot 8.304–8.582 ms versus exact warm pool 2.524–2.631 ms; Web unsnapshotted cache 19.635–19.815 ms versus exact warm pool 1.213–1.259 ms. The controlled old-graph replay A/B finds no Web change, a small favorable Node replay-off result after counterbalancing, and only 16 replay-table bytes in the 14,958,851-byte Node22 blob. |
| 2026-08-29 | RRC8 | review | The Sol-only Deno branch review found four items. Three were accepted: stale task spawners could cross a warm lease, the public foreground-task drain did not require a live isolate scope, and fork CI did not run the changed Node module integration test. The proposed `deno_kv` test lane was refuted because the crate has zero Rust tests and the changed initializer is already covered by workspace check and carry-crate Clippy. No Opus 5 or Fable review ran. |
| 2026-08-29 | RRC8 | finding | The local Deno correction generation-fences task spawners, rotates both active handles during reset, binds foreground-task draining to a live mutable `PinScope`, and builds the fork CLI and test server before the Node module integration lane. Full `deno_core` passes 476 tests with 2 declared ignores. Four task-spawner tests, both warm-reset tests, the Node module integration lane, warning-denied core Clippy, and the Nimbus runtime compile pass against the exact local Deno and V8 graph. |
| 2026-08-29 | RRC8 | review | The required Sol xhigh follow-up reports no actionable P0 through P3 finding in the corrective diff. The Deno release lockfile is restored. U3 remains in progress because the reviewed correction is not yet an exact commit, and U2 publication remains deferred. |
| 2026-08-29 | RRC8 | finding | The broad Nimbus runtime replay found that the old Locker smoke test still treated raw `UnenteredIsolate` as `Send` and called its now-unsafe constructor without the required contract. The test now uses V8's explicit `SendableUnenteredIsolate` wrapper only for isolated cross-thread smoke coverage. Production Locker runtimes remain thread-affine. |
| 2026-08-29 | RRC8 | evidence | The corrected Locker suite passes 8 tests. The broad non-Node runtime replay passes 498 tests with 94 declared ignores, plus the generated anchor, Locker integration, and active doctest. The replay also exposed and closed a cross-graph `TempDir::into_path()` warning without raising Deno's dependency floor. Deno helper and binary checks and warning-denied Clippy pass. |
| 2026-08-29 | RRC8 | review | The Sol-only full Nimbus review reported two items. The P2 crossover finding is accepted because separate greps could mix fields from different JSONL records. The P3 cancellation-test deletion is refuted: the test remains registered and its body is byte-identical to `HEAD`. No Opus 5 or Fable review ran. |
| 2026-08-29 | RRC8 | finding | The crossover gate now parses JSONL and validates the schema, benchmark group, profile, workload, execution model, pool kind, benchmark ID, strategy label, and actual construction mode on each selected record. Three helper tests pass, including the mixed-row regression. The exact local Deno/V8 graph passes both live Node22 and WebStandard crossover rows. |
| 2026-08-30 | RRC8 | review | The Sol-only crossover follow-up found three accepted evidence defects: construction mode came from the requested profile, strategy was not a first-class trace field, and the success test did not assert its result. No Opus 5 or Fable review ran. |
| 2026-08-30 | RRC8 | finding | Successful V8 construction now increments one of two runtime-owned counters after bootstrap finalization. Benchmark traces derive actual mode from those counters, reject absent or mixed modes, and report explicit strategy plus both counts. Five helper tests, focused Node22 and Web construction tests, warning-denied all-target runtime Clippy, and the live local-graph crossover pass. |
| 2026-08-30 | RRC8 | review | The next Sol-only review found two accepted verifier defects: a reused trace directory could admit stale rows, and an unknown expected mode could pass with zero counters. Each run now gets a unique trace directory, and the validator accepts only the two supported modes. The follow-up found no remaining trace defect. Its only P1 is the known release-order gate: the normal Cargo graph cannot consume unpublished fork revisions. No Opus 5 or Fable review ran. |
| 2026-08-30 | RRC8 | evidence | The unpublished local graph passes macOS fresh-root product smoke, the 10-step embedded UI Chromium walk, all 9 applications and 37 anchors, and the clean Desktop worktree's 186 unit tests plus 5 packaged Electron tests. The isolated Debian 13.4 x86_64 build on `minicloud.local` passes native product smoke and all 9 applications. |
| 2026-08-30 | RRC8 | finding | The release replay closed four local defects: generated `.env.local` files no longer invalidate application source-integrity checks; harness source capture fails closed; cargo-deny recognizes the Deno 2.9.6 libuv and bindgen graph; and the lockfile replaces yanked `chacha20` 0.10.0 with 0.10.2. The test-only snapshot fallback diagnostic now states its real cause. |
| 2026-08-30 | RRC8 | evidence | Full local `make ci` on the unpublished Deno 2.9.6 and V8 150.4 graph exits zero. It passes formatting, warning-denied Clippy, dependency policy, snapshot provenance, 498 focused runtime tests, 7,722 workspace tests, required liveness and protocol campaigns, JavaScript build and typecheck lanes, 846 UI tests, proof helpers, and 60 installer checks. The declared inventory is 94 focused runtime ignores and 111 workspace skips. |
| 2026-08-30 | RRC8 | review | A Sol xhigh review repeated the known normal-graph P1 and accepted one verifier-coverage P1. U3 restored the old profile verifier name as a compact current-contract aggregator instead of restoring rejected realm checks. The repair also updates stale TFA paths and verifies the compute-owned scaling boundary. The focused gate passes 24 REC checks, 19 tenant-isolation checks, 32 TFA checks, 8 crossover-trace tests, and the rejected-symbol check. No Opus 5 or Fable review ran. |
| 2026-08-30 | RRC8 | review | The next Sol xhigh review accepted two more verifier defects. The completion gate validated only synthetic trace fixtures, and the parser could accept an appended prior measurement series. The gate now validates the saved real Node and Web traces and rejects duplicate or non-increasing measured-iteration rows for each pool kind. No Opus 5 or Fable review ran. |
| 2026-08-30 | RRC8 | evidence | A fresh 10-sample exact-graph run wrote 16-record Node and Web JSONL traces into the RRC8 artifact directory. Both traces pass exact selector, strategy, construction-mode, counter, and measurement-order validation. The compact completion gate and its 8 validator tests pass. |
| 2026-08-30 | RRC8 | review | A later Sol xhigh review repeated the two known normal-graph P1 reports and accepted one trace-identity P1. Increasing iteration counts could not prove that selected rows came from one benchmark generation. No Opus 5 or Fable review ran. |
| 2026-08-30 | RRC8 | finding | PIR0 crossover traces now use schema v3 and carry one generated run ID through both benchmark processes and strategies. Validation rejects an empty identity, mixed generations, and non-increasing samples. A fresh exact-graph replay, 8 regression tests, the aggregate runtime-strategy gate, and all proof helpers pass. |
| 2026-08-30 | RRC8 | review | The focused Sol follow-up accepted one cross-file identity defect. Separate validator processes could accept a consistent Node trace and a consistent Web trace from different generations. It repeated the known normal-graph P1. No Opus 5 or Fable review ran. |
| 2026-08-30 | RRC8 | finding | Live validation now supplies the generated run ID to both trace checks. Durable closeout validates the Node artifact, extracts its identity, and requires the Web artifact to match. The aggregate gate, 8 regression tests, and all proof helpers pass. |
| 2026-08-30 | RRC8 | review | The final Sol xhigh follow-up reports no remaining trace-integrity defect. Its two P1 findings are duplicate reports of the known unpublished normal-graph blocker. No Opus 5 or Fable review ran. |
| 2026-08-30 | RRC8 | blocked | The exact unpublished Linux release build completed all dependencies but made only 11 minutes 29 seconds of CPU progress during 9 hours 57 minutes of final full-LTO linking on the 8 GiB `minicloud.local` host. The linker retained approximately 6.8 GiB and waited on swapped pages. The attempt was stopped after a bounded health check. Temporary swap and kernel tuning were fully reverted, and the 4.6 GiB compilation cache remains. No release binary was produced. The exact release lane needs a higher-memory runner after the immutable graph exists. |
| 2026-08-30 | RRC8 | resumed | The owner authorized the exact fork commits, immutable fork publication, Nimbus repin, and a higher-memory hosted Linux build. The Opus 5 and Fable restriction remains. |
| 2026-08-30 | RRC8 | checkpoint | Deno correction commit `8d48dc4a68df8e083ed4b17855440b1df6405620` records the reviewed nine-file warm-reset repair. The rusty_v8 versioned branch `nimbus/v150.4.0` now points at reviewed commit `dbb70a973d28cfe8cd6a2ea66d4f3d14fee488f0`; branch workflows `33340050199` and `33340050226` are pending. |
| 2026-08-30 | RRC8 | finding | The first rusty_v8 branch run found that both hosted source-build workflows lacked the V8 Linux `glib-2.0` development prerequisite. Commit `0990fe0da72431f86bcebfd2dc9a5145dd7fcc00` installs `libglib2.0-dev` in both workflows. Actionlint, 15 release-tool tests, tag validation, and the exact-commit Sol xhigh review pass. |
| 2026-08-30 | RRC8 | cleanup | Removed 98.7 GiB of ignored, rebuildable Cargo artifacts from the inactive SA6 and IMV worktrees after confirming that no process used either target. Free macOS data-volume space increased from 11 GiB to 109 GiB. Source changes, proofs, and all active Nimbus, Deno, and rusty_v8 caches remain intact. |
| 2026-08-30 | RRC8 | finding | Hosted run `33340402232` reached the test suite and exposed three Rust 1.91 compile-fail snapshots generated with `rust-src`; Linux lacked that declared component and omitted only standard-library source excerpts. Commit `2eed57dce3eb88a2937318276481d92095057580` declares `rust-src`. All 20 compile-fail cases, formatting, release tools, tag validation, action lint, whitespace, and the Sol-only exact-commit review pass. Superseded runs were cancelled. Replacement runs are `33342584853` and `33342584858`. |
| 2026-08-30 | RRC8 | finding | Run `33342584858` passed 305 of 306 native Windows ARM64 tests. The checksum test then caught a missing-sidecar panic, and Windows ARM64 aborted during unwind. Commit `62a8eddbfc3fa1f4d6a8554c87eb58cc898cbfe5` exposes the same fail-closed checks as `Result` helpers and tests errors without unwind. The source suite passes 306 tests with 2 skips, and six build-script tests pass. Focused Clippy, formatting, 15 release-tool tests, action lint, whitespace, and Sol review pass. RRC8 cancelled the superseded runs and started replacements `33351121161` and `33351121128`. |
| 2026-08-31 | RRC8 | finding | Replacement run `33351121128` proved the corrected native Windows ARM64 lane. Two AArch64 GNU jobs then reached the inherited matrix's exact 180-minute limit during source build. They reported cancellation, not a test failure. Commit `597ebc820d8de0039ec10b84f9f7adc0645c6db9` preserves upstream's 180-minute limit and gives Nimbus public runners the existing 360-minute cold-build allowance. Action lint, formatting, 15 release-tool tests, whitespace, strict comment lint, and Sol review pass. RRC8 cancelled the superseded runs and started replacements `33361207769` and `33361207758`. |
| 2026-08-31 | RRC8 | finding | Run `33361207758` then hit a GitHub transport reset while it downloaded `sccache`. The one-attempt download failed before any build, and branch fail-fast cancelled 27 unrelated matrix jobs. Commit `961a76d0cee88efdecfa9224c519fd153c404b51` adds bounded retries for that immutable public download and disables fail-fast only on Nimbus versioned branches. Upstream branch behavior remains unchanged. Action lint, formatting, 15 release-tool tests, whitespace, strict comment lint, and Sol review pass. Exact-head replacements are `33361461904` and `33361461885`. |
| 2026-08-31 | RRC8 | evidence | Exact-head branch workflows passed at `961a76d0cee88efdecfa9224c519fd153c404b51`: inherited CI run `33361461904` passed 39 of 39 jobs, and asset run `33361461885` passed 9 of 9 jobs. Annotated tag object `39585d3144a61d16067bd13c2a59463b5831825f` peels to that commit. The tag-only push started release-asset run `33376771045` and inherited CI run `33376770979`. |
| 2026-08-31 | RRC8 | finding | Cold annotated-tag runs exposed runner-only failures without a source change. Asset run `33376771045` built AArch64 GNU, then QEMU raised `SIGILL` in `clear_kept_objects`; the exact branch asset job passed all four configurations. Inherited run `33376770979` reached its six-hour limit in two AArch64 variants; their paired variants and all exact branch variants passed. Each workflow has one macOS job still active. After they finish, rerun only the failed or timed-out jobs on the same tag and commit. |
| 2026-08-31 | RRC8 | evidence | Same-commit reruns closed the cold-runner failures. Inherited tag run `33376770979` passed 39 of 39 jobs. Asset and publication run `33376771045` passed all seven target jobs, matrix derivation, and publication. Public release `v150.4.0-nimbus.1` is non-draft and non-prerelease. Its annotated tag peels to `961a76d0cee88efdecfa9224c519fd153c404b51`. A fresh download passed the repository verifier with 44 nonempty payloads and 44 checksum sidecars. The fork default branch is `nimbus/v150.4.0`. U2 is complete, and U4 starts with the Deno tag repin. |
| 2026-08-31 | RRC8 | finding | U4 found that the first AES-GCM short-tag policy treated Node 20 and 22 like Node 24 and consumed the one-shot warning before input validation. Current Node source requires Node 20 and 22 to warn only with `--pending-deprecation`, Node 24 to warn by default, and Node 26 to deny. Deno commit `15b0156a3033bcb327b92a4200355aca82ac23be` exposes the three-state policy and validates before warning. Nimbus maps each target to that policy and has focused regressions for silent Node 20 operation and warning order. |
| 2026-08-31 | RRC8 | evidence | The immutable V8 repin is Deno commit `96832ab2dbbe711842d13d0d0aeaf88f8387a5b3`. Locked workspace check, formatting, warning-denied carry Clippy, 183 focused carry tests, all 1,517 tests in the Node AES-GCM file, and the focused Nimbus Node 20 and Node 24 regressions pass. The exact Sol xhigh follow-up reports no accepted or actionable product-code finding. No Opus 5 or Fable review ran. |
| 2026-08-31 | RRC8 | finding | Hosted Deno runs found three workflow-only gaps. Isolated `deno_node`, `deno_fetch`, `deno_websocket`, and `deno_runtime` tests did not select a V8 backend. The Node module test also requires the pinned `tests/util/std` submodule. The workflow now enables `deno_core/default`, initializes only that depth-one fixture, and runs both changed Node integration cases. All corrected commands and three focused Sol-only reviews pass. |
| 2026-08-31 | RRC8 | evidence | Exact Deno branch run `33442674740` passed every step at `6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3`. Annotated tag object `4d8b978255e8ca9a78d040531ee764d695fd3bcf` peels to that commit. Tag run `33444743536` is in progress. |
| 2026-08-31 | RRC8 | evidence | Deno tag run `33444743536` passed at `6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3`. Public release `v2.9.6-nimbus.1` is non-draft and non-prerelease, and the default branch is `nimbus/v2.9.6`. U5 is complete. |
| 2026-08-31 | RRC8 | evidence | U6 repinned the normal Nimbus graph to public Deno `v2.9.6-nimbus.1` and rusty_v8 `v150.4.0-nimbus.1`. Cargo resolves 41 Deno packages and V8 to the exact published commits. Fork provenance, policy, standardization, the all-target runtime check, 499 canonical runtime tests, the embedded anchor, eight Locker tests, and the active doctest pass. Full repository and artifact replay remain in progress. |
| 2026-08-31 | RRC8 | finding | The first public-tag CI replay found two release-graph defects. Cargo deny rejected Wasmtime 46.0.2 for RUSTSEC-2026-0268 and RUSTSEC-2026-0269; the lock now uses fixed Wasmtime 46.0.3 and Cranelift 0.133.3, and 17 focused Wasmtime tests pass. The workspace V8 asset override and digest manifest still named 149.4.0; both now bind `v150.4.0-nimbus.1` and its exact published Linux x86_64 asset digests. |
| 2026-08-31 | RRC8 | evidence | Full `make ci` passes without a local V8 override on the public-tag graph. It passes formatting, warning-denied Clippy, dependency policy, live Node22 anchor provenance, 499 canonical runtime tests with 94 declared ignores, 7,722 workspace tests with 111 declared skips, two active workspace doctests, required liveness and protocol campaigns, JavaScript builds and typechecks, 846 UI tests, proof helpers, and 60 installer checks. |
| 2026-08-31 | RRC8 | review | Sol xhigh reviewed committed Nimbus candidate `d6636b980deedfeee8a64afb06230fa8a19a10a9` in eight isolated passes. Eight findings were valid: a public retirement deadline aborted durable recovery, one Krun pre-activation state could not close its stop fence, the Cloudflare KV REST minimum list limit was wrong, two matrix-verifier inputs failed open or crashed, Linux direct uninstall could remove package-owned helpers, the rejected-symbol gate accepted `rg` errors, and staged package roots could omit a version. Each repair has a focused regression. Two reports were false: the removed LR12 lane covered a deleted executor, and the runtime test already waits on an exact host-start notification. No Opus 5 or Fable review ran. |
