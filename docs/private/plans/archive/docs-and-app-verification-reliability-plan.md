# Documentation and Application Verification Reliability Plan

Status: `cleanup candidate` | Owner: this plan | Created: 2026-08-17.
Completion: pending the cleanup pull request.
Baseline: main @ 82bdcf2db5f7e021bdf701cab13f60e6e138c2cf.
Proof root: `../proof/docs-and-app-verification-reliability/`.
Next action: require green hosted checks on cleanup PR #278, merge it under
owner authority, audit the merge, and remove the clean worktree.

## Outcome

> Nimbus documents the network control plane from public product concepts to
> private ownership records. Application verification starts from a fresh
> checkout, leaves source unchanged, owns every temporary resource, produces
> structured evidence, and completes within a measured parallel-run budget.

## Architecture

| Component | Before | After |
|---|---|---|
| Documentation contract | Missing public owner and stale private status. | Public source map and private ownership truth. |
| Source workspace | In-place codegen and tracked-file mutation. | Read-only input with byte-digest proof. |
| Prerequisite contract | Hidden behind CI artifacts. | Fresh-checkout build and Node preflight. |
| Operator state | Shared discovery, authentication, and audit paths. | Case-local paths with one run-global network root. |
| Network port authority | Shell scan-close plus product leases. | Product provider-assigned leases and socket adoption. |
| Case executor | Nine serial applications. | Bounded parallel applications with serial diagnostics. |
| Evidence contract | Console text. | Validated JSON, JUnit, timing, and cleanup state. |

## Scope

- Owns: network-documentation truth, completed network-plan archival, and related developer documentation.
- Owns: application-verification isolation, prerequisites, resource lifetime, evidence, and measured performance.
- Owns: a bounded `nimbus dev` Compose opt-out and the current bare-local `nimbus run` decision because the live application lane needs those seams.
- Does not own: delivered network semantics, sandbox provider behavior, or adapter feature expansion.
- Does not own: cluster transport or a new application surface.
- Does not own: broad architecture-review repairs. The active architecture review remains the owner outside the paths and findings listed here.
- Non-goals: compatibility shims, a second port authority, a new test framework, public links into `docs/private/`, or performance gains that reduce coverage.

## Promotion gate

Promote this plan to `active` only when every gate holds:

| Gate | Required state |
|---|---|
| Authority | The owner approved the named worktree and branch. |
| Plan checks | Plan, index, proof contract, and routing changes pass without product changes. |
| Durability | Commit the checkpoint before fetch. Prove it with `git cat-file -e HEAD:docs/private/plans/archive/docs-and-app-verification-reliability-plan.md`. |
| Reconciliation | The clean branch retains its baseline and proof root after current-main reconciliation. |
| Coverage | Every finding has one task. Every task has measurable acceptance. AVR0 can author the red verifier. |
| Execution | The goal retains repository review and pull-request rules. |

## Coordination

- The completed network plan supplies ownership facts. This plan does not reopen them.
- The archived examples plan supplies history. Current source and live evidence win.
- The architecture review owns unrelated repairs. This plan owns listed paths.
- The docs skill governs public structure and the private fence.
- The verification runbook governs long and hosted checks after AVR0 creates it.
- Three implementation PRs run in order. Each merge precedes current-main reconciliation.

## Invariants

1. A verification run never changes tracked source bytes.
2. Cleanup never restores source with `git checkout`, reset, or clean.
3. Each temporary resource has one lifetime owner. Cleanup failure makes the
   run fail.
4. `nimbus-network -> nimbus-core` remains its only workspace edge.
5. Concrete sockets and provider effects stay outside `nimbus-network`.
6. Public pages cite source and never link to `docs/private/`.
7. Default `nimbus dev` Compose discovery remains unchanged. Tests use an
   explicit opt-out.
8. A target-resolution fix cannot weaken PR #238 or PR #239 tenant boundaries.
9. Reports separate desired test intent, observed results, and cleanup status.
10. A verifier reads source or stable contract fixtures, never active plan prose.
11. One task stays `in_progress`. Each terminal task records exact evidence.
12. Review runs only on a candidate-frozen phase pull request after its gates.
13. One run-global network-state root preserves host-global lease authority.
    Authentication, discovery, audit, app, data, control, and log roots are
    case-local.
14. The runner consumes provider-assigned product leases and retained listener
    adoption. It never creates a shell-side port allocator.

## Findings ledger

| ID | Classification | Evidence | Owning task |
|---|---|---|---|
| AVRF01 | HIGH, confirmed | A green live run rewrote five example manifests and `package-lock.json`. | AVR4 |
| AVRF02 | HIGH, confirmed | The raw Cargo fallback fails without UI and embedded-package artifacts; direct script invocation is not self-contained. | AVR3 |
| AVRF03 | HIGH, confirmed | The runner renames tracked `compose.yaml` around two dev-mode apps. | AVR5 |
| AVRF04 | HIGH, confirmed | The public architecture set has no `nimbus-network` page or source-map rows. | AVR2 |
| AVRF05 | MEDIUM, confirmed | Private docs and the active architecture review still call NNC9 and its completed plan active; the archived examples plan also has a stale active row. | AVR1 |
| AVRF06 | MEDIUM, confirmed | Permanent verifiers parse the completed plan as contract data. | AVR1 |
| AVRF07 | MEDIUM, confirmed | The runner creates a temp root but never removes it. | AVR7 |
| AVRF08 | MEDIUM, confirmed | Main-port scan-close and fixed 27017/8000/9000 listeners permit collisions. | AVR7 |
| AVRF09 | MEDIUM, confirmed | The runner lacks an early Node `>=22 <25` preflight. | AVR3 |
| AVRF10 | MEDIUM, needs current repro | Bare-local `nimbus run` uses an explicit-target workaround for an old auth defect. | AVR6 |
| AVRF11 | MEDIUM, confirmed | Results lack canonical JSON, JUnit, hashes, timings, and cleanup evidence. | AVR8 |
| AVRF12 | MEDIUM, confirmed | Nine app cases run serially even after their resources become independent. | AVR9 |
| AVRF13 | MEDIUM, confirmed | The runner has no fail-closed source-byte postcondition; status-only comparison can miss changes in an already-dirty file. | AVR4 |
| AVRF14 | LOW, confirmed | README and comments retain old app counts and old Convex status. | AVR10 |
| AVRF15 | LOW, confirmed | The shared live-update name does not distinguish push from polling. | AVR10 |
| AVRF16 | LOW, confirmed | Public overview and sandbox/server pages omit lifecycle-plane cross-links. | AVR2 |
| AVRF17 | HIGH, confirmed | Concurrent cases share platform authentication, discovery, and audit paths; a later server can replace the discovery record used by bare-local commands. | AVR7 |
| AVRF18 | HIGH, confirmed | Repository bootstrap routing names six missing private routing, operating, local-development, adapter, and Convex-guidance documents. | AVR0 |
| AVRF19 | LOW, confirmed | The docs build emits Astro's `markdown.gfm` and `markdown.smartypants` deprecation warning. | AVR10 |
| AVRF20 | MEDIUM, confirmed | Docs runs `32050695317`, `32050937873`, and `32054899165` deployed their previews, then failed because the zero-retry GitHub comment step received HTTP 503. | AVR2 |
| AVRF21 | MEDIUM, confirmed | CI run `32050937772` reproduced a listener-projection test whose process-wide one-shot storage fault could be consumed by concurrent router startup instead of its named projection transaction. | AVR2 |
| AVRF22 | HIGH, confirmed | The required local `cargo test` path runs managed `nimbus-server` tests concurrently even though they share one process-global network authority; the full lib run failed 62 tests with `DuplicateProcessComposition`, while its serialized run passed. | AVR11 |
| AVRF23 | LOW, confirmed | The fresh UI prerequisite emits 18 route-file warnings for support modules under `packages/nimbus-ui/src/routes`, which hides useful verification output. | AVR10 |
| AVRF24 | MEDIUM, resolved | A whole-directory `node_modules` link let case provisioning remove owner-worktree package links. The AVR4 adapter now owns real case-local scope and `.bin` directories and links only their entries. | AVR4 |
| AVRF25 | LOW, confirmed | `docs/reference/cli.md` and `docs/source-map.md` have an 85-diagnostic pre-existing technical-writing baseline. AVR5 additions are delta-clean. | AVR10 |
| AVRF26 | HIGH, resolved | Hosted runs reproduced Azure regional Ubuntu mirror stalls before project code ran. PR #280 removed that mirror from the official priority list, bounded apt network waits, passed 50 checks with three expected skips, and merged as `437c2a185`. | AVR11, AVR12 |

## Decisions

Binding records AVRD1-AVRD8, including dates, evidence, consequences, and
re-open conditions, live in
[`acceptance-contract.md`](../proof/docs-and-app-verification-reliability/acceptance-contract.md).

## Verifier contract

`scripts/verify-docs-app-verification.sh` owns 24 source-derived conditions.
AVR0 authors it red. Terminal output is `Summary: 24 passed, 0 failed`, and
one meaningful mutation per condition reports `24/24`. The binding acceptance
contract records condition ownership, phase counts, and exact commands:
[`acceptance-contract.md`](../proof/docs-and-app-verification-reliability/acceptance-contract.md).

## Status ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| AVR0 | Verify the baseline, author the 24-condition verifier red, repair private verification routing, and capture all fail-before evidence without product changes. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr0.md`. Work commit `e7ea6d220`; baseline 0/24; mutation self-test 24/24; docs 108; site 17/17; build 109 pages; six Markdown files lint-clean. |
| AVR1 | Extract stable network-verifier contracts, archive the completed network plan, and correct private routing and status. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr1.md`. Work commits `3300c6b6f` and `b24959165`; AVRC01-AVRC04 4/4; network verifier 39/39; mutation suite 610/610; docs 108; site 17/17; build 109 pages. |
| AVR2 | Publish and cross-link the source-verified network architecture and align public lifecycle-plane messaging. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr2.md`. Work commits `4ad5a2c1b`, `389734582`, `d74bce443`, `b98ee3242`, and `a786468eb`; AVRC05-AVRC10 6/6; phase one 10/10; mutations 24/24; network verifier 39/39; docs 109; site 17/17; build 110 HTML. Final head `c3e10f6ac` passed CI, Docs, CodeQL, Desktop UI, and Windows. PR #275 merged as `520dba9fb`; the owner branch fast-forwarded to the same current-main commit with zero divergence. |
| AVR3 | Make the application lane self-building from a fresh checkout and add a fail-fast Node version contract. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr3.md`. Work commit `8042e32e4`; AVRC11-AVRC12 2/2; 13 behavior cases; mutations 24/24; Node.js 22 Make and Node.js 24 direct live runs passed 5/5 app assertions. |
| AVR4 | Replace in-place app preparation with a validated case manifest and disposable workspaces. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr4.md`. Work commit `827877d06`; AVRC13-AVRC15 3/3; 6/6 behavior tests; nine preparation fixtures; mutations 24/24; 14/14 live assertions across three execution shapes. |
| AVR5 | Add an explicit Compose-discovery opt-out and delete the tracked-file sideline. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr5.md`. Work commit `02788d24b`; AVRC16-AVRC17 2/2; CLI 1,015 passed and 4 ignored; Clippy; live dev cases 7/7; docs 109 and 17/17. |
| AVR6 | Reproduce and close, or disprove and remove, the bare-local target workaround. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr6.md`. Work commit `390bcaf27`; AVRC18 1/1; CLI 1,019 passed and 4 ignored; explicit and bare-local live results matched; wrong-silo and invalid-bearer checks failed closed. |
| AVR7 | Give ports, child processes, temporary roots, logs, and cancellation one fail-closed lifetime owner. | `done` | Work `bd2a8a364`; review corrections `2215a5772`, `27331b144`; hosted correction `9e9482ce8`; proof: `../proof/docs-and-app-verification-reliability/avr7.md`. PR [#276](https://github.com/nimbus/nimbus/pull/276) passed 54 hosted checks with 3 expected skips and zero open CodeQL alerts, then merged as `b58ef8c35`. Reconciliation commit `ec6d2414c` preserves the local recovery checkpoint on current `main`. |
| AVR8 | Emit canonical JSON and JUnit evidence with hashes, timings, assertions, and cleanup state. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr8.md`. Report 8/8; supervisor 2/2; fault and retry 7/7; AVRC21-AVRC22 2/2; mutations 24/24. The final live run passed 9 applications, 37 anchors, 12 JUnit tests, matching source digests, and clean removal in 83,725 ms. |
| AVR9 | Add bounded parallel execution and meet the measured wall-clock target without coverage loss. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr9.md`. AVRC23 1/1; evaluator 5/5; scheduler 2/2; fault and retry 7/7; mutations 24/24. Three serial and five parallel minicloud samples passed 9 applications and 37 anchors. Medians were 112,403 and 67,066 ms; ratio 0.5967. |
| AVR10 | Correct all example documentation, comments, counts, update semantics, and operator instructions. | `done` | Proof: `../proof/docs-and-app-verification-reliability/avr10.md`. Work commit `04a675b29`; AVRC24 1/1; manifest docs 6/6; UI 336/336; full verifier and mutations 24/24; docs 109 and 17/17; site build 110 pages; writing diagnostics changed from 18 to 10. |
| AVR11 | Run local, minicloud, repository, docs, review, and hosted-CI acceptance, then close the third implementation pull request. | `done` | PR [#277](https://github.com/nimbus/nimbus/pull/277) passed 54 hosted checks with 3 expected skips and merged as `c9b551a30`. Reconciliation commit `d7c178523` preserves checkpoint `a2f49170d` and current-main storage-plan work. |
| AVR12 | After the third implementation pull request merges, archive this plan through a cleanup pull request and remove its active routing. | `in_progress` | Cleanup PR [#278](https://github.com/nimbus/nimbus/pull/278) is open. Its first hosted run exposed the resolved Azure mirror defect. Remediation PR #280 passed 50 checks with three expected skips and merged as `437c2a185`; the cleanup branch reconciled it. Archive, routing, AVR 24/24, mutation 24/24, docs 109, site 17/17, diff, and archive searches pass. Replacement hosted checks, merge, and clean worktree removal remain. |

## Tasks

Use this phase order:

| Phase | Tasks | Completion gate |
|---|---|---|
| 0. Baseline | AVR0 | Red verifier, complete proof inventory, no product change. |
| 1. Documentation truth | AVR1-AVR2 | AVRC01-AVRC10 green; first implementation PR reviewed, hosted-green, merged, and reconciled. |
| 2. Hermetic serial lane | AVR3-AVR7 | AVRC01-AVRC20 green; second implementation PR reviewed, hosted-green, merged, and reconciled. |
| 3. Evidence and speed | AVR8-AVR10 | AVRC01-AVRC24 green and both time budgets hold. |
| 4. Integrated acceptance | AVR11 | Third implementation PR is candidate-frozen, reviewed once, hosted-green, merged, and reconciled. |
| 5. Cleanup | AVR12 | Cleanup PR merges and the plan moves to the archive. |

### Campaign checkpoints

| Pull request | Included tasks | Required checkpoint |
|---|---|---|
| Implementation PR 1 | AVR0-AVR2 | One Sol/xhigh/fast phase review after all phase gates; merge confirmation; clean current-main reconciliation before AVR3. |
| Implementation PR 2 | AVR3-AVR7 | One Sol/xhigh/fast phase review after all phase gates; merge confirmation; clean current-main reconciliation before AVR8. |
| Implementation PR 3 | AVR8-AVR11 | One Sol/xhigh/fast pre-PR review after the complete candidate is committed; hosted acceptance and merge confirmation. |
| Cleanup PR | AVR12 | Documentation-only checks; no autoreview unless executable code changes. |

The status line and execution log record the active PR number, hosted run, merge
commit, reconciliation commit, and next task. A later phase cannot start while
its predecessor PR is open or unmerged.

### AVR0 Baseline and red verifier

- Problem: the plan needs durable recovery state and a source-derived baseline.
- Owning seam and paths: this plan, its proof root,
  `docs/private/{README.md,operating/,adapters/}`, `AGENTS.md`, and
  `scripts/verify-docs-app-verification.sh`.
- Steps: execute AVR0.1-AVR0.6 from the acceptance contract in order.
- Acceptance: `HEAD` contains the plan and verifier. The worktree is clean.
  The verifier reports exact baseline counts. No product behavior changes.
  Each finding has one owner.
- Fail-before: the current source fails AVRF01 through AVRF18 where applicable.
- Verification: run the AVR0 command-contract row. Run `git diff --check`, both
  docs gates, the docs site build, and the technical-writing linter on all AVR0
  Markdown changes.

### AVR1 Stable network contracts and private closeout

- Problem: stale private status and plan-parsing scripts prevent safe archival.
- Owning seam and paths: network verifier scripts,
  `docs/private/architecture/network/`, `docs/private/architecture/README.md`,
  `docs/private/plans/architecture-review-2026-07-plan.md`, the plans index, the
  archived examples plan, and the completed network plan.
- Steps: execute AVR1.1-AVR1.6 from the acceptance contract in order.
- Acceptance: no executable reads the active-plan path. Network checks match
  AVR0 counts. Private docs state the merged status. Archive links resolve.
- Fail-before: current scripts name the active plan and private docs name NNC9
  as active.
- Verification: run the AVR1 command-contract row. Require
  `Summary: 4 passed, 0 failed` for AVRC01-AVRC04 and zero executable references
  to the old active-plan path.

### AVR2 Public network architecture

- Problem: public users cannot find the connectivity-resource owner or its
  relationship to transport, compute, storage, sandboxes, and services.
- Owning seam and paths: `ARCHITECTURE.md`, `README.md`,
  `docs/concepts/index.md`, `docs/concepts/architecture/`,
  `docs/concepts/how-nimbus-works.md`,
  `docs/reference/current-capabilities.md`, `docs/source-map.md`,
  `.github/workflows/docs.yml`, and `scripts/verify-nimbus-docs-site.sh`.
- Steps: execute AVR2.1-AVR2.6 from the acceptance contract in order.
- Acceptance: every claim maps to source. Transport and resource lifecycle stay
  distinct. Desired, durable, and observed state stay distinct. No public page
  links to private docs.
- Acceptance: a successful preview upload writes its URL to the job summary.
  PR comment delivery retries three times. If GitHub remains unavailable, the
  workflow reports a warning but retains the successful preview result.
- Fail-before: the architecture index and source map contain no network page.
- Fail-before: three Docs runs uploaded a preview and then failed only when the
  zero-retry PR comment received HTTP 503.
- Verification: run the AVR2 command-contract row. Require
  `--through-phase 1` to report `10 passed, 0 failed`. Inspect `llms.txt`,
  `llms-full.txt`, and `llms-small.txt` for the network page and private-fence
  violations.

### AVR3 Fresh-checkout prerequisites and Node contract

- Problem: local fallback builds fail late and supported Node versions are not
  checked before work starts.
- Owning seam and paths: `Makefile`, `scripts/examples-verify.sh`, and focused
  runner contract tests.
- Steps: execute AVR3.1-AVR3.5 from the acceptance contract in order.
- Acceptance: a fresh exported checkout builds and runs one selected app.
  Node 22 and 24 pass. Unsupported versions fail before work starts. A supplied
  binary skips the Rust build. Direct script invocation is either self-contained
  or fails before work with the exact supported Make command.
- Fail-before: raw Cargo reports the missing UI and package manifest, and Node
  20 reaches later work before it fails.
- Verification: run the AVR3 command-contract row, then
  `NIMBUS_EXAMPLES_VERIFY_ONLY=nimbus/tasks make examples-verify` and the same
  selection through direct script invocation. Require AVRC11-AVRC12 `2/2`.

### AVR4 Validated manifest and disposable workspaces

- Problem: preparation rewrites tracked manifests and the root lockfile.
- Owning seam and paths: the examples case manifest, the runner workspace
  adapter, and its fixture tests.
- Steps: execute AVR4.1-AVR4.5 from the acceptance contract in order.
- Acceptance: the manifest names nine unique apps and their required surfaces.
  Success and every failure leave source unchanged. No recovery command restores
  tracked files.
- Fail-before: current live codegen changes the five manifests and lockfile.
- Verification: run the AVR4 command-contract row. Its named cases are
  `manifest_rejects_duplicate_or_incomplete_case`,
  `dirty_source_bytes_survive_success`, `dirty_source_bytes_survive_failure`,
  `staged_source_bytes_survive_failure`, and all nine preparation fixtures.
  Require AVRC13-AVRC15 `3/3`.

### AVR5 Explicit Compose mode

- Problem: two app cases hide Compose files by renaming tracked source.
- Owning seam and paths: `nimbus dev` argument and planning code, CLI tests,
  runner boot options, CLI reference, and source map.
- Steps: execute AVR5.1-AVR5.5 from the acceptance contract in order.
- Acceptance: default discovery tests stay green. The opt-out does not read the
  root Compose project. Signals cannot strand a backup. The runner never moves
  `compose.yaml`.
- Fail-before: current dev cases call `sideline_compose`.
- Verification: run the AVR5 command-contract row,
  `cargo test -p nimbus-cli compose_discovery_opt_out`, and both dev app cases.
  Run both docs gates. Require AVRC16-AVRC17 `2/2`.

### AVR6 Bare-local target resolution

- Problem: the runner carries a possibly stale explicit-target auth workaround.
- Owning seam and paths: CLI local target resolution, Convex invocation auth,
  stdio contract tests, and the runner comment and invocation.
- Steps: execute AVR6.1-AVR6.4 from the acceptance contract in order.
- Acceptance: both local target forms return the same result. Stdout contains
  result JSON only. Banners stay on stderr. Invalid credentials fail closed.
- Fail-before: record either the current 401 or proof that the old report no
  longer reproduces. Non-reproduction can make the product-code subdecision
  no-action, but removal, tests, and proof still make AVR6 `done`.
- Verification: run the AVR6 command-contract row and
  `cargo test -p nimbus-cli run::tests`. Run both live target forms, the
  cross-case refusal, and named PR #238/#239 trust regressions. Require AVRC18
  `1/1`.

### AVR7 Per-case resource lifetime

- Problem: port selection races, fixed wire ports collide, and temp roots leak.
- Owning seam and paths: runner resource lifetime, listener configuration, and
  process-level fault fixtures.
- Steps: execute AVR7.1-AVR7.6 from the acceptance contract in order.
- Acceptance: no scan-close or runner-side port allocation remains. Two cases
  bind concurrently through one network authority. A case cannot read another
  case's discovery, token, audit, data, or result state. An external-binder race
  cannot report green. Each fault cut leaves no owned resource. Cleanup failure
  makes the run red.
- Fail-before: current runner closes its probe socket, assumes 27017/8000, and
  leaves `DATA_ROOT` after success.
- Verification: run the AVR7 command-contract row,
  `cargo test -p nimbus-network port_lease`,
  `cargo test -p nimbus-server listener_lease`, the
  cross-case discovery/auth sentinel, bind-race, concurrent-case, process-tree,
  six fault cuts, cleanup-retry, and retained-artifact tests. Require
  AVRC19-AVRC20 `2/2`.

### AVR8 Structured evidence

- Problem: text output cannot prove the binary, source, duration, or cleanup.
- Owning seam and paths: application-verification report schema, writer,
  validator, JUnit projection, and CI artifact upload.
- Steps: execute AVR8.1-AVR8.5 from the acceptance contract in order.
- Acceptance: reports include hashes, versions, manifest digest, ports, anchors,
  semantics, durations, exits, cleanup, and source status. Invalid reports fail.
- Fail-before: the current lane emits only console text.
- Verification: run the AVR8 command-contract row. Its named cases are schema
  success and rejection goldens, credential redaction, interrupted-write
  recovery, stable report ordering, and success/failure JUnit projections.
  Require AVRC21-AVRC22 `2/2`.

### AVR9 Bounded parallel execution

- Problem: isolated cases still pay nine server boot cycles serially.
- Owning seam and paths: runner scheduler, case logs, report ordering, and
  CI job configuration.
- Steps: execute AVR9.1-AVR9.6 from the acceptance contract in order.
- Acceptance: parallel coverage matches serial coverage. Report order stays
  stable. No collision occurs. Parallel median time is at most 60 percent of
  serial median time and at most 1,200 seconds. Five runs pass. A busy or
  differently configured host invalidates, rather than fails, the sample.
- Fail-before: the current runner has one active case and nine sequential boots.
- Verification: run the AVR9 command-contract row, scheduler and failure-drain
  tests, jobs=1 parity, and two concurrent fault runs. Require AVRC23 `1/1` and
  preserve the eight raw timing reports plus one verdict.

### AVR10 Example documentation and comments

- Problem: user and maintainer text reports old counts, failures, and semantics.
- Owning seam: example READMEs, runner and Make comments, and affected CLI help.
  It also owns `docs/source-map.md` and the docs-site Markdown processor.
- Steps: execute AVR10.1-AVR10.6 from the acceptance contract in order.
- Acceptance: no text states an old count or partial Convex status. Update text
  distinguishes push from polling. Commands match tested paths and Node range.
- Acceptance: the docs build emits no deprecated Markdown-processor warning.
- Fail-before: static searches find every stale claim from AVRF14 and AVRF15.
- Fail-before: the docs build emits the warning recorded by AVRF19.
- Verification: run the AVR10 command-contract row and require AVRC24 `1/1`.
  Record zero new technical-writing diagnostics. Do not expand this task to fix
  unrelated diagnostics in untouched plans-index lines.

### AVR11 Full acceptance and third implementation pull request

- Problem: task proofs need one integrated candidate and hosted confirmation.
- Owning seam and paths: the complete phase diffs, proof root, CI workflow, and
  pull-request evidence.
- Steps: execute AVR11.1-AVR11.10 from the acceptance contract in order.
- Acceptance: AVR reports 24/24 and self-test 24/24. All apps and anchors pass
  in serial and parallel modes. Cleanup passes. Local, minicloud, and hosted
  gates pass. GPT-5.6 Sol reviews the complete candidate in xhigh fast mode.
- Acceptance: the canonical local Rust test entry point isolates tests that
  require the process-global network authority.
- Acceptance: `make test` and `make ci` pass without a hidden manual
  `--test-threads=1` workaround. The product continues to reject a second
  in-process authority.
- Fail-before: the AVR0 baseline remains in the proof root.
- Fail-before: unrestricted `cargo test -p nimbus-server --lib` reported 601
  passed, 62 `DuplicateProcessComposition` failures, and 35 ignored. The same
  binary reported 663 passed, 0 failed, and 35 ignored with
  `--test-threads=1`.
- Verification: run the AVR11 command-contract row. Use
  `nimbus-autoreview --gate pre-pr --mode auto` only after the owner commits and
  freezes the candidate. Rerun one narrow correction review only when an accepted
  finding changes executable code. Record exact counts, hashes, durations,
  reviewer identity, dispositions, PR URL, hosted run URLs, and merge commit.

### AVR12 Post-merge cleanup

- Problem: a merged plan must not remain an active control plane.
- Owning seam and paths: this plan, its proof root, plans index, branch, and
  dedicated worktree.
- Steps: execute AVR12.1-AVR12.6 from the acceptance contract in order.
- Acceptance: the archive records all three implementation PRs, the cleanup PR,
  and their merges. No active
  route names this plan. No executable consumes it as data. No unmerged change
  remains.
- Fail-before: not applicable because the implementation merge is the trigger.
- Verification: run the AVR12 command-contract row. Confirm all four merges.
  Inspect final worktree and branch state. Record the recovery status for local
  branch and worktree removal.

## Goal

```text
Execute docs/private/plans/archive/docs-and-app-verification-reliability-plan.md
to completion. This is a whole-plan goal, not a single-task goal. Read the
plan fully, then read AGENTS.md, README.md, ARCHITECTURE.md, docs/README.md,
docs/private/plans/README.md, docs/private/architecture/README.md,
docs/private/testing/TEST_CONVENTIONS.md, the docs skill, the plans skill,
the technical-writing skill, the nimbus-autoreview skill, the gh skill, and
../proof/docs-and-app-verification-reliability/acceptance-contract.md relative to
the plan directory, plus the active task's source and tests. AVR0 must create
and source-verify the six
missing private routing and operating documents named by AVRF18. Read those
documents after AVR0 and before any later task. Work in
/Users/jack/src/github.com/nimbus/nimbus-docs-and-app-verification-reliability
on branch codex/docs-and-app-verification-reliability. Chat history is not
progress state. Resume from the status ledger, the execution log, and git
state. Before any fetch or reconciliation, prove the durable checkpoint with
`git cat-file -e HEAD:docs/private/plans/archive/docs-and-app-verification-reliability-plan.md`.
Never recover with git clean, reset, or checkout. If compaction happens,
continue from the plan and git state rather than restarting. Loop: attribute
every dirty path, keep one task in_progress, implement at the owning seam,
capture fail-before evidence, run the exact task commands, commit the work,
write the proof file, append the execution log with the work commit, mark the
task terminal with exact evidence, commit the plan update, then advance.
Decide rather than ask. Mark a wrong or already-satisfied whole task no-action
with a one-line reason. Record each discovery in the findings ledger. Route an
in-scope discovery to its owning task and an out-of-scope discovery to a named
owner. At about twice the planned scope, stop and re-scope before more edits.
Record a real blocker and continue with the next dependency-ready task.
Binding constraints: preserve all fourteen invariants, every non-goal, the
private docs fence, trusted tenant binding, source-byte immutability, one
run-global network authority, case-local operator state, exact cleanup, and
full app coverage. Do not weaken a test to make a gate pass. Commit policy:
use one work commit and one plan-checkpoint commit per task. Use exactly the
four campaign checkpoints in the plan. Do not start a later implementation
phase before the prior PR merges and current main is reconciled into a clean
owner branch. Use one Nimbus autoreview only after each complete phase PR is
committed, acceptance-green, and candidate-frozen. Use GPT-5.6 Sol, xhigh
reasoning, and fast mode; reject any result from another reviewer. Run one
narrow correction review only after an accepted finding changes executable
code. Do not rerun for docs, proof, formatting, ledger, or non-material edits.
Push and open each implementation PR when its gate passes. Do not merge without
owner authorization. Stop only at a valid stop state from the plans skill.
Before stopping, update the ledger, log, status-line next action, active PR,
and reconciliation state. The goal is met when AVR0-AVR12 are terminal, all
three implementation PRs and the cleanup PR are merged, the AVR verifier
reports 24/24 with self-test 24/24, hosted checks pass, and the completed plan
is archived.
```

## Execution log

Append rows at the end. This section stays last.

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-17 | meta | authored | Plan authored from the post-merge documentation and live-application audit. No implementation started. |
| 2026-08-17 | meta | promoted | Full plan review resolved all 11 findings. Structural audit: 13/13 task contracts, 18/18 findings routed, AVRC01-AVRC24 mapped, 425 plan lines, ledger at line 119. Technical-writing lint: 4 files, 0 diagnostics. Docs: 108 pages and 17/17 site conditions. Site build: 109 HTML pages. No product implementation started. |
| 2026-08-17 | AVR0 | started | Goal activated. Durable plan checkpoint `7abfe4409` exists in `HEAD`; fetched `origin/main` remains `82bdcf2db`; clean branch was one commit ahead before this transition. |
| 2026-08-17 | AVR0 | completed | Work commit `e7ea6d220` added six missing private routing/runbook documents and the two verifier entry points without product behavior changes. Proof: `../proof/docs-and-app-verification-reliability/avr0.md`. Baseline 0/24; mutation self-test 24/24; all syntax, ShellCheck, writing, docs, site, link, and diff gates pass. |
| 2026-08-17 | AVR1 | started | AVR0 is durable. Classify the 13 executable readers, extract stable contract inputs, preserve verifier behavior, correct stale private status, then archive and reroute the completed network plan. |
| 2026-08-17 | AVR1 | completed | Work commits `3300c6b6f` and `b24959165` extracted the stable JSON contract, archived the completed network plan, refreshed live-source census inputs, and corrected three affected mutation fixtures. Proof: `../proof/docs-and-app-verification-reliability/avr1.md`. AVRC01-AVRC04 4/4; network verifier 39/39; mutations 610/610; docs 108; site 17/17; build 109 pages. |
| 2026-08-17 | AVR2 | started | AVR1 is durable and all archival behavior is green. Map public network ownership and lifecycle claims to source, add the public page and source-map routes, then inspect every generated `llms` output. |
| 2026-08-17 | AVR2 | candidate complete | Work commits `4ad5a2c1b`, `389734582`, and `d74bce443` publish the source-verified page and close all review findings. Proof: `../proof/docs-and-app-verification-reliability/avr2.md`. AVR2 6/6; phase one 10/10; mutations 24/24; network verifier 39/39; docs 109; site 17/17; build 110 HTML; lint delta zero. Implementation PR 1 is next. |
| 2026-08-17 | AVR2 | PR opened | Implementation PR 1 is [#275](https://github.com/nimbus/nimbus/pull/275). Head `ad7923682` started CI run `32050695309`, docs run `32050695317`, and CodeQL run `32050695320`. Hosted checks are in progress. |
| 2026-08-17 | AVR2 | hosted finding | Docs run `32050695317` failed after preview upload because GitHub's incident returned HTTP 503 to the zero-retry comment step. Replacement run `32050785842` passed. AVRF20 routes bounded notification recovery to AVR11; AVR2 product scope stays frozen. |
| 2026-08-17 | AVR2 | hosted correction | Commit `b98ee3242` resolves CI run `32050937772` by targeting the listener projection fault to the system tenant's durable listener-and-port write record. Focused 1/1, fresh-process stress 50/50, listener group 10/10, serialized server 663 passed with 35 ignored, and package Clippy are green. The unrestricted full-server run separately exposed AVRF22 for AVR11. |
| 2026-08-17 | AVR2 | Docs hardening | Docs run `32054899165` became the third run to upload a valid preview and then fail on a zero-retry comment PATCH during GitHub's incident. AVRF20 moved from AVR11 to AVR2 because it repeatedly blocks PR 1. The candidate writes the URL to the job summary, retries comments three times, and converts exhausted comment delivery to a visible warning. |
| 2026-08-17 | AVR2 | Docs hosted proof | Docs run `32055431404` uploaded a preview, exhausted all three comment retries on HTTP 503, emitted the explicit warning, and completed successfully. CodeQL run `32055431439` failed only when GitHub returned HTTP 503 during Rust initialization; JavaScript analysis passed. Other hosted runs remain active. |
| 2026-08-17 | AVR2 | hosted CI green | CI run `32055431425` completed with 47 successful jobs, three expected skips, and zero failures. Workspace shard 2 passed the AVRF21 regression. Desktop UI run `32055431555` also passed. Windows remains active. |
| 2026-08-17 | AVR2 | hosted candidate green | Windows run `32055431570` passed. Failed-only CodeQL attempt 2 passed Rust analysis after GitHub reported API recovery. PR #275 reports a clean merge state and every current-head check is green. |
| 2026-08-17 | AVR2 | completed | Final head `c3e10f6ac` passed CI run `32061251288`, Docs `32061251374`, CodeQL `32061251375`, Desktop UI `32061251273`, and Windows `32061251260`. PR #275 merged as `520dba9fb`. The owner branch fast-forwarded to that current-main commit with zero divergence. |
| 2026-08-17 | AVR3 | started | Implementation PR 1 is merged and reconciled. Reproduce the fresh-checkout artifact failure and the late unsupported-Node failure, then implement AVR3.1-AVR3.5 at the runner and Make seams. |
| 2026-08-17 | AVR3 | completed | Work commit `8042e32e4` makes the Make entry self-building and both entries fail fast on host input. Proof: `../proof/docs-and-app-verification-reliability/avr3.md`. AVRC11-AVRC12 2/2; 13 behavior cases; 24/24 mutations; live Node.js 22 Make and Node.js 24 direct runs pass 5/5 app assertions. |
| 2026-08-17 | AVR4 | started | Capture source-byte mutation on success and failure. Define the nine-case manifest, disposable workspace adapter, and fail-closed byte-manifest proof before implementation. |
| 2026-08-17 | AVR4 | completed | Work commit `827877d06` adds one nine-case manifest and an owned disposable-workspace adapter. Proof: `../proof/docs-and-app-verification-reliability/avr4.md`. AVRC13-AVRC15 3/3; behavior 6/6; preparation fixtures 9/9; mutations 24/24; three live success shapes passed 14/14 assertions and an expected live failure matched source bytes. |
| 2026-08-17 | AVR5 | started | Preserve default and explicit-file Compose discovery, define an explicit no-discovery mode, route both dev cases through it, and delete the tracked-file sideline and recovery path. |
| 2026-08-17 | AVR5 | completed | Work commit `02788d24b` adds `--no-compose-discovery`, preserves default and explicit behavior, and deletes the runner's tracked-file sideline. Proof: `../proof/docs-and-app-verification-reliability/avr5.md`. AVRC16-AVRC17 2/2; CLI 1,015 passed and 4 ignored; live dev cases 7/7; mutations 24/24; docs 109 and 17/17. |
| 2026-08-17 | AVR6 | started | Reproduce explicit and omitted local targets in isolated operator state. Close or disprove the stale authentication workaround without weakening tenant or credential boundaries. |
| 2026-08-17 | AVR6 | completed | Work commit `390bcaf27` separates host-admin discovery from application auth. Proof: `../proof/docs-and-app-verification-reliability/avr6.md`. AVRC18 1/1; run tests 11/11; CLI 1,019 passed and 4 ignored; live Tasks 5/5 with equal JSON and clean stdio; all four PR #238/#239 trust regressions passed. |
| 2026-08-17 | AVR7 | started | Inventory each runner resource owner and capture fail-before evidence for port races, cross-case operator-state reads, child-process leaks, fault cuts, cleanup retry, and retained failure artifacts. |
| 2026-08-17 | AVR7 | candidate complete | Work commit `bd2a8a364` gives each port, process group, root, log, credential, discovery record, and cleanup result one owner. Proof: `../proof/docs-and-app-verification-reliability/avr7.md`. AVR7 2/2; phase 20/20; mutations 24/24; lifetime 9/9; product 1/1; runner cuts 6/6; nine live apps and 37 assertions; affected Rust, Clippy, format, syntax, and static gates pass. Implementation PR 2 review is next. |
| 2026-08-17 | AVR7 | review correction candidate | Sol/xhigh/fast review `0.98` found three accepted defects. Commit `2215a5772` removes credentials from supervisor argv, makes EXDEV artifact retention retryable, and documents the control-data root. Lifetime 11/11, nine live apps and 37 assertions, phase 20/20, mutations 24/24, docs 109 and 17/17, and site build 110 are green. One narrow correction review is next. |
| 2026-08-17 | AVR7 | review complete | The one narrow Sol/xhigh/fast correction review scored `0.90`. It accepted the three original corrections and found one introduced credential-file cleanup defect. Commit `27331b144` registers the file with the exit owner, scrubs before retention, and retries deletion. Six cuts plus cleanup retry pass 7/7; all nine apps and 37 assertions, AVR7 2/2, phase 20/20, and mutations 24/24 pass. No further review is due. |
| 2026-08-17 | AVR7 | PR opened | Implementation PR [#276](https://github.com/nimbus/nimbus/pull/276) contains the frozen AVR3-AVR7 candidate. Hosted checks, owner-authorized merge, and current-main reconciliation remain. |
| 2026-08-17 | AVR7 | hosted correction | CodeQL check `95575973966` found three file-use races and one loopback-test data-flow alert. Commit `9e9482ce8` uses stable open handles, rejects linked credential files, records the validated loopback suppression, and gives graceful server exit a bounded pre-signal window. Lifetime 12/12, product 1/1, runner 7/7, all nine apps and 37 assertions, phase 20/20, and mutations 24/24 pass. |
| 2026-08-18 | AVR7 | hosted candidate green | PR #276 head `d5fa0117d` passed 54 checks with three expected skips and zero open CodeQL alerts. CI run `32092865987` attempt 2 reran three jobs that timed out in the Azure package mirror and passed them on fresh runners. The PR reports a clean merge state. Owner merge authorization and current-main reconciliation remain. |
| 2026-08-18 | AVR7 | completed | PR #276 merged as `b58ef8c35`. Reconciliation commit `ec6d2414c` merged current `origin/main` into the clean owner branch and preserved recovery checkpoint `0c20bf17c`; the branch then had zero commits behind current main. |
| 2026-08-18 | AVR8 | started | AVR7 and implementation PR 2 are merged and reconciled. Capture the console-only fail-before, then implement the versioned report schema, atomic write and recovery, credential redaction, validation, and deterministic JUnit projection at the application-verification report seam. |
| 2026-08-18 | AVR8 | completed | The report seam emits validated JSON and deterministic JUnit from manifest-ordered per-case records. Proof: `../proof/docs-and-app-verification-reliability/avr8.md`. Report 8/8; supervisor 2/2; lifetime 12/12; fault and retry 7/7; AVRC21-AVRC22 2/2; mutations 24/24. The final Node.js 24 run passed nine applications and 37 anchors in 83,725 ms with matching source digests and clean removal. |
| 2026-08-18 | AVR9 | started | AVR8 is durable and acceptance-green. Verify host activity, capture three serial samples, add bounded scheduling with drain-after-failure, then prove equal coverage, deterministic report order, isolation, and both time budgets across five valid parallel runs. |
| 2026-08-18 | AVR9 | implementation checkpoint | The dirty worktree owns bounded 1-9 worker scheduling, manifest-ordered logs and reports, targeted fault injection, first-failure drain, and worker signal forwarding. A nine-case four-worker run passed 37 anchors in 42,641 ms; a targeted two-worker fault left two terminal cases, seven not-run cases, and clean durable leases. Signal tests and the host-valid eight-sample benchmark remain. |
| 2026-08-18 | AVR9 | completed | Proof: `../proof/docs-and-app-verification-reliability/avr9.md`. AVRC23 1/1; evaluator 5/5; scheduler 2/2; fault and retry 7/7; mutations 24/24. On minicloud, three serial and five parallel samples each passed 9 applications and 37 anchors with matching source and clean removal. Medians were 112,403 and 67,066 ms; ratio 0.5967. |
| 2026-08-18 | AVR10 | started | AVR9 is durable and acceptance-green. Derive counts, correct nine-app and push-versus-polling text, document bounded workers and retained artifacts, remove stale text, and prove technical-writing baseline delta. |
| 2026-08-18 | AVR10 | completed | Work commit `04a675b29` derives all claims from the manifest, removes stale status, distinguishes update modes, makes UI support-file ownership explicit, and upgrades the docs processor without its deprecation warning. Proof: `../proof/docs-and-app-verification-reliability/avr10.md`. AVRC24 1/1; docs behavior 6/6; UI 336/336; full verifier and mutations 24/24; docs 109 and 17/17; build 110 pages. |
| 2026-08-18 | AVR11 | started | AVR10 is durable and acceptance-green. Reproduce AVRF22 at the canonical test entry point, isolate process-global network-authority tests without weakening duplicate-authority rejection, then run the complete local and minicloud matrices. |
| 2026-08-18 | AVR11 | candidate validation | `make test` now composes the serialized runtime lane, Nextest workspace lane, and doctests. A new RustSec database update exposed vulnerable `h2` lines and yanked `spin` lines during `make ci`; the dirty candidate removes them with narrow local dependency patches. Focused blob 248/248, S3 20/20, storage 448/448 with 2 documented fixture skips, dependency policy, attribution, lock, and workspace-hack drift checks pass. Exact-tree repository and minicloud gates are next. |
| 2026-08-18 | AVR11 | exact-tree acceptance | Final-tree `make test` and `make ci` passed. Local and minicloud each passed 9 applications and 37 anchors in serial and five-worker modes with matching source and exact cleanup. Minicloud also passed 662/662 server tests with 35 skips, and 43,652 source paths matched the owner worktree byte-for-byte. Finish the static gates, freeze, and one review. |
| 2026-08-18 | AVR11 | pre-freeze acceptance | AVRC and mutation checks passed 24/24; docs passed 109 pages and 17/17 site conditions; the site build emitted 110 HTML pages; five changed Markdown files had zero writing diagnostics; format and diff checks passed. Attribute and commit the complete candidate before its one Sol review. |
| 2026-08-18 | AVR11 | review correction | Candidate `056c243bc` is 8 commits ahead of current main and 0 behind. The GPT-5.6 Sol/xhigh/fast review scored 0.98 and reported six accepted findings: source snapshot flags, benchmark evidence binding, incomplete-case truth, worker cancellation, manifest-derived case count, and the website Node floor. Apply all six and run one narrow correction review. |
| 2026-08-18 | AVR11 | candidate frozen | Commits `e8fc63489` and `9f3fba2da` close all six full-review findings and both narrow-review closure findings. The narrow GPT-5.6 Sol/xhigh/fast review scored 0.96. Local affected proofs, AVRC 24/24, mutations 24/24, and final-tree minicloud focused, serial 9/37, and five-worker 9/37 runs pass. No further review is due. Open PR 3. |
| 2026-08-18 | AVR11 | PR opened | Implementation PR [#277](https://github.com/nimbus/nimbus/pull/277) opened from candidate checkpoint `f81c6cb7a`. Hosted checks and owner-authorized merge remain. |
| 2026-08-18 | AVR11 | hosted correction | Desktop UI run `32148666875` found the AVR10 settings support-file rename missing from one grep allow-list. Commit `358218c20` updates that exact path and documents its ShellCheck exception. The gate, ShellCheck, route codegen, and UI 336/336 pass. |
| 2026-08-18 | AVR11 | hosted candidate green | PR #277 head `897bff403` passed 54 checks with 3 expected skips and zero failures. CI run `32149119033`, Docs `32149119052`, CodeQL `32149118959`, Desktop UI `32149119101`, and Windows `32149118977` passed. The PR is clean and mergeable. Owner merge authority remains. |
| 2026-08-18 | AVR11 | completed | Owner-authorized PR #277 merged as `c9b551a30`. Reconciliation commit `d7c178523` combines the durable hosted-green checkpoint with current main and preserves the independent storage-integrity plan. |
| 2026-08-18 | AVR12 | started | Implementation PR 3 is merged and reconciled. Move the complete control plane to the archive, retain proof, replace active routing, run the AVR12 gates, and open the cleanup PR. |
| 2026-08-18 | AVR12 | cleanup candidate | The archived plan, retained proof root, and one index retrospective replace active routing. AVR and mutation checks pass 24/24. Documentation passes 109 pages and 17/17 site conditions. Diff and archive searches pass. The changed closeout prose has zero writing diagnostics; 44 older diagnostics remain in untouched index entries. Open the documentation-only cleanup PR. |
| 2026-08-18 | AVR12 | cleanup PR opened | Documentation-only cleanup PR [#278](https://github.com/nimbus/nimbus/pull/278) carries the archived control plane, retained proof, and routing retrospective. Hosted checks, owner-authorized merge, final audit, and clean worktree removal remain. |
