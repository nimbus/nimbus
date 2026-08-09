# Plans

This file is the routing index for current Nimbus roadmap work. It answers two
questions:

1. Which plan owns the work?
2. In what order should the owning plans run?

It is intentionally not a history catalog. Keep past execution records out of
this index so the next agent can read it as the current control plane.

## First Principles

- Launch safety comes before breadth. Fail-closed egress, tenant separation,
  policy auditability, and release channels outrank new compatibility surfaces.
- Trust-boundary substrates come before consumers. Build the filesystem, proxy,
  runtime, sandbox, and network seams before features that rely on them.
- Single-node correctness comes before cluster mode. Cluster plans should consume
  single-node primitives rather than re-owning them.
- Runtime and sandbox ownership stays separated. `nimbus-runtime` owns injected
  traits and execution interfaces; behavior crates own policy, storage, proxy,
  filesystem, and sandbox implementation.
- Drafts are not execution targets. Promote a draft only after it has scope,
  verifier/proof obligations, dependencies, and a paste-ready goal.

## Status Vocabulary

- `active` or `in_progress`: ready to execute or resume.
- `proposed`: coherent enough to review and promote, but still needs an explicit
  owner decision before implementation.
- `deferred`: scoped, but gated on a named consumer or deployment trigger.
- `draft` or `exploratory`: records candidate work; do not target with an
  implementation goal.
- `map`, `spec`, or `decision record`: routing or architecture context, not an
  implementation control plane.

Plans in the same phase are parallel-safe unless a dependency is named in the
bullet. Later phases should consume earlier seams instead of re-deriving them.

## Execution Order

### Phase 1 - Launch Safety And Egress Trust

- `architecture-review-2026-07-plan.md` - `active`. Control plane for the
  2026-07 full-workspace architecture review: guarantee/fail-closed repairs
  (storage write-path unification, EgressGateway pairing, decision-log
  fail-closed), seam repairs, consolidations, decompositions, test-infra and
  UI hygiene, doc/spec truth-ups, and the `nimbus-compute` extraction plus
  workload-identity decision records. Review-driven refactor and cleanup work
  should route through this plan's band ledgers while it is active.
- `nimbus-runtime-tenant-isolation-plan.md` - `complete` in PR #227. Canonical
  runtime-owner identity, routing-versus-reuse-authority separation,
  owner-partitioned
  V8/Wasmtime retained state, tenant and deployment retirement, and a
  compute-owned Nimbus runtime manager consumed by Convex, Cloud Functions,
  and future adapters. It consumes Engine/storage's durable tenant incarnation
  and lifecycle authority rather than minting a parallel one. `IsolateGroup`
  remains a separate deferred density/VM-shape question, not the tenant
  boundary.
- `distribution-plan.md` - `in_progress`. Owns binary release, Homebrew/cask,
  Linux package mirror, release-owned OCI images, and channel cutover. It should
  consume launch safety decisions rather than define them.

### Phase 2 - Runtime, Filesystem, And WASM Substrates

- `wasi-agent-capabilities-plan.md` - `deferred`. Starts only after the Wasmtime
  component linker, NimbusFS binder, and HTTP-client binder exist. Owns the
  process primitive and WIT projection layer; it must not re-own filesystem or
  proxy behavior.

Phase 2 coordination: if future filesystem work, Wasmtime, or WAC need runtime
bootstrap changes in the same window, create or reuse a shared
extension-registry seam before the second concern edits `extensions.rs`.

### Phase 3 - Network, Sandbox, And Machine Execution

- `nimbus-network-control-plane-plan.md` - `active; NNC6.5 complete; NNC6.5a
  acceptance green; commit pending`. NNC6.5 froze the source-derived teardown owner, caller, failure,
  path, and verification contract. NNC6.5a now owns the strict portable
  protocol, reducer, exact provision/restart handoff validators, server
  codec/schema, and named compile-green test fixtures. It owns no provider
  effect or caller cutover. NNC6.5b-NNC6.5g retain compute driver,
  ingress/node adapters, sandbox drain/stop and detach/release adapters, native
  caller cutover, Compose/machine cutover, and final compensation,
  tenant-retirement, and deletion convergence. NNCV035 remains the sole
  expected-red implementation condition until NNC6.5g. NNC6.1e's R1-R15
  portable discovery, pure recovery decisions,
  and distinct-process durability proof are green; its sole full Sol/xhigh/fast
  review reported zero findings at confidence `0.90`. NNC6.2 now owns the
  pure admitted-intent to exact `NetworkPlan` compiler. C1-C18 are green after
  correcting five accepted findings from its one full Sol/xhigh/fast review;
  one standalone display-name/profile finding is rejected from the public API
  and live test contract. Corrected affected behavior passes `1,477/1,477`
  with 27 declared skips, NNCV028 passes `18/18` plus `6/6`, and the complete
  204-case aggregate adversarial contract is green across a bounded prefix and
  exact continuation. Workspace, format/diff, Bash/ShellCheck, docs 108, and
  site `17/17` gates are green. Its sole narrow Sol/xhigh/fast review confirmed
  the behavioral corrections and found one accepted P3 arithmetic defect;
  exact `198 + 6 = 204` reporting is corrected and proven. Review cadence is
  exhausted. Exact item commit `0977c17d9` is durable. NNC6.2a's complete
  candidate now retains one strict saga-v2 compiled plan, one required physical
  object, derived phase tuples, the unchanged Engine adapter, a pure exact-plan
  reservation value, and bounded no-snapshot distinct-process replay. Its
  A1-A18 criteria are green after its one candidate-frozen full review accepted
  two P2 corrections: strict typed saga-v2 decoding now rejects duplicate keys,
  and the predecessor contract fails closed when its pinned commit is absent.
  Focused durability is `3/3`, affected behavior is `844/844` with 29 declared
  skips, NNCV029 is `23/23` plus `8/8`, aggregate live is `30/30`, complete
  mutation arithmetic is `188 + 10 + 7 + 8 = 213`, and all affected quality
  gates pass. Its sole narrow Sol/xhigh/fast correction review confirmed both
  implementations and found one accepted P2 test-only gap: the duplicate-key
  matrix lacked a direct inner-content field. The added duplicate
  `formatVersion` case passes focused `1/1` and workloads `96/96`; no executable
  code changed, and no third review ran or is warranted. Exact NNC6.2a item
  commit `ba7830360` is durable; NNC6.1e1 now owns the active recovery row. Its
  three-lane source audit found that the earlier omnibus ingress wording would
  persist `IntentCommitted` and then call coarse lifecycle methods that cannot
  execute one exact saga phase idempotently. The prospectively corrected
  NNC6.1e1 unit owns durable submission and confirmed pure decision only. Its
  I1-I20 matrix, bounded outcome contract, exact initial allowlist, crash and
  cancellation semantics, and later-owner map are frozen in
  `proof/nimbus-network-control-plane/nnc6.1e1-durable-workload-saga-ingress.md`.
  The bounded ingress remains candidate-frozen after its complete-item review.
  NNC6.3 is the read-only provision substitution audit. It prospectively
  splits executable durability and pure provision decisions into
  NNC6.3a-NNC6.3b. Revised NNC6.4 atomically adds real provider commands,
  replaces every native/runtime/Compose/Machine provision caller, and deletes
  every coarse path in the same item. NNC6.4a retains restart; the prospective
  NNC6.5a-NNC6.5g sequence retains teardown; NNC6.6 retains resolution caller
  fencing; NNC6.1e2 retains final fresh-process convergence.
  The existing sole coordinator now owns one effect-free `submit_intent` seam:
  one load, at most one CAS, one ambiguity read only after an ambiguous CAS,
  and no decision before exact confirmed durability. Missing, replay,
  successor, stale, divergent, conflict, ambiguity, crossed-key, and
  cancellation behavior passes `10/10`; corrected two-process convergence and
  pre/post durability crash cuts pass `2/2` with one child-only ignore and 20
  repeated lanes (`40/40` parent executions). The winner now waits for an
  atomic contender acknowledgement, and the pre-durability child enters a
  parked submission CAS before the parent kills it. Compute remains `120/120`
  plus one ignore; the unsplit server lane passes `640/640` with 29 skips.
  NNCV030 passes 10 direct checks and `12/12` mutations; aggregate live is
  `31/31`; affected quality, docs 108, and site `17/17` gates pass. I1-I18 are
  green. No effectful caller or manifest changed. The one full actual
  Sol/xhigh/fast review accepted those two process-proof defects plus one stale
  proof header at overall confidence `0.96`; production ingress semantics were
  judged consistent. All three findings are corrected. The sole narrow
  correction review is clean at `0.94`; no third review ran. I1-I20, docs 108,
  and site `17/17` are green. NNC6.3 completed the read-only row. Its
  source census proves that Container/Krun `start` is still monolithic, the
  saga lacks executable content, native callers lack admitted node/selection
  inputs, `ServiceManager` mixes naming and effects, and Compose bypasses the
  Engine-backed saga. Cloud Functions is corrected to a snapshot-only negative
  case. The sole Sol/xhigh/fast audit review found four planning defects, all
  accepted: temporary legacy authorities, missing publication ownership,
  missing definite-error behavior, and an incomplete machine/node sink census.
  The corrected split makes NNC6.3b pure and NNC6.4 one atomic replacement,
  assigns idempotent owner-local publication after workload readiness, records
  definite failures without later effects, and names every forwarded/guest/node
  start sink. The final gate also found that NNCV030 misclassified later
  canonical proof records as NNC6.1e1 source. Its narrow classifier correction
  keeps arbitrary product paths fail-closed; focused checks pass `10/10` and
  `13/13`, live verifier `31/31`, and retained mutations `228/228`. The sole
  narrow Sol/xhigh/fast correction review is clean at `0.99`; the four
  docs-only review corrections did not trigger another full review. NNC6.3a
  now owns the active strict executable-carrier and closed-digest row. E1-E20
  are green after its sole complete-item Sol/xhigh/fast review returned five P2
  and three P3 findings at confidence `0.98`. Seven findings are accepted and
  corrected; the claim that the nested successor decoder lacked
  `deny_unknown_fields` is source-rejected, with an exact nested-field
  regression added. Workloads owns one redacting bounded v1 carrier; compute alone
  encodes and strictly decodes `SandboxSpec`, the desired digest is derived
  over the complete intent, and the Engine adapter persists one required
  executable object. Decode errors no longer retain secret-bearing serde
  sources; a non-default egress rule round-trips exactly. The physical
  corruption proof now drives the real Engine execution-unit and store paths
  and proves exact document, four-index, and journal behavior. Full affected
  suites pass workloads `106/106`, compute `122/122` plus one ignore, and
  server `549/549` plus 30 ignores. NNCV031 passes `25/25` and `13/13`; its
  staged-path census, real Debug leak mutation, and strict legacy-format
  mutation fail closed. The live aggregate passes `32/32`, and exact retained
  plus additive adversarial coverage is `178 + 62 + 1 = 241`. Strict Clippy,
  proof lint, scripts, format/diff, docs `108`, and site `17/17` are green. The
  sole narrow Sol/xhigh/fast review validated every executable correction and
  found one accepted P3 in stale staged-state wording at confidence `0.98`.
  The ledger matched the reviewed 30-path candidate, and NNC6.3a is durable at
  `ed0560b4e45f7ec571934624962de72d021a71a8`. No further NNC6.3a review is
  warranted. NNC6.3b is complete under the frozen E1-E20 contract in
  `proof/nimbus-network-control-plane/nnc6.3b-pure-provision-decision.md`.
  It contains one exact composition constructor, an authenticated
  provider-report digest, explicit forwarding/TLS semantics, a pre-effect
  durable attempt, and a closed success/definite-failure/ambiguous reducer.
  E1-E20 are green. NNCV032 passes `32/32` direct conditions and `36/36`
  mutations, the live aggregate passes `33/33`, and the bounded retained-plus-
  additive run passes `277/277`. Full affected suites pass network `239` plus
  one ignore, workloads `125`, compute `147` plus one child-only ignore, and
  server `645` plus 30 declared skips. Affected quality, dependency/path,
  proof-lint, docs `108`, and site `17/17` gates pass. The exact candidate owns
  55 allowlisted paths and changes no manifest, provider caller, or effect
  owner. The one full Sol/xhigh/fast item review's eight accepted executable
  findings are corrected and proven; its admission-digest claim is source-
  rejected and its ledger placeholder is closed normally. The one permitted
  narrow correction review found one real activation-prerequisite bypass,
  which has exact `0/1` fail-before and corrected `1/1` proof; its constructor-
  validation claim is source-rejected. Review cadence is exhausted. The final
  pre-ledger executable patch SHA-256 is
  `ff0551ba284866427b2a62fc147e94ab1695f68f1089f2238bb97f0c81be1de3`.
  NNC6.4 completed the atomic provider-command and caller replacement at
  `6f4f909a06a20de1003d5aafc2f5ffcba43cf0bd`. Its evidence is in
  `proof/nimbus-network-control-plane/nnc6.4-atomic-provision-caller-cutover.md`.
  NNCV033 passes `40/40` direct checks and `50/50` mutations. The aggregate
  passes `34/34` and `327/327`. Affected non-CLI passes `2,502/2,502` with `79`
  skips, and CLI passes `936/936` with one skip.

  The completed item has one full Sol/xhigh/fast review and one narrow
  correction review. The proof records every finding disposition. The plan
  authorizes no further NNC6.4 review.

  NNC6.4a is complete. Its read-only substitution audit is complete at
  `proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md`.
  A1-A20 freeze one nested same-generation restart state, a separate
  execution-attempt identity, compute-owned schedule/count/watch and command
  authority, Container/Krun capability substitution, idempotent service/SDK
  ingress, exact race/crash/recovery proofs, deletion gates, later-owner
  boundaries, and the candidate path allowlist. Band R0 is durable at
  `6d8961bd6d4da819b2524128cb398e22e0a9382f`. R1 is durable at
  `d117ba369eaf5acc5ede9ec3edad32a11ddfbeb2`: the
  strict workloads-v4 record owns nested restart state, full admission
  history, exact execution-attempt fences, policy counts, deadlines, and pure
  recovery decisions. Pure behavior passes `17/17`; exact store behavior
  passes `9/9`; NNCV034 mutations pass `39/39`; and aggregate mutations pass
  `366/366`. The R2 seam audit strengthened NNCV034 to `68/68`
  sole-diagnostic mutations, froze R1 and R2 path ranges separately, and kept
  exactly the intended ten R2-R4 diagnostics red. It found two portable gaps
  that R2 must close before provider effects: record-owned claim/result
  transitions and one bounded, clock-free global restart-candidate query. R2
  closed those two portable gaps at
  `14e6236d4e3d1199a7ae40674bcdedd50b98fd58`: exact claims,
  inspection, absence-fenced retry, success receipts, terminal failure, and
  one strict Engine-backed global candidate page are durable. Workloads passes
  `165/165`. Server workload-store behavior passes `52` with `5` declared
  child-only ignores. Affected all-target, strict Clippy, warning-denied
  rustdoc, format, and static gates pass. NNCV034 remains `68/68`, with exactly
  the intended nine R2-R4 diagnostics red and no path violation. The normalized
  compute admission reducer and sole-coordinator CAS are durable at
  `8935e0c77dd188f50566b72c917b2005a213ecdd`: all `8/8` reducer, race,
  idempotency, deadline, and cancellation tests pass, and full compute passes
  `241` with one declared child-only ignore. Private confirmed command and
  exact result reduction are durable at
  `e1e9c95167972c2566a468d01aa0b91e559dd9be`: only the direct claim-CAS
  winner executes; replay, ambiguity, and fresh-process recovery first persist
  inspection; only exact absence advances the same attempt by one dispatch
  epoch; crossed results fail closed; and definite failure stops. Focused
  command/ambiguity behavior passes `10/10`; full compute passes `251` with one
  declared child-only ignore. R2 provider orchestration is durable at
  `5826dff6019d453f9eba575ecf67850ac3b19e6a`: compute owns the exact
  registry, dispatcher, driver, supervisor, deterministic clock, and bounded
  durable watch; the shared provider journal owns restart ordinals and exact
  attempt fences; and real Container/Krun plus server ingress/listener
  capabilities are green. A provider realm opts into restart only after it
  earns the complete set, so forwarded machine remains truthfully unregistered
  until R3. Focused restart behavior is `55 + 69 + 20`; full
  compute/sandbox/server/services is `289 + 1 ignore`, `1,005 + 27 ignores`,
  `594 + 31 ignores`, and `82`; final composition/CLI adaptation is
  `10 + 6 + 3`. Affected all-target, strict Clippy, warning-denied rustdoc,
  format/diff/syntax, and NNCV034 `71/71` pass. R3 verifier extraction
  is durable at `e1c57ce9eba823b8dc7d0a5a4ab02d58fe4c0030`: the `1,527`-line
  production contract and `471`-line fixture sibling reuse one scanner, and
  R2/R3 now have separate frozen path ranges. R3 service/SDK is durable at
  `99930529633e02f027ae17adaa8d7379a73af37b` and
  routes authorized explicit restart through one compute submitter and the
  existing retained supervisor. Exact completed replay, source-generation
  rejection, provider convergence, and zero coarse stop calls pass. Full
  workloads, compute, and serial process-global-authority Server behavior pass
  `167`, `292 + 1` ignore, and `596 + 31` ignores; strict affected Clippy is
  green. The Nimbus package build/test/typecheck passes with `24`-route parity.
  NNCV034 remains `71/71`, and the live contract is red only for machine,
  scheduler, and final behavior. Compose, machine, and scheduler cutovers are
  next. No structured review ran on that partial checkpoint.

  Final NNC6.4a closeout satisfies A1-A20. The narrow-review corrections prove
  observation-absence republish, inspect-before-terminal successor veto,
  later-veto read-only node inspection across every machine boundary, and
  assertion-quality verifier enforcement. Final affected behavior passes
  workloads `172`, compute `303 + 1 ignore`, sandbox `1,004 + 27 ignores`,
  machine `34`, node `72`, server `692 + 32 ignores`, CLI `948 + 1 ignore`, and
  services `82`. NNCV033 passes `40/40 + 50/50`; NNCV034 passes `86/86`; the
  aggregate passes `35/35 + 413/413`; docs pass `108`; and the site gate passes
  `17/17`. One full and one narrow Sol/xhigh/fast review are fully
  dispositioned. No third review ran or is warranted. Exact item commit
  `a37a87f86ee80252812fda66d33d23f05e73d0d4` closes NNC6.4a. NNC6.5
  teardown audit A1-A23 are green and candidate-frozen in
  `proof/nimbus-network-control-plane/nnc6.5-teardown-choreography-substitution-audit.md`.
  It owns only the plan/proof/NNCV035 expected-red files and awaits its sole
  item review. Product source work starts with NNC6.5a only after NNC6.5 closes.
  Network resource identity is retained
  tenant-qualified and rederived for every attachment, route,
  listener, endpoint, and lease; it never uses the mutable decision digest,
  address, or port. Source-bearing sovereignty cannot be relaxed, and the
  exact portable content authenticates the complete plan envelope. Its completed
  audit found that the current saga tuple cannot reconstruct full desired
  network state before first reservation; the corrected carrier now reconstructs
  it exactly. NNC6.2a owns the durable compiled-plan carrier and fresh-process
  replay. NNC6.1e1 now owns the bounded durable submission seam without caller
  cutover. Workloads-owned execution identity and generation now reach
  operational node paths, false in-memory desired-state authorities are
  deleted, and status, systemd, cgroup, and exported journal evidence
  correlate one exact identity without cross-record stitching. The sole full
  review's two P2s and sole narrow review's one P2 are corrected and proven;
  no third review ran. The Linux sovereignty tripwire is
  complete with a root-safe,
  raw-artifact-derived evidence contract, 70/70 deterministic tests, 62/62
  adversarial verifier cases, fully dispositioned full+narrow Sol reviews, and
  two source-matched LinuxKit PASS runs whose ordered pair proves fresh-process
  cleanup/re-entry. NNC5.2 is complete: one manager-derived, tenant-qualified
  attachment authority persists exact resource version, generation, digest,
  lease epoch, phase, selected provider, and stable opaque handle. Both actual
  Container/Krun production routes authenticate that durable owner plus
  provider inspection before effects. The final production routes pass 4/4,
  portable authority/state passes 15/15, lifecycle/routes pass 38/38, the
  backend lane passes 332/332, and affected crates pass 1003/1003 with 24
  declared skips. Its full review's sole proof gap is corrected by four
  constructor/private-route tests and the one narrow correction review is
  clean at 0.99. No further NNC5.2 review ran or is warranted. NNC5.2a is now
  complete: exact attachment-to-segment association and one sandbox-owned
  provider-attempt journal precede every attachment effect across Container,
  Krun, and machine-forwarded routes. Its full and sole narrow Sol reviews
  found two final-detach recovery defects; both have exact fail-before proofs,
  corrected retry behavior, and full affected gates. Final lifecycle is
  41/41, portable 233/233 with one subprocess skip, backend 332/332, sandbox
  785/785 with 24 declared skips, verifier 18/18 plus 67/67 adversarial cases,
  docs 108, and site 17/17. Before executable work, the original NNC5.2b scope
  was prospectively split into three acceptance-bearing items: exact durable
  evidence/candidate enumeration (`NNC5.2b`), the pure eight-row classifier
  (`NNC5.2c`), and exact startup quarantine plus filename-authority deletion
  (`NNC5.2d`). NNC5.2b is complete. One tenant-qualified
  sandbox/provider/artifact-realm locator and exact generation-bound
  provider-attempt identity persist before effects; typed IPAM partition
  enumeration validates every durable key; least-authority reader adapters
  form a deterministic desired/provider union with exact claim-qualified
  allocator observations and untrusted present/absent/unknown artifact
  evidence. Provider evidence authenticates the pinned directory capability,
  and terminal transition/replay/retirement authenticate exact realm/backend
  for both safe phases. The full review's four and sole narrow review's two
  accepted defects are corrected with exact fail-before proofs; no third
  review ran. Final orphan evidence passes 18/18, the focused lane passes
  88/88, affected crates pass 1048/1048 with 26 skips, verifier passes 18/18
  plus 67/67 adversarial cases, and docs/site gates pass. NNC5.2c is complete
  at `ae29108f3bd2037557727e0036cf0f7ebfc039c0`: its pure total classifier
  covers the required eight-row matrix, returns only `Adopt` or one of 19
  named quarantine reasons, and explicitly proves exact, missing, and
  substituted provider handles across all four desired phases. Classifier
  tests pass 10/10, orphan evidence passes 28/28 plus one declared child skip,
  and affected crates pass 1058/1058 with 26 declared skips. Its one full and
  one narrow Sol/xhigh/fast reviews are fully dispositioned; no third review
  ran. NNC5.2d is complete: its shared startup adapter,
  exact desired/allocator quarantine, both real backend admission fences, and
  filename-authority deletion are implemented. Integration added a
  deliberately narrow read-only retention row for the legitimate
  desired-absent, reserved, no-provider-effect restart state; its real
  fail-before was 0/1 and six unsafe variations remain quarantined. Final
  classifier, state-machine, and real-backend startup proofs pass 12/12, 8/8,
  and 4/4. Affected crates pass 1070/1070 with 24 declared skips; check,
  strict Clippy, warning-denied rustdoc, core-only dependency, verifier 19/19,
  mutations 72/72, multi-tenant verifier 16/16, docs 108, and site 17/17 pass.
  Its single candidate-frozen Sol/xhigh/fast review returned zero findings at
  confidence 0.87; no repeat review is warranted. NNC5.3 is complete: one
  read-only host-managed Container/Krun composer authenticates exact
  desired/durable attachment, attempt-bound Netavark/IPAM/status,
  active-table and real-shape nft pin, listener lifetime, and PEP evidence.
  Its full review's four findings and sole narrow review's two findings have
  exact fail-before and corrected proofs; focused readiness passes 30/30,
  sandbox passes 859/859 with 21 skips, and no third review ran. NNC5.3a
  completed the machine-forwarded gvproxy/route/worker/listener evidence.
  NNC5.4 now proves real-process convergence at every named shared host-managed
  attachment create/delete boundary for both Container and Krun. Its complete
  120-child matrix derives exact attachment/version/association/epoch,
  provider handle, IPAM, allocator, and listener fences from one synced
  immutable pre-crash witness. The sole full Sol/xhigh/fast review found four
  accepted defects and one source-rejected nonexistent phase; the one narrow
  correction review found three accepted defects. All seven accepted findings
  have exact fail-before and corrected proof. Durable recovery passes 6/6,
  lifecycle 59 with five child skips, Sandbox 870/870 with 26 skips, live
  verifier 21/21, adversarial mutations 101/101, and affected quality/docs
  gates are green. No third review ran or is warranted.
  NNC5.4a is complete. It replaces process-local publication outcomes with one
  strict durable
  `Absent -> Exposing -> Exposed -> Withdrawing -> Absent` record, exact
  per-slot ambiguity, a small real/substitutable provider capability,
  inspect-before-effect retry, real-process crash cuts, and no-release
  ordering. Its expected-red packet was exact: the focused command exited
  101 with 0 passed, 4 failed, 0 ignored, and 896 filtered. Direct
  expose/withdraw response loss and lifecycle expose/fresh-owner-withdraw
  cases fail only on duplicate native mutation counts; the lifecycle cases
  preserved exact durable network authority. The corrected item passes
  `nnc5_4a_` 17 with one child-only ignore, five real-process parent tests with
  one child-only ignore, forwarding 20/20, provider cleanup 30/30, Sandbox 898
  with 27 intentional ignores, verifier 22/22, adversarial mutations 111/111,
  verifier self-test 112/112, AST 12/12, affected quality gates, docs 108, and
  site 17/17. The sole full Sol/xhigh/fast review's four accepted and two
  rejected findings are dispositioned. Its accepted executable corrections
  warranted one narrow review; that review's sole IPv4-mapped IPv6 defect is
  corrected and proven. No third review ran or is warranted. NNC5.5's
  implementation and R1-R11 acceptance convergence are complete after the
  sole full review's five accepted corrections. One shared
  bounded readiness provider replaces the two direct probe owners; nft
  readiness has observation-only authority; namespace effects are private to
  the host adapter; and NNCV004/NNCV012/NNCV022/NNCV023 seal the exact portable
  boundary, production effect census, and privileged capabilities. Final
  correction behavior is 1,156/1,156 with 28 declared skips, live verifier
  24/24, isolated mutations 27/27, and aggregate mutations 139/139. The sole
  full Sol/xhigh/fast review ran at 0.98; its five findings are corrected and
  proven. The sole narrow correction review confirmed four corrections and
  raised one claim about `container/runtime/lifecycle.rs`; exact module routing
  proves that file is test-only, so the claim is rejected. No third review is
  warranted. NNC5.5 is complete in one exact checkpoint; no push or PR.
  NNC5.6 has R1-R14 green after its full and narrow item reviews. One typed
  sandbox-owned inspection carries handle, execution, restart, cleanup, and
  opaque snapshot evidence through services, compute, Compose,
  guest/forwarded Machine API, and every backend implementation. Container and
  Krun inspection now use
  existing-only shared locks, pure restart assessment, and bounded
  child/process-group observation; they cannot restart, clean, repair PEP,
  release authority, publish endpoints, or write manifests. Exited/absent
  retained workloads remain non-publishable `Stopping`, and crossed
  ID/tenant/name/backend evidence is rejected before cache or lifecycle
  effects. The full Sol/xhigh/fast review's seven accepted findings are
  corrected: guest finality is monotonic, retained Compose ownership blocks
  replacement, command output is truly bounded, process-group/reap ownership
  is explicit, external evidence participates in comparison versions, and
  contradictory PlanOnly states cannot publish. A related post-reap numeric
  process-group race exposed during full-suite convergence is also corrected.
  The sole narrow correction review found two incomplete corrections:
  unavailable inspection still authorized Compose replacement, and two
  Container terminal branches omitted raw runner-handoff bytes from their
  versions. Both reproduced exactly as 0/2, correct to 2/2, and pass full
  affected gates. Final affected behavior is 2,104/2,104 with 27 declared skips;
  live verifier is 25/25 and aggregate mutations are 158/158. NNC6.4a still
  owns desired-generation-fenced restart; NNC8.3 still owns orphan
  cleanup/finalization/reuse. Both permitted Sol/xhigh/fast reviews are
  dispositioned, no third review ran or is warranted, and NNC5.6 is one exact
  69-path HEAD checkpoint. NNC6.1 now carries one existing shared manager from
  CLI composition through server construction into compute. Its sole full
  item review accepted two boundary corrections: direct compute construction
  now fences workload managers without that shared manager, and the identity
  proof uses the production router-state handoff. Both corrections pass
  focused behavior and exclusive NNCV025 mutations; the aggregate is 173/173.
  The one permitted narrow correction review is clean at confidence 0.98; no
  further NNC6.1 review is warranted. NNC6.1a's read-only audit, acceptance
  freeze, exact NNCV026 expected-red, and product implementation are complete.
  Its small compute coordinator, type-erased node capability,
  provider-restart fence, and both caller routes are converging. The first
  compile proved the direct CLI-to-compute dependency and lockfile metadata are
  required; both were added to the frozen scope before either dependency file
  was edited. Corrected focused behavior is 11/11 plus restart 1/1; affected
  behavior was 1,059/1,059 with one child-only skip. The sole full Sol review
  found two P3 issues: the standalone route masked typed reconcile errors, and
  R3 lacked its valid protocol-only `None` assertion. Both are corrected.
  Correction behavior passes 2/2, affected behavior passes 1,060/1,060 with
  one child-only skip, and live verifier 27/27 passes. The direct-bypass
  mutation fails only NNCV026 at 26/1, and all 14 affected NNCV026 mutations
  pass exclusively. The earlier complete aggregate remains 187/187. The
  Linux/session-systemd executor lane is cfg-excluded on the macOS owner host
  and is not claimed as live provider evidence. The one narrow correction
  review is clean at confidence 0.98, and no further NNC6.1a review is
  warranted. NNC6.1b has candidate-frozen the workload-saga decision before
  implementation: `nimbus-workloads` owns portable state and the object-safe CAS
  port, `nimbus-compute` is the sole transition writer, and `nimbus-server` owns
  the `_nimbus._workload_sagas` adapter through
  `Engine::begin_mutation_execution_unit`. The decision proof records the exact
  current census, phase graph, schema, ambiguity rules, dependency edges, and
  seven expected-red implementation gaps. Its sole full Sol review found five
  accepted defects and one claim rejected against the current three-path
  Engine contract. The corrected decision now defines higher-generation
  successor promotion, mandatory typed recovery evidence, complete-payload
  transition IDs, lossless decimal counters, and an exact full-census helper.
  Final census `7/1/2/3/54/0`, decision `1/1`, expected-red `0/7`, live
  structure `27/27`, writing/script/format/diff, docs `108`, and site `17/17`
  pass. The full review's five accepted findings and the sole narrow review's
  two accepted precision gaps are corrected; one mutation-path claim is
  source-rejected, and no third review ran. NNC6.1c's portable vocabulary,
  store port, direct edges, uncomposed coordinator, and verifier adjustment are
  implemented. Its pre-review acceptance audit found three executable defects
  and four coverage, cardinality, or ledger gaps; all seven are corrected. The
  sole full Sol/xhigh/fast item review then found eight defects. All eight have
  fail-before or direct source evidence and corrected proofs. Corrected R1-R14
  are green at saga/store/coordinator `31/19/10`, affected libraries `243/243`,
  live verifier `27/27`, and final NNCV026 mutations `15/15`. The sole narrow
  correction review found one P2: the cardinality scan accepted non-struct
  duplicate owners. The corrected scan covers struct, enum, union, trait, and
  type declarations, and independent struct and enum mutations fail closed.
  No third review ran. NNC6.1c is committed at `a0a802ea7`. NNC6.1c1 has
  completed implementation and R1-R14 against the exact path allowlist in
  `proof/nimbus-network-control-plane/nnc6.1c1-operational-identity-authority-cutover.md`.
  Workloads-owned execution identity and generation now drive node lifecycle.
  Crossed or missing node assignments fail before effects. Observed generation
  is lossless. Both false in-memory authorities and all three services writes
  are deleted; the item is durable at `a098db7b5`, and no further NNC6.1c1
  review is warranted. NNC6.1d records its implementation and R1-R20 contract in
  `proof/nimbus-network-control-plane/nnc6.1d-durable-workload-saga-store.md`.
  Its pre-review candidate passed R1-R20: store `21/21`, compute `15/15`,
  reserved prefix `21/21`, affected `1,537/1,537` with 32 skips, live verifier
  `28/28`, aggregate self-test `188/188`, direct modes, affected quality gates,
  docs `108`, and site `17/17`. The sole full Sol/xhigh/fast item review found
  one P2 mutable-phase recovery-cursor defect and two P3 proof-harness defects;
  all three are accepted and corrected. R1-R20 are green again: store `23/23`,
  full workloads `67/67`, affected `1,539/1,539` with 32 skips, immutable
  saga-ID inter-page recovery, bounded child terminate/reap, exact verifier
  child `27/1`, live verifier `28/28`, and aggregate mutations `188/188` pass.
  The one narrow Sol/xhigh/fast correction review is clean at confidence
  `0.97`; no further NNC6.1d review is warranted. NNC6.1d is durable at
  `60c0a1b23`. The NNC6.1e audit found that the original row combined durable
  tenant discovery, pure recovery decisions, admitted lifecycle ingress,
  provision/restart/teardown effects, and final convergence. It is split before
  implementation: NNC6.1e owns bounded tenant paging plus the effect-free
  all-phase decision proof; NNC6.1e1 follows NNC6.2a and owns durable submission
  plus confirmed pure decisions; NNC6.1e2 follows NNC6.3a-NNC6.3b and
  NNC6.4-NNC6.6 and closes startup recovery plus tenant retirement. NNC6.1d
  retains the durable server adapter; the NNC6.3 split plus NNC6.4-NNC6.6
  retain caller cutover and provider choreography; and NNC8.3 retains cleanup
  finalization/reuse.
  The candidate proves affected behavior `195/195` with one child-only ignore,
  live verifier `28/28`, focused NNC6.1e mutations `10/10`, aggregate mutations
  `198/198`, docs `108`, and site `17/17`; no provider-effect path changed. The
  exact reviewed staged tree is `8c9d522263522a874848e6a9516c86fcad931e86`;
  the exact item is durable at `2204fa8d7`, and no correction review was
  warranted. NNC6.2 begins with a read-only
  substitution/caller audit and a frozen fail-before proof before product edits.
  It also requires the first workload-capable compute injection. No temporary
  no-op or production in-memory store exists.
  Cleanup, release, finalization, and reuse stay in NNC8.3. Machine forwarding obeys
  NNC5.2a's common lifecycle; runtime absence, workload status, readiness persistence,
  cleanup convergence, and inspect/restart policy stay in their named later
  owners. Sole owner for the transport-free connectivity-resource control
  plane: portable network identities and plans,
  crash-safe segment and cross-process host-port lease authority,
  generation/epoch fencing, capability satisfaction, reconciliation, exact
  provision/teardown choreography, and sovereignty proof. It must stay below
  tenant, sandbox, services, machine, KV, compute, server, system, and future
  cluster. NNC0.0 first makes the force-tracked owner plan durable in branch
  history; the remaining NNC0 items capture the complete dependency/bind census,
  real-process proof harnesses, and executable failure baselines before
  extraction. Its workloads/compute, sandbox, egress/proxy, service, system,
  and horizontal-scaling integrations consume those owners rather than forking
  desired-workload state, policy, effects, projections, or cluster transport.

- `nimbus-sandbox-plan.md` - `proposed`. Owns the multi-backend sandbox
  architecture (`ADOPT_MULTI_BACKEND_SANDBOX_ARCHITECTURE`, 2026-07-08): the
  `SandboxBackend` router/dispatch seam with backend families
  (container/krun landed; libkrun_session, firecracker, isolate, wasm,
  gvisor, cloud_hypervisor, qemu named), the family/profile/capability
  vocabulary,
  and the libkrun-family session bands (shared backend skeleton, desktop
  profile, GPU profile). Band R (router) precedes any new backend family;
  Band B precedes the desktop/GPU bands.
- `firecracker-fast-invocation-backend-plan.md` - `proposed`. Owns the
  `firecracker` backend family: snapshot-backed fast invocation on stock
  Firecracker (sidecar + jailer, snapshot/restore, template fork) behind the
  Band R dispatch seam. Replaces the removed in-fork snapshot port (former
  sandbox-plan Band S) and precedes density work that assumes snapshot/fork
  scale. Promote when the `fast_invocation` product surface is scheduled and
  Band R is landed.
- `sandbox-disk-limit-enforcement-plan.md` - `proposed`. Owns host-layer
  project-quota detection and enforce-or-refuse startup semantics for per-service
  disk caps. Promote when a concrete service needs production disk limits.
- `windows-machine-support-plan.md` - `deferred`. Owns Windows developer-machine
  support using a Windows-native CLI plus WSL2 machine provider. Promote when
  Windows development support is scheduled.

### Phase 4 - Connection Residency, Secrets, And Service Identity

- `secret-management-plan.md` - `deferred`. Owns tenant-scoped secret references,
  providers, storage, cache invalidation, host bridge, and audit model. Promote
  when a named consumer needs stronger-than-env-var credentials.
- `service-identity-provider-auth-plan.md` - `in_progress`. Owns workload identity
  minting into provider credentials. SI0 landed (PR #126: `nimbus-workload-identity`
  crate — provider-auth policy, admission-anchored mint authorization, claim set,
  audit schema); SI1+ remain gated (production minting on HS1, provider adapters
  on a concrete provider need). It consumes workload identity and cluster
  node identity; it does not own secret values.
- `agent-browser-service-plan.md` - `deferred`. Owns the built-in browser
  service, session/profile storage, warm pool, Playwright-compatible surface,
  and sandboxed-Chrome production provider. Its credentialed flows should consume
  secret management and service identity rather than inventing a parallel model.

### Phase 5 - Cluster And Cross-Node Work

- `horizontal-scaling-plan.md` - `deferred`. Leads all multi-node work. Owns node
  identity, discovery, membership, placement replication, gossip invalidation,
  isolate placement, microVM placement, content distribution, and the
  cluster-mode integration layer, including HS5's per-Durable-Object placement,
  shared lease authority, and epoch-fenced protected writes. It supplies
  raft-committed fenced node super-net leases to the canonical
  `nimbus-network` allocation contract; it does not own a second segment
  allocator, provider effects, or local network state store.
- `nimbus-fips-iroh-ed25519-retrofit-plan.md` - `draft`. Owns a future
  aws-lc-rs/PQ TLS posture, NodeSigner seam, and CMVP-triggered identity-key
  retrofit. Promote only after the current FIPS and iroh identity facts are
  refreshed from primary sources.
- `nimbus-private-registry-cache-isolation-plan.md` - `draft`. Owns tenant
  isolation for authenticated/private registry blob-cache hits. Promote when
  authenticated registry pulls become real product scope.

### Phase 6 - Demand-Gated Policy, Admission, Transport, And Density

- `archive/parallel-prepare-serial-commit-plan.md` - `complete, archived`
  (2026-07-23; PRs #188–#236). Delivered the per-tenant Convex-parity
  committer: parallel prepare, serial assignment, ordered publication,
  bounded conflict retry, provider lease fencing/pipelining, deterministic
  crash/replay and differential verification, and pinned external Elle
  evidence. FINAL recorded all readiness dimensions `PASS` and all closeout
  verdicts `SAFE`.
- `archive/storage-unification-and-carryover-plan.md` - `complete, archived`
  (2026-07-30; PRs #248–#261; decisions U1–U10). Delivered
  the 2026-07-29 storage review's ranked wins (CommitTransaction witness,
  objects/KV/scheduler onto a real commit path, provider facade extraction,
  DynamoDB principal) plus every carry-over: main full-CI triage (RED at
  plan creation; gates all phases), the July 21 HIGH-bug triad, the
  open-loop latency companion, the resource-binding decision, and the
  formal hot-key/OCC closure.
- `archive/storage-follow-ups-plan.md` - `complete, archived` (2026-07-31;
  PRs #262–#270). Closed every executable follow-up from the
  storage-unification campaign: the PPSC arm-theft fault-interface fix
  (`DurableApplyKind`), the MySQL scheduler outlier, DynamoDB batch stream
  staleness plus stream/scan read-policy bypasses and the GetRecords
  amplification ceiling, the nimbus-core lifecycle-predicate planner bug,
  BatchWriteItem validation ordering and rejection rules, the nimbus-fs
  object-write seam, schema-redeclaration write-amplification, three
  root-caused flake families, and the SUC6.2 literal measurement (U7
  override; measured REJECT). FU13 (400 KiB ceiling on the remaining
  DynamoDB write paths) and FU14 (extenddb nested-collection sizing,
  upstream) remain scoped open tickets recorded in the archived plan.
- `archive/sqlite-write-throughput-optimization-plan.md` - `complete, archived`
  (2026-07-28). Campaign PASS in one day: prepared-statement reuse +
  batch-invariant apply context (PR #244), resident writer (PR #245),
  attribution-rejected forward apply (PR #246), final acceptance ratio
  1.836 over `B_ref` with `F_ref` 51,399 N=256 mut/s (PR #247) — 2.4×
  the pre-campaign 21,433 observation. Hot-key +163%, N=1 ≈3.7×,
  storage lane +185%. Full A/B, fail-before, and rejected-run evidence
  under `proof/sqlite-write-throughput/`.
- `archive/clock-architecture-reliability-plan.md` - `complete, archived`
  (2026-07-21; independent Opus 4.8 review clean). Delivered canonical wall and
  monotonic clock seams, Engine-owned absolute scheduling, monotonic local
  duration policies, explicit temporal-validation observations, clock-source
  guards, and structural gates that leave unproved distributed clock authority
  disabled under the horizontal-scaling plan.
- `layered-admission-control-plan.md` - `deferred`. Owns future layered
  admission experiments and EO8-style promotion work. Consumes the
  retry-amplification admission signal from
  `archive/parallel-prepare-serial-commit-plan.md`.
- `native-transport-evolution-plan.md` - `proposed`. Owns benchmark-driven
  Nimbus-native transport evolution without replacing the established WebSocket
  protocol by default.
- `enterprise-crate-adoption-plan.md` - `proposed`. Owns the cross-workspace
  screen for mature Rust crates at commodity substrate seams: Sigstore artifact
  verification, DNS, OIDC/JWKS, OCI spec types, local-socket HTTP parsing,
  policy engines, telemetry, object-storage breadth, QUIC/H3, crypto/TLS
  provider posture, and path-capability primitives. It is a routing and
  promotion control plane; individual rows execute in their owning substrate
  plans after current-source and dependency-graph proof.
- `nimbus-tenant-admission-audit-plan.md` - `draft`. Owns aggregate admission,
  OCSF spool, H-gate ratification, and admitted quota enforcement. Consumes the
  node-scoped `EgressEngine` seams landed by the completed
  `archive/nimbus-egress-engine-plan.md` (decision-event fan-out sink, per-tenant
  fairness budget values).
- `nimbus-proxy-policy-hardening-plan.md` - `draft`. Owns proxy policy
  hardening adjacent to K11P without moving transport dependencies into runtime,
  sandbox, or policy crates.
- `nimbus-sandbox-egress-regression-and-seams-plan.md` - `draft`. Owns KVM proof
  lanes, path-filtered checks, readiness parity, port-pool hardening,
  netavark/firewall determinism, and tenancy hardening.
- `nimbus-proxy-density-and-datapath-plan.md` - `draft`. Owns shared-listener
  and possible eBPF/Aya datapath work. Promote only after K11P, sandbox
  snapshot/fork scale, and source-IP spoofing proof exist.
- `nimbus-masque-h3-egress-plan.md` - `draft`. Owns future QUIC/H3/MASQUE
  egress support. Until promoted and proven, proxy-required rules must deny
  QUIC/UDP bypass.
- `nimbus-libkrun-2-migration-plan.md` - `draft`. Owns the future libkrun/crun
  2.x fork migration after tuple validation and ABI-doctor baselines are ready.
- `nimbus-run-exec-exploration-plan.md` - `exploratory`. Records the possible
  raw argv surface `nimbus run exec -- ...`.
- `nimbus-run-jobs-exploration-plan.md` - `exploratory`. Records the possible
  named workload template surface `nimbus run jobs <name>`.

## Routing Aids And Specs

These documents may inform plan work, but they are not implementation targets:

- `nimbus-modernization-roadmap-plan-map.md` - row-to-owner map for the
  modernization roadmap. Use it before creating another plan.
- `storage-seams-architecture.md` - governing storage/filesystem/object/volume
  seam spec.
- `profile-aware-isolate-runtime-final-architecture-plan.md` - runtime decision
  record for profile-aware isolate pools, snapshots, code cache, pointer
  compression posture, and adaptive-autoscaling constraints.

## How To Use This Folder

- Start with the plan that owns the concrete workstream.
- Resume `in_progress` work before promoting a new plan in the same lane.
- If a draft owns the right topic, promote it before implementation by adding
  scope, sequencing, verifier/proof obligations, and a paste-ready goal.
- If no plan owns the topic, add or promote exactly one owner plan and update
  `nimbus-modernization-roadmap-plan-map.md` if the work came from the roadmap.
- Keep past execution records out of this README; this file should remain
  readable as the current execution order.
