# Nimbus Release Readiness 2026-08

Status: `active`. Owner: this plan. Created: 2026-08-27.
Baseline: `codex/storage-review-repairs` @ `1403bc780` (`origin/main` @ `b57a2d680`)
Proof root: `proof/release-readiness-2026-08/`
Next action: pin the reachable Deno revision, commit the Nimbus integration,
then run the exact candidate and release gates

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
| RRC3 | Run every application and protocol-adapter smoke lane, including browser-visible app flows. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc3-app-adapter-smoke.md` |
| RRC4 | Test storage providers, encryption, backup/restore, object storage, consistency, and restart recovery. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc4-storage-recovery.md` |
| RRC5 | Test services, sandboxes, network policy, Compose, macOS machines, and Linux execution on `nimbus@minicloud`. | `blocked(exact-candidate replay depends on RRC1; provisional pass)` | `proof/release-readiness-2026-08/rrc5-workload-hosts.md` |
| RRC6 | Test and repair the desktop app against the candidate server, including packaging and local Mac UI operation. | `blocked(exact-candidate replay and notarization remain; provisional pass)` | `proof/release-readiness-2026-08/rrc6-desktop.md` |
| RRC7 | Validate archives, install paths, packages, OCI artifacts, upgrades, and current-release drift without publication. | `blocked(exact-candidate replay and public apt/COPR proofs remain; provisional pass)` | `proof/release-readiness-2026-08/rrc7-distribution.md` |
| RRC8 | Run final repository gates, repeat critical smoke tests, run Sol and Opus reviews, and issue the GO or NO-GO report. | `in_progress` | `proof/release-readiness-2026-08/rrc8-release-verdict.md` |
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
