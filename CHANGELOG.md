# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.46] - 2026-08-28

### Added

- **storage**: Run metadata retention in engine lifecycle by @jackspirou
- **storage**: Add embedded retention checkpoints by @jackspirou
- Add bounded materialized verification sessions by @jackspirou in [#308](https://github.com/nimbus/nimbus/pull/308)
- **storage**: Add deterministic verification root by @jackspirou in [#305](https://github.com/nimbus/nimbus/pull/305)
- **engine**: Make a restarted materialized table load visible by @jackspirou in [#297](https://github.com/nimbus/nimbus/pull/297)
- **cli**: Scaffold Nimbus-native projects with nimbus init nimbus by @jackspirou in [#282](https://github.com/nimbus/nimbus/pull/282)
- **cli**: Add dev compose discovery opt-out by @jackspirou
- **sandbox**: Converge startup network orphans by @jackspirou
- **network**: Fence provider command live claims by @jackspirou
- **system**: Make connectivity projections independent by @jackspirou
- **system**: Project stable network connectivity by @jackspirou
- **network**: Add portable sandbox status handles by @jackspirou
- **services**: Preserve endpoint identity generations by @jackspirou
- **server**: Supervise sibling listener groups by @jackspirou
- **compute**: Converge startup and tenant recovery by @jackspirou
- **network**: Fence service resolution during restart by @jackspirou
- **compute**: Converge workload teardown ownership by @jackspirou
- **machine**: Fence physical stop with workload authority by @jackspirou
- **compose**: Route retirement through compute by @jackspirou
- **network**: Complete forwarded machine teardown provider by @jackspirou
- **machine**: Add forwarded teardown parent authority by @jackspirou
- **machine**: Add exact teardown phase transport by @jackspirou
- **sandbox**: Compose forwarded attachment teardown by @jackspirou
- **machine**: Compose guest teardown execution by @jackspirou
- **sandbox**: Fence container provision and teardown by @jackspirou
- **node**: Fence systemd activation before drain by @jackspirou
- **machine**: Compose durable systemd teardown providers by @jackspirou
- **machine**: Add strict teardown phase wire by @jackspirou
- **network**: Extract confirmed teardown provider journal by @jackspirou
- **network**: Fence forwarded teardown receipt history by @jackspirou
- **network**: Complete host-managed attachment teardown by @jackspirou
- **network**: Add exact krun execution teardown by @jackspirou
- **sandbox**: Complete NNC6.5d1 execution teardown by @jackspirou
- **network**: Complete NNC6.5c teardown adapters by @jackspirou
- **compute**: Complete NNC6.5b teardown driver by @jackspirou
- **workloads**: Add strict teardown protocol by @jackspirou
- **network**: Complete fenced workload restart cutover by @jackspirou
- **compute**: Add fenced service restart submission by @jackspirou
- **compute**: Orchestrate fenced workload restart by @jackspirou
- **compute**: Fence restart command dispatch by @jackspirou
- **compute**: Add fenced restart admission reducer by @jackspirou
- **workloads**: Add fenced restart command store by @jackspirou
- **workloads**: Add fenced restart state checkpoint by @jackspirou
- **compute**: Cut over atomic workload provision by @jackspirou
- **compute**: Add pure provision decision protocol by @jackspirou
- **workloads**: Persist strict executable intent by @jackspirou
- **compute**: Add durable workload saga ingress by @jackspirou
- **network**: Persist compiled workload network plan by @jackspirou
- **network**: Compile admitted workload network plans by @jackspirou
- **network**: Add durable workload recovery decisions by @jackspirou
- **network**: Add durable workload saga store by @jackspirou
- **network**: Complete NNC6.1c1 identity cutover by @jackspirou
- **workloads**: Add portable workload saga contract by @jackspirou
- **compute**: Coordinate node workloads by @jackspirou
- **compute**: Inject shared network manager by @jackspirou
- **sandbox**: Make inspection side-effect-free by @jackspirou
- **network**: Complete machine-forwarded readiness by @jackspirou
- **sandbox**: Complete host-managed attachment readiness by @jackspirou
- **network**: Reconcile OCI startup quarantine by @jackspirou
- **network**: Classify OCI orphan evidence by @jackspirou
- **network**: Enumerate durable orphan evidence by @jackspirou
- **network**: Order durable attachment authority before effects by @jackspirou
- **network**: Persist attachment lifecycle state by @jackspirou
- **sandbox**: Unify OCI attachment lifecycle by @jackspirou
- **network**: Compose standalone kv authority by @jackspirou
- **network**: Compose local-node authority by @jackspirou
- **network**: Compose one OCI process authority by @jackspirou
- **network**: Stage local manager composition by @jackspirou
- **network**: Own local network composition by @jackspirou
- **network**: Compose exact egress readiness by @jackspirou
- **network**: Model capability satisfaction by @jackspirou
- **network**: Prove restart reconciliation by @jackspirou
- **network**: Close bind authority census by @jackspirou
- **network**: Complete NNC3.7a CLI port migration by @jackspirou
- **network**: Complete NNC3.7 machine listener migration by @jackspirou
- **network**: Complete NNC3.6 KV listener migration by @jackspirou
- **network**: Complete NNC3.5 server listener migration by @jackspirou
- **network**: Complete NNC3.4 sandbox port migration by @jackspirou
- **network**: Record provider bind evidence by @jackspirou
- **network**: Model portable port conflicts by @jackspirou
- **network**: Add atomic port lease lifecycle by @jackspirou
- **network**: Retain cleanup authority after lease expiry by @jackspirou
- **network**: Quarantine segment cleanup before reuse by @jackspirou
- **network**: Fence segment allocation by typed epoch by @jackspirou
- **network**: Make block placement atomic by @jackspirou
- **network**: Inject portable segment allocator by @jackspirou
- **network**: Add crash-safe local state authority by @jackspirou
- **network**: Define fenced resource state model by @jackspirou
- **network**: Add stable control-plane identities by @jackspirou
- **network**: Add low-dependency control-plane crate by @jackspirou
- **ppsc**: Complete deterministic reliability closeout by @jackspirou in [#235](https://github.com/nimbus/nimbus/pull/235)
- Enable ordered publisher for provider tenants by @jackspirou in [#234](https://github.com/nimbus/nimbus/pull/234)

### Build

- **examples**: Enforce fresh verification preflight by @jackspirou
- Update Nimbus Deno fork to 2.9.3 by @jackspirou in [#205](https://github.com/nimbus/nimbus/pull/205)

### CI/CD

- Preserve hosted mirror attributes by @jackspirou
- Harden Ubuntu apt mirror setup by @jackspirou
- **docs**: Preserve successful previews when comments fail by @jackspirou
- Cache failed Windows inventory builds by @jackspirou
- Register privileged namespace regression by @jackspirou
- **bun-jsc-adapter**: Provision + cache WebKit source (FR-WK) by @jackspirou in [#170](https://github.com/nimbus/nimbus/pull/170)
- **bun-jsc-adapter**: Repoint workflow_dispatch defaults to 20260708 proof by @jackspirou in [#166](https://github.com/nimbus/nimbus/pull/166)

### Changed

- **verification**: Archive completed network plan by @jackspirou
- **verifier**: Extract restart contract fixture by @jackspirou
- **compute**: Make restart progress phase-explicit by @jackspirou
- **sandbox**: Separate network authority roots by @jackspirou
- **network**: Retire legacy port authority by @jackspirou
- **network**: Split segment allocation from realization by @jackspirou
- **network**: Own published endpoint vocabulary by @jackspirou

### Documentation

- **plan**: Record desktop release evidence
- **plan**: Close provisional workload-host matrix
- **plan**: Record provisional storage recovery
- **plan**: Record provisional adapter smoke
- **plan**: Record provisional product smoke
- **plan**: Record release audit blocker
- **plan**: Start release readiness campaign
- **nkv**: Define tenant kv durability boundary by @jackspirou
- **plan**: Close corrected SA6 scope by @jackspirou
- **plan**: Close SA10 and accept SA6 by @jackspirou
- **plan**: Close SA7 and start SA10 by @jackspirou
- **plan**: Close SA9 and start SA7 by @jackspirou
- **plan**: Close SA5 and start SA9 by @jackspirou
- Close SA2 and start SA5 by @jackspirou
- Advance metadata retention to qualification by @jackspirou
- **storage**: Close incremental materialized verification by @jackspirou in [#315](https://github.com/nimbus/nimbus/pull/315)
- **plans**: Close SA1 and start SA3 by @jackspirou
- **plans**: Close SA8 and start SA1 by @jackspirou
- **plans**: Record M9 decision in SA13 and normalize Band SA status cells by @jackspirou
- **plans**: Record storage adversarial review as Band SA by @jackspirou
- **plans**: Propose blob lifecycle integrity by @jackspirou
- **plans**: Propose incremental materialized verification by @jackspirou
- **research**: Record the PPSC5-D/U8 performance closeout by @jackspirou
- Tighten AGENTS.md and recover its verification hazards by @jackspirou
- **storage**: Reconcile governing specs with the landed integrity contracts by @jackspirou
- Archive application verification plan by @jackspirou
- **examples**: Align verification guidance by @jackspirou
- **plans**: Close AVR2 and start AVR3 by @jackspirou
- **plans**: Record AVR2 hosted acceptance by @jackspirou
- **plans**: Checkpoint AVR2 preview hardening by @jackspirou
- **plans**: Checkpoint AVR2 hosted correction by @jackspirou
- Publish network control-plane architecture by @jackspirou
- **network**: Record focused behavior evidence by @jackspirou
- **network**: Publish landed control-plane architecture by @jackspirou
- **network**: Record post-NNC6.5g main reconciliation by @jackspirou
- **network**: Record current-main reconciliation by @jackspirou
- **network**: Freeze NNC6.5d4 forwarded teardown contract by @jackspirou
- **network**: Freeze NNC6.5d3 detach release contract by @jackspirou
- **network**: Checkpoint NNC6.5d2 teardown audit by @jackspirou
- **network**: Complete NNC6.5d teardown audit by @jackspirou
- **plans**: Freeze NNC6.5b teardown driver by @jackspirou
- **plans**: Activate NNC6.5b teardown driver by @jackspirou
- **network**: Checkpoint NNC6.5 completion by @jackspirou
- **network**: Freeze workload teardown choreography by @jackspirou
- **network**: Checkpoint NNC6.4a completion by @jackspirou
- **plans**: Checkpoint NNC6.4a service restart by @jackspirou
- **plans**: Checkpoint restart verifier extraction by @jackspirou
- **plans**: Checkpoint restart provider orchestration by @jackspirou
- **plans**: Checkpoint restart command fencing by @jackspirou
- **plans**: Checkpoint NNC6.4a restart admission by @jackspirou
- **plans**: Checkpoint NNC6.4a restart store by @jackspirou
- **plans**: Start NNC6.4a restart adapters by @jackspirou
- **plans**: Start NNC6.4a portable restart state by @jackspirou
- **plans**: Freeze NNC6.4a restart substitution audit by @jackspirou
- **plans**: Record NNC6.4 preparation checkpoint by @jackspirou
- **network**: Freeze provision choreography cutover by @jackspirou
- **plan**: Freeze NNC6.1e1 durable ingress boundary by @jackspirou
- **plan**: Route network work to NNC6.1e1 by @jackspirou
- **plan**: Freeze NNC6.2a durable carrier contract by @jackspirou
- **plan**: Advance network work to NNC6.2a by @jackspirou
- **plan**: Refine NNC6.2 source allowlist by @jackspirou
- **plan**: Checkpoint NNC6.2 audit by @jackspirou
- **plan**: Freeze NNC6.2 compiler contract by @jackspirou
- **plan**: Advance network work to NNC6.2 by @jackspirou
- **plan**: Split NNC6.1e recovery decisions by @jackspirou
- **plan**: Close NNC6.1d and start NNC6.1e by @jackspirou
- **network**: Freeze operational identity cutover by @jackspirou
- **network**: Freeze workload saga contract by @jackspirou
- **network**: Record capability seam substitution audit by @jackspirou
- **network**: Reconcile cluster allocation seam by @jackspirou
- **plans**: Start NNC0.1 network baseline by @jackspirou
- **plans**: Complete NNC0.0 durability checkpoint by @jackspirou
- **plans**: Establish nimbus network control plane by @jackspirou
- Make developer guides strict-mode conformant by @jackspirou
- Revise developer guides for clarity by @jackspirou
- CLAUDE.md storage feature-gated-tests gotcha (owner-approved); storage follow-ups plan (FU1-FU8) by @jackspirou
- SWT5 final acceptance — campaign PASS (ratio 1.836) by @jackspirou in [#247](https://github.com/nimbus/nimbus/pull/247)
- SWT0.2 freeze B_ref baseline with accepted quiet-host evidence by @jackspirou in [#243](https://github.com/nimbus/nimbus/pull/243)
- Archive completed PPSC plan by @jackspirou in [#237](https://github.com/nimbus/nimbus/pull/237)
- **website**: Correct sandbox/example/config accuracy from website review by @jackspirou in [#223](https://github.com/nimbus/nimbus/pull/223)
- **agents**: Delegate to Codex natively; record three false-green traps by @jackspirou
- **agents**: Name all three engine-owned commit paths by @jackspirou
- **architecture**: Name all three engine-owned commit paths by @jackspirou
- **plans**: Archive Deno 2.9.3 fork campaign by @jackspirou
- **agents**: Record the blocking-test-wait hang trap by @jackspirou
- **agents**: Liaison holds for long codex jobs, not main-loop polling by @jackspirou
- **research**: Fix stale batching claims in concurrent-write bench doc by @jackspirou
- **agents**: Codify the no-nesting review flow for codex delegation by @jackspirou
- **research**: Record PPSC2-C adaptive-batching bench results by @jackspirou
- **research**: Record PPSC0 post-instrumentation write baseline by @jackspirou
- **agents**: Codex delegation contract — never rely on idle-wake by @jackspirou
- **developers**: Restore the index adapter list and deepen the Adapters overview by @jackspirou in [#190](https://github.com/nimbus/nimbus/pull/190)
- **developers**: Add an Adapters overview page by @jackspirou in [#189](https://github.com/nimbus/nimbus/pull/189)
- **site**: Group protocol adapters under an "Adapters" sidebar category by @jackspirou in [#187](https://github.com/nimbus/nimbus/pull/187)
- **plans**: Promote PPSC plan to active — Convex north star, gates become verification by @jackspirou
- **plans**: PPSC5 gains a provider-arm trigger (DBaaS = Convex's network-persistence shape) by @jackspirou
- **plans**: Re-slice PPSC plan — full Convex contract wire-now, machinery measurement-gated by @jackspirou
- **plans**: Register parallel-prepare-serial-commit plan (proposed, Phase 6) by @jackspirou
- Full public-docs review — 7 new pages + accuracy fixes across 20 pages by @jackspirou in [#186](https://github.com/nimbus/nimbus/pull/186)
- **plans**: Archive runtime-guest-trust-global-hardening (complete) + drop README entry by @jackspirou
- **plans**: Promote runtime-guest-trust-global-hardening to active (implementation start) by @jackspirou
- **plans**: Elevate guest-trust-global plan per exemplar review (deno/workerd/convex) by @jackspirou
- **plans**: Revise guest-trust-global security plan per Codex verification by @jackspirou
- **plans**: Archive Deno 2.9.2 fork maintenance by @jackspirou
- **plans**: Archive examples-and-target-resolution ; register security plan by @jackspirou
- Gpt-5.6-sol taste 8.5 by owner override by @jackspirou
- Gpt-5.6-sol taste 7 -> 8, UI/UX-weighted by @jackspirou
- Settle gpt-5.6-sol taste at 7 with per-axis note by @jackspirou
- **plans**: Fold gpt-5.6-sol DX review into examples plan by @jackspirou
- Raise gpt-5.6-sol taste rating to 9 in model routing table by @jackspirou
- Stop-gate outage blocks once per 30-min window, not per edit state by @jackspirou
- **plans**: Examples plan — per-directory READMEs + full adapter coverage by @jackspirou
- Stop-gate skips docs-only turns and diagnoses engine outages by @jackspirou
- **plans**: Promote examples-and-target-resolution control plane by @jackspirou
- Stop-gate reviews run at low effort; autoreview loops keep high by @jackspirou
- Stop-gate fingerprint covers untracked file content by @jackspirou
- Stop-gate is now the git-triage wrapper, not the plugin gate by @jackspirou
- **plans**: Archive erasure-operator-wiring (complete) + drop README entry by @jackspirou
- **plans**: Route erasure-operator-wiring plan (RustFS-lane follow-ups) by @jackspirou
- **plans**: Archive rustfs-storage-hardening (complete) + drop README entry by @jackspirou
- **plans**: EC13 RustFS adoption row → done (dependency ruled out; copy/adapt + pattern only) by @jackspirou
- Default Codex model is gpt-5.6-sol at high effort by @jackspirou
- **plans**: Archive bun-fork-refresh (complete) + drop README entry by @jackspirou
- **plans**: Bun-fork-refresh BFR0 done, BFR1 local 1-7 green, BFR2 branch pushed by @jackspirou
- **plans**: Bun-fork-refresh spec — add UI fixture pins and stale oven-sh path defaults by @jackspirou
- **plans**: Promote bun-fork-refresh plan + spec (2026-07-08 upstream snapshot) by @jackspirou
- Docs, scripts: finish sandbox_spec.rs path repair (page body + SDK verifier) by @jackspirou
- Repair source-map citations after nimbus-compute extraction (CP3) by @jackspirou
- **plans**: Reserve libkrun_session as its own sandbox backend family by @jackspirou
- **plans**: Adopt multi-backend sandbox architecture in Phase 3 routing by @jackspirou
- **plans**: Mark CP3  done — campaign complete by @jackspirou
- **plans**: Mark TI7/TI8  done by @jackspirou
- **plans**: CP3 in_progress (PR open) by @jackspirou
- **plans**: TI7/TI8 in_progress by @jackspirou
- **plans**: Mark CP2  done by @jackspirou
- **plans**: CP2 in_progress by @jackspirou
- **plans**: Mark CP1  done by @jackspirou
- **plans**: Truth-up stale SR8 row (#145 merged) by @jackspirou
- **plans**: Mark TI6  done by @jackspirou
- **plans**: CP1 in_progress by @jackspirou
- **plans**: Mark DE9/DE10/DE12  done by @jackspirou
- **plans**: TI6 in_progress + TI7/TI8 engine follow-ups from the TI6 review by @jackspirou
- **plans**: Mark CO7/GR9  done by @jackspirou
- **plans**: Amend CP spec from adversarial verification (subscriptions.rs blocker, ComputeError not AppError-move, real deps) by @jackspirou
- **plans**: CP nimbus-compute extraction spec (CP1/CP2/CP3, staged) by @jackspirou
- **plans**: Mark DE1-DE4 , SR2/SR5  done by @jackspirou
- **plans**: Mark SR6  done by @jackspirou
- **plans**: Mark SR8  done by @jackspirou
- **plans**: Mark CO10/CO11/CO12  done — CO band complete by @jackspirou
- **plans**: Mark TI1/TI2  done by @jackspirou
- **plans**: Mark DS1/DS2/DS4/DS5 done (direct-to-main 7d60ca6c7) by @jackspirou
- Docs, scripts: ARCHITECTURE crate table + workload-identity ladder, feature-flag notes, BPD verifier repair (DS1, DS2, DS4, DS5) by @jackspirou
- **plans**: Mark CO2/SR3/SR4  done by @jackspirou
- **plans**: Mark CO1/GR4/GR6/TI3/TI4/TI5/DE11 , UI1/UI2/UI6  done by @jackspirou
- **plans**: Mark GR5/DE6/DE7/DE8  done by @jackspirou
- **plans**: Mark CO13/CO14/DE16  done by @jackspirou
- **plans**: Mark UI3/UI4/UI5/UI7 , SR1/CO6/DS3  done by @jackspirou
- **plans**: Mark GR7/GR8 , CO8/CO9/DE5/DE13 , CO3/CO4/CO5/DE14/DE15  done by @jackspirou
- **plans**: TI6 — stabilize the nimbus-engine timing-flake family (campaign evidence) by @jackspirou
- **plans**: Mark engine-lane CO1/GR4/GR6/TI3/TI4/TI5/DE11 in_progress with gate evidence by @jackspirou
- **plans**: Normalize CO13/CO14/DE16 to in_progress (done requires merge evidence) by @jackspirou
- **plans**: CO13/CO14/DE16 implemented in js-lane, PRs pending by @jackspirou
- **plans**: AD3 — SR7 keep enum dispatch (re-open conditions), SR8 delete provider trait post-SR1 by @jackspirou
- **plans**: DS5 — BPD verifier drift (pre-existing, surfaced by sweeps review) by @jackspirou
- **plans**: Six lane specs for review-plan bands + GR9 (BlobGc pins) by @jackspirou
- **plans**: SR7/SR8 blocked pending owner ADR decision by @jackspirou
- **plans**: As-built sections for all eight campaign specs; track enterprise-crate plan by @jackspirou
- **plans**: GR3 done — PR #134 merged 19ac348b9 by @jackspirou
- **plans**: GR2 done — PR #132 merged 2e30e88e0 by @jackspirou
- **plans**: GR1 done — PR #129 merged bcac953bb by @jackspirou
- **plans**: Service-identity to in_progress — SI0 landed via PR #126 by @jackspirou
- **plans**: Windows plan truth-up from 2026-07 portfolio audit by @jackspirou
- **plans**: Sync review proof HTML AD2 section with refined plan decision by @jackspirou
- **plans**: Correct DS3 (VolumeProvider exists), add windows sequencing notes, refine AD2 by @jackspirou
- **plans**: Force-track architecture-review-2026-07 plan body and review proof by @jackspirou
- **plans**: Register architecture-review-2026-07 control-plane plan by @jackspirou
- Update CHANGELOG.md for v0.1.45 by @github-actions[bot]

### Fixed

- **ui**: Recover stale desktop sessions
- **workloads**: Converge startup and teardown
- **sandbox**: Close live krun workload gaps
- **machine**: Trust bootc guest convergence
- **machine**: Align guest image and stop authority
- **object-storage**: Honor separate control roots
- **backup**: Support separate control roots
- **storage**: Harden backup shutdown behavior
- **examples**: Make browser tasks portable
- **server**: Align cors and origin policies
- **cloudflare**: Report exact final kv page
- **cloudflare**: Keep provider fallback exhaustive
- **cloudflare**: Support kv on default sqlite store
- **cli**: Render actionable command errors
- **vendor**: Clean rust warning regressions
- **vendor**: Make lazy static lifetime explicit
- **release**: Harden js dependencies and audit gates
- **storage**: Verify imported destination position
- **storage**: Recover SQLite restore writers
- **proof**: Make IMV mutations fail closed
- **storage**: Allow default SQLite restore metadata
- **review**: Close verification and readiness gaps
- **verification**: Harden IMV performance gate
- **verification**: Require retention evidence per backend
- **storage**: Make embedded PITR import atomic
- **storage**: Verify complete materialized state
- **blob**: Preserve transient integrity failures by @jackspirou in [#327](https://github.com/nimbus/nimbus/pull/327)
- **server**: Expose consistency verification outcomes by @jackspirou in [#326](https://github.com/nimbus/nimbus/pull/326)
- Fix backup archive version diagnostics by @jackspirou in [#325](https://github.com/nimbus/nimbus/pull/325)
- **storage**: Reject unsupported durable format versions by @jackspirou in [#324](https://github.com/nimbus/nimbus/pull/324)
- **s3**: Stabilize Convex object manifest identity by @jackspirou in [#322](https://github.com/nimbus/nimbus/pull/322)
- **storage**: Avoid retention retry busy loop by @jackspirou
- **engine**: Fence local persistence roots per process by @jackspirou in [#312](https://github.com/nimbus/nimbus/pull/312)
- **s3**: Enforce wire integrity contracts by @jackspirou in [#311](https://github.com/nimbus/nimbus/pull/311)
- **storage**: Distinguish scalar negative zero digests by @jackspirou in [#310](https://github.com/nimbus/nimbus/pull/310)
- **storage**: Canonicalize materialized positions by @jackspirou in [#303](https://github.com/nimbus/nimbus/pull/303)
- **sandbox**: Name the unfinished exit receipt instead of failing on it by @jackspirou in [#299](https://github.com/nimbus/nimbus/pull/299)
- **engine**: Fold a materialized load to the sequence its caller requires by @jackspirou in [#301](https://github.com/nimbus/nimbus/pull/301)
- Clear the diagnostics that rustc 1.98 added by @jackspirou in [#300](https://github.com/nimbus/nimbus/pull/300)
- **sandbox**: Wait for a receipt's value, not for its path by @jackspirou in [#298](https://github.com/nimbus/nimbus/pull/298)
- Unblock the Coverage lane at both of its causes by @jackspirou in [#296](https://github.com/nimbus/nimbus/pull/296)
- **engine**: Keep the materialized serving surface consistent under load races by @jackspirou in [#293](https://github.com/nimbus/nimbus/pull/293)
- **system**: Publish projection retry state, and stop reading eventual state as instantaneous by @jackspirou in [#295](https://github.com/nimbus/nimbus/pull/295)
- **s3**: Fence multipart metadata writes on the revision the writer observed by @jackspirou in [#284](https://github.com/nimbus/nimbus/pull/284)
- **s3**: Decide object write conditions inside the commit authority by @jackspirou in [#281](https://github.com/nimbus/nimbus/pull/281)
- **ui**: Align shell gate with route support rename by @jackspirou
- **examples**: Bind correction evidence exactly by @jackspirou
- **examples**: Close verification evidence gaps by @jackspirou
- **examples**: Close hosted verification races by @jackspirou
- **examples**: Scrub verification credentials on exit by @jackspirou
- **examples**: Harden verification cleanup and credentials by @jackspirou
- **cli**: Separate local discovery from admin auth by @jackspirou
- **docs**: Normalize private-fence link targets by @jackspirou
- **cli**: Separate portable machine publication reads by @jackspirou
- **cli**: Isolate Unix machine publication effects by @jackspirou
- **cli**: Restore non-Unix machine seam parity by @jackspirou
- **sandbox**: Close Windows platform lint inventory by @jackspirou
- **network**: Resolve remaining PR CI failures by @jackspirou
- **network**: Close PR CI regressions by @jackspirou
- **network**: Retain overlapping verifier mutations by @jackspirou
- **network**: Close repository gate defects by @jackspirou
- **compute**: Bound restart store retries by @jackspirou
- **kv**: Bound command metric cardinality by @jackspirou
- Bind Convex socket auth to silo by @jackspirou
- **cloud-functions**: Bind deployments to trusted tenants by @jackspirou
- **convex**: Scope auth verifiers to silos by @jackspirou
- **engine**: Classify publisher persistence outcomes before retry by @jackspirou in [#220](https://github.com/nimbus/nimbus/pull/220)
- **engine**: Close lost-wakeup deadlock in the mutation journal worker by @jackspirou in [#184](https://github.com/nimbus/nimbus/pull/184)

### Miscellaneous

- Unbreak clippy under rust 1.97.0 (useless_borrows_in_formatting) by @jackspirou in [#172](https://github.com/nimbus/nimbus/pull/172)

### Performance

- **network**: Parallelize control-plane verifier mutations by @jackspirou

### Testing

- **ui**: Prove reconnect recovery lifecycle
- **ui**: Refresh release smoke contracts
- **engine**: Scope libsql ack-loss fault
- **engine**: Classify PITR journal flush outcome
- **retention**: Mutate real verifier conditions
- **engine**: Await embedded lock release on reload
- **server**: Prebind Mongo spec listener
- **engine**: Align PITR fault proof with atomic import
- **blob**: Pin NBLE1 erasure parity bytes by @jackspirou in [#323](https://github.com/nimbus/nimbus/pull/323)
- **engine**: Allow process fence artifact in replica cache by @jackspirou in [#316](https://github.com/nimbus/nimbus/pull/316)
- **storage**: Bound retention concurrency wait by @jackspirou
- **storage**: Baseline metadata retention contract by @jackspirou in [#313](https://github.com/nimbus/nimbus/pull/313)
- Satisfy IMV4 archive taxonomy by @jackspirou in [#307](https://github.com/nimbus/nimbus/pull/307)
- **engine**: Close the drain race in the queued-cancellation rollback test by @jackspirou in [#294](https://github.com/nimbus/nimbus/pull/294)
- **ports**: Claim host port windows instead of probing and releasing them by @jackspirou in [#288](https://github.com/nimbus/nimbus/pull/288)
- **sandbox**: Accept either live-attempt observation when adopting a krun claim by @jackspirou in [#292](https://github.com/nimbus/nimbus/pull/292)
- **storage**: Prove SQLite physical durability under injected faults by @jackspirou in [#290](https://github.com/nimbus/nimbus/pull/290)
- **storage**: Make every storage writer declare its commit effects by @jackspirou in [#285](https://github.com/nimbus/nimbus/pull/285)
- Isolate Rust authorities and restore CI by @jackspirou
- **examples**: Add bounded verification scheduler by @jackspirou
- **examples**: Emit structured verification evidence by @jackspirou
- **examples**: Own verification resource lifetimes by @jackspirou
- **examples**: Isolate verification workspaces by @jackspirou
- **server**: Target listener projection fault by @jackspirou
- **docs**: Isolate private-fence mutation by @jackspirou
- **network**: Preserve archived-plan mutation fixtures by @jackspirou
- **network**: Prove offline sovereign lifecycle by @jackspirou
- **network**: Close static authority verifier by @jackspirou
- **compute**: Exercise restart callback fences by @jackspirou
- **network**: Unify persisted phase crash harness by @jackspirou
- **network**: Verify protocol listener parity by @jackspirou
- **compute**: Record NNC6.5b fail-before by @jackspirou
- **workloads**: Freeze NNC6.4a R2 contracts by @jackspirou
- **network**: Freeze NNC6.4a restart contract by @jackspirou
- **network**: Freeze NNC6.4 dispatch contract by @jackspirou
- **network**: Freeze NNC6.1e1 ingress fail-before by @jackspirou
- **network**: Capture workload identity cutover red by @jackspirou
- **network**: Add local sovereignty tripwire by @jackspirou
- **network**: Close production authority census by @jackspirou
- **network**: Harden multi-tenant invariant proofs by @jackspirou
- **network**: Deepen source contract verifier by @jackspirou
- **network**: Capture lifecycle and allocation baselines by @jackspirou
- **network**: Add expected-red control-plane verifier by @jackspirou
- **network**: Capture NNC0.7 orphan and listener gaps by @jackspirou
- **network**: Capture NNC0.6a inspect restart races by @jackspirou
- **network**: Capture NNC0.6 withdrawal readiness gaps by @jackspirou
- **network**: Capture NNC0.5 allocation gaps by @jackspirou
- **network**: Capture NNC0.4 state corruption by @jackspirou
- **network**: Capture NNC0.3 cleanup reuse race by @jackspirou
- **network**: Capture NNC0.2 port allocation races by @jackspirou
- **network**: Add exact-boundary crash-cut harness by @jackspirou
- **network**: Add deterministic process contention harness by @jackspirou
- **network**: Capture NNC0.1 architecture baseline by @jackspirou
- **engine**: Shut down the trigger worker after the fixture seed by @jackspirou in [#211](https://github.com/nimbus/nimbus/pull/211)
- **core**: Property-test conflict-predicate interval edges by @jackspirou in [#198](https://github.com/nimbus/nimbus/pull/198)
- **engine**: Prove provider-lane ops-per-round-trip scaling for adaptive batching by @jackspirou in [#197](https://github.com/nimbus/nimbus/pull/197)
- **convex**: Make seeded-demo timeouts env-tunable, bump defaults 3s->10s by @jackspirou in [#169](https://github.com/nimbus/nimbus/pull/169)
- Testing, system, node: EngineFixture adoption + shared control-plane fixtures (TI1, TI2) by @jackspirou in [#147](https://github.com/nimbus/nimbus/pull/147)

### Adapters/sdk/docs

- PPSC3-B — taxonomy vocabulary, typed SDK errors, write contract, host-write posture by @jackspirou in [#200](https://github.com/nimbus/nimbus/pull/200)

### Bench

- **storage**: Decide incremental verification gate by @jackspirou in [#304](https://github.com/nimbus/nimbus/pull/304)
- **engine**: Establish IMV fail-before baseline by @jackspirou in [#302](https://github.com/nimbus/nimbus/pull/302)
- Open-loop service-latency mode + minicloud evidence (SUC6.1) by @jackspirou in [#256](https://github.com/nimbus/nimbus/pull/256)
- SWT4.1 forward-apply attribution; implementation rejected (D17) by @jackspirou in [#246](https://github.com/nimbus/nimbus/pull/246)
- **engine**: Concurrent write-throughput (group-commit) benchmark + proof by @jackspirou in [#185](https://github.com/nimbus/nimbus/pull/185)

### Blob

- Fix Phase C bench measurement validity (post-merge review) by @jackspirou in [#177](https://github.com/nimbus/nimbus/pull/177)
- LocalPackStore vs ErasureBlobStore throughput bench (RFS7 Phase C) by @jackspirou in [#175](https://github.com/nimbus/nimbus/pull/175)
- Erasure heal, shard GC, and stats — RFS7 Phase B by @jackspirou in [#174](https://github.com/nimbus/nimbus/pull/174)
- ErasureBlobStore multi-drive erasure leg — RFS7 Phase A by @jackspirou in [#173](https://github.com/nimbus/nimbus/pull/173)
- Local pack scrubber, quarantine, and index rebuild (RFS5) by @jackspirou in [#167](https://github.com/nimbus/nimbus/pull/167)
- RustFS-informed local pack durability + root ownership (RFS2, RFS3) by @jackspirou in [#164](https://github.com/nimbus/nimbus/pull/164)

### Bridge

- Consolidate host-call dispatch shims into one dispatch.rs (SR6) by @jackspirou in [#148](https://github.com/nimbus/nimbus/pull/148)

### Bun/jsc

- Repin adapter to proof 20260709 (embedder deny thread-local) by @jackspirou in [#176](https://github.com/nimbus/nimbus/pull/176)
- Refresh fork to nimbus/bun-main-20260708 + repin adapter by @jackspirou in [#165](https://github.com/nimbus/nimbus/pull/165)

### Cli

- Fix backup/restore encryption-domain defect — ciphertext end-to-end by @jackspirou in [#180](https://github.com/nimbus/nimbus/pull/180)

### Compute

- Migrate handler orchestration into compute-owned functions (CP3) by @jackspirou in [#163](https://github.com/nimbus/nimbus/pull/163)
- Split AppState into axum-free ComputeState + server transport wrapper (CP2) by @jackspirou in [#161](https://github.com/nimbus/nimbus/pull/161)
- Extract transport-free compute plane into nimbus-compute (CP1) by @jackspirou in [#157](https://github.com/nimbus/nimbus/pull/157)

### Core/bridge/testing

- PPSC3-C — closed commit-error class, shared conformance harness, invocation-kind capability seam by @jackspirou in [#201](https://github.com/nimbus/nimbus/pull/201)

### Deps

- Hold crossbeam-epoch at 0.9.18; ban 0.9.20 (aborts nimbus-server tests on macOS) by @jackspirou in [#160](https://github.com/nimbus/nimbus/pull/160)
- Resolve RUSTSEC-2026-0204/0205 (crossbeam-epoch 0.9.20, serial_test 3.5.0 drops scc) by @jackspirou in [#158](https://github.com/nimbus/nimbus/pull/158)

### Dx

- Give the CLI one owner for authoring-root precedence by @jackspirou in [#291](https://github.com/nimbus/nimbus/pull/291)
- Make package wiring reversible — `packages install`/`uninstall` by @jackspirou in [#286](https://github.com/nimbus/nimbus/pull/286)
- Correct agent quickstart install, status vocabulary, and SDK error decoding by @jackspirou in [#279](https://github.com/nimbus/nimbus/pull/279)

### Dynamodb

- 400 KiB item ceiling on every write path over AWS-accurate sizing; 4 MB transaction aggregate (FU13, FU14) by @jackspirou in [#271](https://github.com/nimbus/nimbus/pull/271)
- Enforce BatchWriteItem's duplicate-item and 400 KiB rules by @jackspirou in [#269](https://github.com/nimbus/nimbus/pull/269)
- Validate the whole BatchWriteItem request before applying it by @jackspirou in [#267](https://github.com/nimbus/nimbus/pull/267)
- Transactional batch prior images; policy-aware scan and stream reads (FU3/FU4) by @jackspirou in [#265](https://github.com/nimbus/nimbus/pull/265)
- Execute requests as the calling access key, not as system by @jackspirou in [#255](https://github.com/nimbus/nimbus/pull/255)
- Pin UpdateItem atomicity with a lost-write regression test (SUC4.1) by @jackspirou in [#251](https://github.com/nimbus/nimbus/pull/251)

### Egress

- Pin CONNECT gate deferral + forced interception with regression tests (SUC4.3) by @jackspirou in [#252](https://github.com/nimbus/nimbus/pull/252)

### Engine

- Rebuild verification after typed retention expiry by @jackspirou
- Skip the durable write for an unchanged table schema by @jackspirou in [#268](https://github.com/nimbus/nimbus/pull/268)
- Fix four mutation_journal flakes (FU9) by @jackspirou in [#266](https://github.com/nimbus/nimbus/pull/266)
- Stop trigger-candidate restarts from flaking journal assertions by @jackspirou in [#262](https://github.com/nimbus/nimbus/pull/262)
- SUC2 — single commit-sequence transcription + fenced object writes by @jackspirou in [#250](https://github.com/nimbus/nimbus/pull/250)
- Make tenant admission topology-safe by @jackspirou in [#233](https://github.com/nimbus/nimbus/pull/233)
- Make committer arm selection immutable by @jackspirou in [#226](https://github.com/nimbus/nimbus/pull/226)
- Fence system projections across providers by @jackspirou in [#224](https://github.com/nimbus/nimbus/pull/224)
- PPSC5-C.1 — one durable-outcome classifier for every write route by @jackspirou in [#219](https://github.com/nimbus/nimbus/pull/219)
- Evict definitive fenced committers by @jackspirou in [#218](https://github.com/nimbus/nimbus/pull/218)
- PPSC5-C — fence every provider durable write path by @jackspirou in [#217](https://github.com/nimbus/nimbus/pull/217)
- Acquire and renew the provider committer lease (PPSC5-C unit 3) by @jackspirou in [#216](https://github.com/nimbus/nimbus/pull/216)
- Align the window's current-view reads with its image coverage by @jackspirou in [#213](https://github.com/nimbus/nimbus/pull/213)
- PPSC5-B — crash-recovery decision table by @jackspirou in [#209](https://github.com/nimbus/nimbus/pull/209)
- Make mutation-journal test assertions deterministic by @jackspirou in [#210](https://github.com/nimbus/nimbus/pull/210)
- Remove observer test and flush races by @jackspirou in [#208](https://github.com/nimbus/nimbus/pull/208)
- PPSC5-A — ordered embedded mutation publisher by @jackspirou in [#206](https://github.com/nimbus/nimbus/pull/206)
- PPSC4-B — off-gate prepare pool with inline window re-prepare by @jackspirou in [#203](https://github.com/nimbus/nimbus/pull/203)
- PPSC4-A — committer-as-actor with bounded inbox, structural serial invariants, first loom models by @jackspirou in [#202](https://github.com/nimbus/nimbus/pull/202)
- PPSC2-C — adaptive journal batching, hermitage serializability matrix, Elle history recorder by @jackspirou in [#196](https://github.com/nimbus/nimbus/pull/196)
- PPSC2-A — in-memory full-image conflict window by @jackspirou in [#194](https://github.com/nimbus/nimbus/pull/194)
- Bound the shadow-conflict observation window by @jackspirou in [#192](https://github.com/nimbus/nimbus/pull/192)
- PPSC0 instrumentation — PreparedCommit, phase metrics, shadow conflicts, provider gate-hold lane by @jackspirou in [#191](https://github.com/nimbus/nimbus/pull/191)
- Zero-write commit correctness across materialized reads, coalescing, and subscription bootstrap (TI7, TI8) by @jackspirou in [#162](https://github.com/nimbus/nimbus/pull/162)
- Stabilize materialized-serving & subscription timing tests via settle barriers (TI6) by @jackspirou in [#156](https://github.com/nimbus/nimbus/pull/156)
- Shared worker/queue/pause seams, at-least-once triggers, ladder cleanup, honest unwrap (CO1, GR4, GR6, TI3, TI4, TI5, DE11) by @jackspirou in [#142](https://github.com/nimbus/nimbus/pull/142)

### Engine/bridge/server

- PPSC1 — server-side bounded conflict retry by @jackspirou in [#193](https://github.com/nimbus/nimbus/pull/193)

### Engine/core

- PPSC3-A — commit error taxonomy, shadow mutation caps, tenant write-rate shadowing by @jackspirou in [#199](https://github.com/nimbus/nimbus/pull/199)
- PPSC2-B — schema epochs, collection-group dependencies, assign-time stamping by @jackspirou in [#195](https://github.com/nimbus/nimbus/pull/195)

### Engine/server

- PPSC4-C — loom handoff matrix, tenant mutation-isolate ceiling, warm-pool concurrency guard by @jackspirou in [#204](https://github.com/nimbus/nimbus/pull/204)

### Engine/storage

- PPSC0 foundation testability seams (S0a/S0b/S0c) by @jackspirou in [#188](https://github.com/nimbus/nimbus/pull/188)

### Firebase

- Firestore typed values in array transforms; type-safe query contract (SUC4.2) by @jackspirou in [#253](https://github.com/nimbus/nimbus/pull/253)
- Invoke buf + protoc-gen-es through node (Windows codegen portability) by @jackspirou in [#146](https://github.com/nimbus/nimbus/pull/146)

### Fs

- Platform-split as_stdio return type (fixes windows-check red on main) by @jackspirou in [#127](https://github.com/nimbus/nimbus/pull/127)

### Js

- Convex client mixins, SDK control-plane split, buf-generated protobuf (CO13, CO14, DE16) by @jackspirou in [#139](https://github.com/nimbus/nimbus/pull/139)

### Network

- Canonicalize forwarded workload composition by @jackspirou
- Complete NNC6.5f caller substitution audit by @jackspirou
- Complete NNC6.5e native source retirement cutover by @jackspirou
- Converge machine-forwarded publication batches by @jackspirou
- Converge partial attachment outcomes by @jackspirou

### Nimbus-ui

- Typed mutation client, storage-table decomposition, one data-loading contract (UI1, UI2, UI6) by @jackspirou in [#143](https://github.com/nimbus/nimbus/pull/143)
- Hygiene wave 1 — shared Loading/Empty, Slideover, doc types, machines split (UI3, UI4, UI5, UI7) by @jackspirou in [#137](https://github.com/nimbus/nimbus/pull/137)

### Object-storage

- External S3 target suite + migration classification (RFS8) by @jackspirou in [#171](https://github.com/nimbus/nimbus/pull/171)

### Plan

- Record final storage review verdict
- Complete storage review repairs
- SRR4 done; SRR5 in_progress
- SRR3 done; SRR4 in_progress
- SRR2 done; SRR3 in_progress
- SRR1 done; SRR2 in_progress
- SRR0 done; SRR1 in_progress
- Archive metadata retention; start SA2 by @jackspirou
- Record safe metadata retention closeout by @jackspirou
- Close SMR3 and start SMR4 by @jackspirou
- Archive incremental materialized verification by @jackspirou in [#318](https://github.com/nimbus/nimbus/pull/318)
- Close SMR2 and start SMR3 by @jackspirou
- Close SMR1 and start SMR2 by @jackspirou
- Close SMR0 and start SMR1 by @jackspirou
- Promote storage metadata retention by @jackspirou
- SA3 done ; SA4 in_progress by @jackspirou
- Start SA8 negative-zero correction by @jackspirou
- IMV0 in_progress by @jackspirou
- Correct the SIC7 make ci counts to the closeout run by @jackspirou
- SIC9 archives the storage integrity contracts campaign by @jackspirou
- SIC7 done, governing specs reconciled by @jackspirou
- SIC7 in progress, verifier fully green by @jackspirou
- SIC6 merged by @jackspirou
- SIC5 merged by @jackspirou
- SIC6 done, physical durability faults proved by @jackspirou
- Next action is SIC6 by @jackspirou
- SIC4 merge and SIC5 execution log by @jackspirou
- SIC4 merged, SIC5 done by @jackspirou
- SIC4 done by @jackspirou
- SIC4 in_progress by @jackspirou
- SIC3 merged by @jackspirou
- Record the hosted clippy toolchain skew on SIC3 by @jackspirou
- SIC3 done by @jackspirou
- SIC3 in_progress by @jackspirou
- Record the SIC2 merge by @jackspirou
- Record CI mirror remediation by @jackspirou
- Record AVR12 cleanup pull request by @jackspirou
- Freeze AVR12 cleanup candidate by @jackspirou
- Complete AVR11 and start AVR12 by @jackspirou
- SIC0 done, SIC1 in_progress by @jackspirou
- Activate storage integrity contracts; SIC0 in_progress by @jackspirou
- Record AVR11 hosted-green checkpoint by @jackspirou
- Record AVR11 hosted UI correction by @jackspirou
- Record AVR11 PR 3 opened by @jackspirou
- Freeze AVR11 candidate for PR 3 by @jackspirou
- AVR10 done; AVR11 in_progress by @jackspirou
- Complete AVR7 and start AVR8 by @jackspirou
- Record AVR7 hosted-green checkpoint by @jackspirou
- Record AVR7 hosted security correction by @jackspirou
- Track implementation PR 2 by @jackspirou
- Close AVR7 review gate by @jackspirou
- Record AVR7 review corrections by @jackspirou
- AVR7 candidate frozen for phase review by @jackspirou
- AVR6 done; AVR7 in_progress by @jackspirou
- AVR5 done; AVR6 in_progress by @jackspirou
- AVR4 done; AVR5 in_progress by @jackspirou
- AVR3 done; AVR4 in_progress by @jackspirou
- Route hosted docs retry hardening by @jackspirou
- Record AVR2 pull request by @jackspirou
- Record AVR2 phase candidate by @jackspirou
- Complete AVR1 and start AVR2 by @jackspirou
- Complete AVR0 and start AVR1 by @jackspirou
- AVR0 in_progress by @jackspirou
- Activate docs and app verification reliability by @jackspirou
- Close network control-plane execution by @jackspirou
- Close NNC8.6 failure contracts by @jackspirou
- Record NNC7.3 checkpoint by @jackspirou
- Start NNC7.1 protocol parity audit by @jackspirou
- NNC6.5g done; NNC6.6 in_progress by @jackspirou
- NNC6.4 done; NNC6.4a in_progress by @jackspirou
- NNC6.3b done; NNC6.4 in_progress by @jackspirou
- Clean up developer docs writing plan by @jackspirou in [#273](https://github.com/nimbus/nimbus/pull/273)
- Record strict update on PR #272 by @jackspirou in [#272](https://github.com/nimbus/nimbus/pull/272)
- DTW3 done; DTW9 waits for PR #272 by @jackspirou
- Record developer docs pull request by @jackspirou
- Complete developer docs review gates by @jackspirou
- FU13/FU14 complete ; only the live-probe ticket remains by @jackspirou
- Archive storage-follow-ups (FU sweep complete, PRs #262-#270) by @jackspirou
- FU12 complete ; FU13/FU14 ticketed by @jackspirou
- FU10  + FU11  complete; FU12 batch rejection rules ticketed by @jackspirou
- Restore FU9 row (complete, PR #266) lost in the fu3 rebase resolution by @jackspirou
- FU3+FU4 complete ; FU11 batch-validation divergence ticketed by @jackspirou
- FU1 complete by @jackspirou
- FU5  + FU6  complete; FU10 projection-reconciliation flake ticketed by @jackspirou
- FU7 complete — SUC6.2 rejection measured on current main (U10; override resolved) by @jackspirou
- SUC3.1 complete (PRs #254, #257-#261); campaign COMPLETE; U9 LoC verdict recorded by @jackspirou
- SUC3.1 step 5 merged ; step 6 test dedupe in_progress by @jackspirou
- SUC3.1 step 4 merged ; MySQL has_scheduled_work outlier ticketed by @jackspirou
- SUC3.1 step 3 merged ; U8 recorded; arm-theft ticket rescoped to fault-interface change by @jackspirou
- SUC3.1 step 2 merged ; step 3 scope gains arm-theft fault gating by @jackspirou
- SUC6.1 complete ; SUC6.2 closed by U7 gate amendment (owner may override) by @jackspirou
- SUC5.1 complete ; PPSC arm-theft + scan-paging follow-up tickets by @jackspirou
- SUC3.1 step 1 merged ; SUC6.3 closed as U6 (moot-by-SWT2); SUC5.1 in_progress by @jackspirou
- SUC4.2 complete ; SUC4 closed; follow-up tickets recorded by @jackspirou
- SUC4.1+SUC4.3 complete (PRs #251, #252; pre-fixed by #231) by @jackspirou
- SUC2 complete ; U5 recorded; SUC4 in_progress by @jackspirou
- SUC0+SUC1 complete ; SUC2.1 in_progress by @jackspirou
- SUC0.1 complete ; SUC0.2+SUC1.1 in_progress by @jackspirou
- Storage unification and carry-over closeout control plane by @jackspirou
- Archive completed SQLite write-throughput campaign (PASS) by @jackspirou
- SWT4 disposed ; SWT5 in_progress by @jackspirou
- SWT2 complete ; SWT3 rejected per gate (D16); SWT4.1 next by @jackspirou
- Mark SWT1 complete ; SWT2 in_progress by @jackspirou
- Mark SWT0 complete; promote SWT1 to in_progress by @jackspirou
- Mark SWT0.1 merged; unblock SWT0.2 B_ref freeze by @jackspirou
- Mark CTRL0 complete, promote SWT0 to in_progress by @jackspirou

### Plans

- README entry for storage-follow-ups plan by @jackspirou
- Archive storage-unification-and-carryover (complete, PRs #248-#261); remove README entry by @jackspirou

### Proof

- **sic**: SIC0 red verifier, writer census, and fail-before evidence by @jackspirou

### Proxy

- Durable-before-response egress decision logging, fail closed (GR3) by @jackspirou in [#134](https://github.com/nimbus/nimbus/pull/134)

### Runtime

- Isolate retained state by tenant owner by @jackspirou in [#227](https://github.com/nimbus/nimbus/pull/227)
- Close the guest-reassignable __nimbus* trust-global isolation class by @jackspirou in [#183](https://github.com/nimbus/nimbus/pull/183)
- Update Nimbus to Deno 2.9.2 fork by @jackspirou in [#182](https://github.com/nimbus/nimbus/pull/182)
- Egress posture is a named constructor argument (GR2) by @jackspirou in [#132](https://github.com/nimbus/nimbus/pull/132)
- Platform-split chmod mode type for deno's FileSystem trait by @jackspirou

### Sandbox

- Seal network effect ownership by @jackspirou

### Scripts

- Reconcile SDK resource-model verifier anchors after CP1/CP3, SDK control-plane split, and plan archival by @jackspirou

### Server

- De-flake authz denial test via document-commit counting by @jackspirou in [#159](https://github.com/nimbus/nimbus/pull/159)
- Operator-authz core, ServiceManager verb seam, HTTP-adapter mount seam (CO2, SR3, SR4) by @jackspirou in [#144](https://github.com/nimbus/nimbus/pull/144)
- Consolidation sweep — pagination, lifecycle verbs, callable statuses, fixtures, misc (CO3, CO4, CO5, DE14, DE15) by @jackspirou in [#138](https://github.com/nimbus/nimbus/pull/138)

### Storage

- Qualify metadata retention bounds by @jackspirou
- Bound libsql retention read round trips by @jackspirou
- Publish floors for version compaction by @jackspirou
- Fail closed across retained history reads by @jackspirou
- Add fenced provider retention compaction by @jackspirou
- Add applied materialized verification deltas by @jackspirou in [#306](https://github.com/nimbus/nimbus/pull/306)
- Qualify every provider against seven semantic dimensions by @jackspirou in [#289](https://github.com/nimbus/nimbus/pull/289)
- Bind materialized artifacts to one canonical position by @jackspirou in [#287](https://github.com/nimbus/nimbus/pull/287)
- Count disabled cron jobs as MySQL scheduled work (FU1) by @jackspirou in [#264](https://github.com/nimbus/nimbus/pull/264)
- Invert the nimbus-fs manifest seam; relocate libsql replica-cache fns (FU6) by @jackspirou in [#263](https://github.com/nimbus/nimbus/pull/263)
- Dedupe provider test suites behind shared scenarios by @jackspirou in [#261](https://github.com/nimbus/nimbus/pull/261)
- Gate remote providers behind opt-in features (SUC3.1 step 5) by @jackspirou in [#260](https://github.com/nimbus/nimbus/pull/260)
- Dedupe postgres/mysql transaction-half twins (SUC3.1 step 4/5) by @jackspirou in [#259](https://github.com/nimbus/nimbus/pull/259)
- Commit-effect witness + fault-point ownership gate (SUC3.1 step 3/5) by @jackspirou in [#258](https://github.com/nimbus/nimbus/pull/258)
- Libsql joins the shared SQL store core (SUC3.1 step 2/5) by @jackspirou in [#257](https://github.com/nimbus/nimbus/pull/257)
- Shared SQL store core — postgres+mysql wrapper layers (SUC3.1 step 1/5) by @jackspirou in [#254](https://github.com/nimbus/nimbus/pull/254)
- Provider lease parity (SUC1.1) with fail-before inventory (SUC0.2) by @jackspirou in [#249](https://github.com/nimbus/nimbus/pull/249)
- SWT2 resident SQLite writer connection by @jackspirou in [#245](https://github.com/nimbus/nimbus/pull/245)
- SWT1 prepared statements and batch-invariant apply context by @jackspirou in [#244](https://github.com/nimbus/nimbus/pull/244)
- SWT0.1 SQLite write observability counters and WAL/checkpoint seam by @jackspirou in [#242](https://github.com/nimbus/nimbus/pull/242)
- Pipeline provider journal writes by @jackspirou in [#232](https://github.com/nimbus/nimbus/pull/232)
- Fenced durable append-and-apply (PPSC5-C unit 2, behaviour-neutral) by @jackspirou in [#215](https://github.com/nimbus/nimbus/pull/215)
- Provider committer lease (PPSC5-C unit 1, behaviour-neutral) by @jackspirou in [#214](https://github.com/nimbus/nimbus/pull/214)
- Require a contiguous applied prefix, and classify pre-image divergence as corruption by @jackspirou in [#212](https://github.com/nimbus/nimbus/pull/212)
- PPSC5-B — duplicate-write defense on every backend by @jackspirou in [#207](https://github.com/nimbus/nimbus/pull/207)
- One transactional write core for the redb TenantStore (GR1) by @jackspirou in [#129](https://github.com/nimbus/nimbus/pull/129)

### Storage,testing

- Durable-record identity and replay discrimination in the PPSC fault interface (FU2) by @jackspirou in [#270](https://github.com/nimbus/nimbus/pull/270)

### System

- Fix torn projection stats snapshot in flush test; SUC0 CI triage by @jackspirou in [#248](https://github.com/nimbus/nimbus/pull/248)

### Ui

- First-principles design review of the operator console, and the fixes by @jackspirou in [#283](https://github.com/nimbus/nimbus/pull/283)

### Verification

- Add AVR0 baseline contracts by @jackspirou

### Workload-identity

- SI3 — short-lived EdDSA JWT minting (LocalDevIssuer) by @jackspirou in [#131](https://github.com/nimbus/nimbus/pull/131)
- Enforce the identity grant at the mint seam (SI1) by @jackspirou in [#128](https://github.com/nimbus/nimbus/pull/128)
- Add SI0 issuance seam crate by @jackspirou in [#126](https://github.com/nimbus/nimbus/pull/126)



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.45...v0.1.46

## [0.1.45] - 2026-07-06

### Added

- **kme**: KME5 — bypass-hardening as construction-time invariants by @jackspirou in [#64](https://github.com/nimbus/nimbus/pull/64)
- **kme**: KME4 — fail-closed execute readiness gate replaces the unconditional krun fail-close by @jackspirou in [#63](https://github.com/nimbus/nimbus/pull/63)
- **kme**: KME3 — route the krun guest through the host egress PEP via injected HTTP_PROXY by @jackspirou in [#62](https://github.com/nimbus/nimbus/pull/62)
- **mongodb**: Operator per-tenant credential ingestion — serve bound on a network address (M9a config) by @jackspirou in [#32](https://github.com/nimbus/nimbus/pull/32)
- Add nimbus kv observability probes by @jackspirou
- Add nimbus kv conformance harness by @jackspirou
- Add nimbus kv cache tiering by @jackspirou
- Add tenant kv storage primitive by @jackspirou
- Add nimbus kv resp listener by @jackspirou
- **nfs**: Implement Nimbus isolate filesystem by @jackspirou in [#31](https://github.com/nimbus/nimbus/pull/31)
- **mongodb**: Operator per-tenant credential ingestion — serve bound on a network address (M9a config) by @jackspirou
- **machine**: Vfkit backend + MachineProvider seam; keep Nimbus-pinned gvproxy (bump 0.8.9) by @jackspirou in [#18](https://github.com/nimbus/nimbus/pull/18)

### BRH

- Real bounded blob range reads, NimbusFS hardening, lazy object opens by @jackspirou in [#97](https://github.com/nimbus/nimbus/pull/97)

### Build

- **deps**: Bump anyhow 1.0.102 -> 1.0.103 (RUSTSEC-2026-0190) by @jackspirou in [#67](https://github.com/nimbus/nimbus/pull/67)

### CB0

- Connection-broker 14-condition progression verifier by @jackspirou in [#102](https://github.com/nimbus/nimbus/pull/102)

### CB1

- Connection-broker substrate — residency, host-owned registry, placement seam by @jackspirou in [#104](https://github.com/nimbus/nimbus/pull/104)

### CB10

- Connection metering — Active-CPU + residency usage records by @jackspirou in [#107](https://github.com/nimbus/nimbus/pull/107)

### CB2

- Per-frame invoke verb over a safe owner-keyed warm pool by @jackspirou in [#105](https://github.com/nimbus/nimbus/pull/105)

### CB3+CB5+CB7

- Complete the connection broker to 14/14 by @jackspirou in [#109](https://github.com/nimbus/nimbus/pull/109)

### CB4

- Inbound WS-upgrade ingress — the default-ALLOW layer by @jackspirou in [#106](https://github.com/nimbus/nimbus/pull/106)

### CB9

- Resident app-WS server + ws/socket.io zero-config compat by @jackspirou in [#108](https://github.com/nimbus/nimbus/pull/108)

### Changed

- **sandbox**: Extract shared EgressProxyRegistry seam (KME prerequisite) by @jackspirou in [#59](https://github.com/nimbus/nimbus/pull/59)
- Salvage filesystem tests and DO lease hardening by @jackspirou

### Documentation

- **plans**: Mark test-infra-rearchitecture complete by @jackspirou
- **plans**: Promote test-infra-rearchitecture as in_progress owner by @jackspirou
- **plans**: Archive-resolvers + README archive-pointers (completes the prior commit) by @jackspirou
- **plans**: Archive completed plans (K11P/wasmtime/connection-broker); retire dead DUA verifier by @jackspirou
- **plans**: Mark K11P / wasmtime / connection-broker complete in the README by @jackspirou
- ARCHITECTURE.md — name the two JS adapter shapes (compat-over-SDK vs independent-wire-protocol) by @jackspirou
- Fix substring-replace corruption in source-map template citation by @jackspirou
- Source-map — repair all 52 stale citations from the nimbus-bin -> nimbus-cli move by @jackspirou
- Source-map — update citations for the nimbus-bin -> nimbus-cli module move by @jackspirou
- ARCHITECTURE.md — add the egress/network-trust plane, refresh runtime truth by @jackspirou
- Retire completed nimbus-egress-engine plan from active roadmap by @jackspirou
- Retire nimbus-fs-capability-wiring plan from active roadmap by @jackspirou
- Consolidate plans README + trim AGENTS.md routing; retire nimbus-kv plan by @jackspirou
- **egress**: State the M14 readiness-seam decision (keep, production producer pending) by @jackspirou in [#78](https://github.com/nimbus/nimbus/pull/78)
- **kme**: TSI+netns is the ratified egress path, not passt by @jackspirou in [#57](https://github.com/nimbus/nimbus/pull/57)
- Salvage archived plan verifier cleanup by @jackspirou
- Document runtime lane diagnostics by @jackspirou
- Refresh source-map ownership paths by @jackspirou
- Update CHANGELOG.md for v0.1.44 by @github-actions[bot]

### EE

- Node-scoped EgressEngine — per-workload PEP lifecycle, allow-ceiling, fairness, fan-out by @jackspirou in [#96](https://github.com/nimbus/nimbus/pull/96)

### FCW

- FsCaps grant-resolved construction, get_range byte-plane reads, resolver hardening by @jackspirou in [#95](https://github.com/nimbus/nimbus/pull/95)

### Fixed

- **nfs**: Bound CAS-RO partial reads and drop ambient default cwd by @jackspirou in [#93](https://github.com/nimbus/nimbus/pull/93)
- **cloud-functions**: Route handler fetch through the per-tenant egress PDP (M13) by @jackspirou in [#76](https://github.com/nimbus/nimbus/pull/76)
- **egress**: Tighten PDP path/IPv6 matching, test validators, document contracts by @jackspirou in [#72](https://github.com/nimbus/nimbus/pull/72)
- **egress-proxy**: Audit denies, normalize Host, map dial failures, pin rebind (PR-8) by @jackspirou in [#74](https://github.com/nimbus/nimbus/pull/74)
- **egress**: Fail closed on the isolate substrate for proxy-enforced rules (H4) by @jackspirou in [#71](https://github.com/nimbus/nimbus/pull/71)
- **egress-proxy**: Close HTTP framing bypasses (Transfer-Encoding, bare CR/LF, numeric-IP CONNECT) by @jackspirou in [#70](https://github.com/nimbus/nimbus/pull/70)
- **kme**: Verifier measures the ratified TSI+netns design, not passt by @jackspirou in [#58](https://github.com/nimbus/nimbus/pull/58)
- **convex**: Complete #41 — team-binding admission (silo+principal→team), replace #43 stopgap, migrate suite by @jackspirou in [#53](https://github.com/nimbus/nimbus/pull/53)
- **convex**: Fail-closed stopgap — refuse the convex application surface on a non-loopback bind by @jackspirou in [#43](https://github.com/nimbus/nimbus/pull/43)
- **firebase**: Bind Firestore tenant to the verified project via a registry by @jackspirou in [#38](https://github.com/nimbus/nimbus/pull/38)
- **runtime**: Preserve RuntimeMetrics Arc identity across RouterOptions::build() (observability) by @jackspirou in [#35](https://github.com/nimbus/nimbus/pull/35)
- **mongodb**: Bind SCRAM credentials to TenantId — authentication decides the tenant (M9a, #23 part a) by @jackspirou in [#29](https://github.com/nimbus/nimbus/pull/29)
- **mongodb**: Fail closed at startup on non-loopback bind without tenant-bound credentials by @jackspirou in [#28](https://github.com/nimbus/nimbus/pull/28)
- **mongodb**: Fail closed at startup on non-loopback bind without tenant-bound credentials by @jackspirou
- **runtime**: Resolve concurrent cross-profile isolate-creation crash + wire WebStandard web APIs by @jackspirou in [#20](https://github.com/nimbus/nimbus/pull/20)
- **release**: Use symbol form for depends_on macos in generated cask by @jackspirou in [#17](https://github.com/nimbus/nimbus/pull/17)

### K11P

- Pingora egress substrate + reopen-wave hardening by @jackspirou in [#94](https://github.com/nimbus/nimbus/pull/94)

### KME2

- Give the krun VMM the container netns + deny-route chain by @jackspirou in [#65](https://github.com/nimbus/nimbus/pull/65)

### MTN4

- Crash-safe tenant-bridge reaper + manifest persistence + legacy nimbus0 purge by @jackspirou in [#82](https://github.com/nimbus/nimbus/pull/82)

### MTN5

- Host-side inter-tenant isolation + DNS-off + cross-tenant KVM deny-proof by @jackspirou in [#83](https://github.com/nimbus/nimbus/pull/83)

### MTN6

- Fix grown-block IPAM shared-cursor bug + prove grow-path isolation (KVM) by @jackspirou in [#91](https://github.com/nimbus/nimbus/pull/91)
- Remaining-segment dimension on NodeCapacity (fail-closed placement) by @jackspirou in [#86](https://github.com/nimbus/nimbus/pull/86)
- Wire the startup orphan-GC live into both backends (reclaimed metric) by @jackspirou in [#85](https://github.com/nimbus/nimbus/pull/85)
- Startup orphan-GC reconciliation primitive for the segment allocator by @jackspirou in [#84](https://github.com/nimbus/nimbus/pull/84)

### MTN7

- Cluster segment allocator seam (lease-gated, behind the SAME trait) by @jackspirou in [#90](https://github.com/nimbus/nimbus/pull/90)

### Miscellaneous

- Sync Cargo.lock for nimbus-services proxy/storage deps (CB3/CB5) by @jackspirou
- Sync Cargo.lock for Seam E deps; restore verifier exec bits by @jackspirou
- Verify nkv cloudflare branch ci by @jackspirou
- Scaffold nimbus kv foundation gate by @jackspirou

### SELH

- Harden sandbox egress launch gates by @jackspirou in [#92](https://github.com/nimbus/nimbus/pull/92)

### Testing

- Canonical nimbus/rusty_v8 asset consumption (owner-directed) by @jackspirou in [#124](https://github.com/nimbus/nimbus/pull/124)
- **B10b**: Cargo-hakari workspace-hack (measured batch; CI archive job is the oracle) by @jackspirou in [#123](https://github.com/nimbus/nimbus/pull/123)
- **B8w4**: Mutation-gap fixes — all 17 B9 survivors killed, gate PASSED by @jackspirou in [#122](https://github.com/nimbus/nimbus/pull/122)
- **B10a**: Parallelize the PR wall poles — node-faas matrix, bin decouple, ptrcomp split by @jackspirou in [#121](https://github.com/nimbus/nimbus/pull/121)
- **B8w2**: Tenant-isolation + blob-crypto test uplift by @jackspirou in [#120](https://github.com/nimbus/nimbus/pull/120)
- **B8w1**: Storage-atomicity + mutation-path test uplift by @jackspirou in [#119](https://github.com/nimbus/nimbus/pull/119)
- **B7**: Nightly tier, flake enforcement, scaling probe, frontier reporting by @jackspirou in [#118](https://github.com/nimbus/nimbus/pull/118)
- **B5**: Archive-consumer shards, harness + provider lanes (G1/A2/A8) by @jackspirou in [#117](https://github.com/nimbus/nimbus/pull/117)
- **B4**: Archive job, slim profile, doctest lane, disk calibration by @jackspirou in [#116](https://github.com/nimbus/nimbus/pull/116)
- **B3b**: Fail-closed prebuilt V8 consumption (A14) by @jackspirou in [#115](https://github.com/nimbus/nimbus/pull/115)
- **B3a**: V8 variant build matrix; trusted-runner manifest composition by @jackspirou in [#114](https://github.com/nimbus/nimbus/pull/114)
- **B3a**: Once-per-pin rusty_v8 prebuild publish workflow by @jackspirou in [#113](https://github.com/nimbus/nimbus/pull/113)
- **B2**: Authoritative inventory, validator gating, case matrix by @jackspirou in [#112](https://github.com/nimbus/nimbus/pull/112)
- **B1**: Declarative nextest taxonomy contract by @jackspirou in [#111](https://github.com/nimbus/nimbus/pull/111)
- **kme**: Rename krun no-path netns test off the verifier's strict negative by @jackspirou in [#66](https://github.com/nimbus/nimbus/pull/66)
- Add websocket egress canaries by @jackspirou

### Deny

- Reconcile skip list with the deno v2.9.1 repin by @jackspirou in [#103](https://github.com/nimbus/nimbus/pull/103)

### Harden

- **krun**: Drop CAP_NET_ADMIN and tighten egress-gate evidence (PR-9) by @jackspirou in [#73](https://github.com/nimbus/nimbus/pull/73)

### Honesty

- **egress**: Delete dead-claims, convert aspirational seams to plan-linked stubs (PR-5) by @jackspirou in [#77](https://github.com/nimbus/nimbus/pull/77)

### Item5

- Narrow nimbus-server composition root into concept-owned config modules by @jackspirou in [#110](https://github.com/nimbus/nimbus/pull/110)

### Runtime

- Repin Deno fork to v2.9.1-nimbus.1 by @jackspirou in [#99](https://github.com/nimbus/nimbus/pull/99)
- Close REC nested-call matrix by @jackspirou

### Sandbox

- Pin each sandbox's netns egress to its own PEP (audit H1) by @jackspirou in [#79](https://github.com/nimbus/nimbus/pull/79)



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.44...v0.1.45

## [0.1.44] - 2026-06-20

### Documentation

- Update CHANGELOG.md for v0.1.42 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.43...v0.1.44

## [0.1.43] - 2026-06-20



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.42...v0.1.43

## [0.1.42] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.41 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.41...v0.1.42

## [0.1.41] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.40 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.40...v0.1.41

## [0.1.40] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.39 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.39...v0.1.40

## [0.1.39] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.38 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.38...v0.1.39

## [0.1.38] - 2026-06-19



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.37...v0.1.38

## [0.1.37] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.36 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.36...v0.1.37

## [0.1.36] - 2026-06-19



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.35...v0.1.36

## [0.1.35] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.34 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.34...v0.1.35

## [0.1.34] - 2026-06-19

### Added

- **website**: Theme-aware favicon with brand sky tile by @jackspirou
- **cli**: DXW3 — start serves all adapters by default, store-backed (D7) by @jackspirou
- **dev**: Firebase migration hint on no-adapter; shorter landing tab comments by @jackspirou
- **provision**: Auto-wire app package.json deps; landing shows migration commands by @jackspirou
- **firebase**: Drop-in firebase package — stock imports work unchanged by @jackspirou
- **cli**: LR12 — nimbus node run, the reconciler's production caller by @jackspirou
- **packaging**: LR10 — ship a hardened systemd unit in deb/rpm by @jackspirou
- **cli,engine**: LR9 — nimbus backup create/restore on SEQ8 archives by @jackspirou
- **server,bin**: LR8 — in-server TLS termination for the main listener by @jackspirou
- **sdk**: LR7 — rest.ts matches the server, with a 3-sided parity guard by @jackspirou
- **cli**: LR6 — nimbus start enables Firestore, MongoDB, and DynamoDB by @jackspirou
- **server,bin**: LR5 — configurable CORS origins by @jackspirou
- **bin**: LR4 — public-bind rotation gate: explicit-once, age advisory by @jackspirou
- **sdk**: LR3 — remove the dead X-Nimbus-Api-Key credential path by @jackspirou
- **cli**: LR2 — nimbus deploy passes the AdminHeaderOnly gate by @jackspirou
- **ndb7**: Default systemd-dbus + linux factory + operator doc by @jackspirou
- **ndb6**: Node-dbus-integration CI lane on ubuntu-24.04 by @jackspirou
- **ndb5**: Linux-gated live systemd integration tests by @jackspirou
- **ndb4**: Zbus error taxonomy + nimbus_core Transport/NotFound by @jackspirou
- **ndb3**: Signal-correlated completion + property encoder by @jackspirou
- **ndb2**: ZbusSystemdClient skeleton + capability probe by @jackspirou
- **ndb1**: Wire zbus_systemd + zbus behind systemd-dbus feature by @jackspirou

### Changed

- **cli**: Render wire surfaces from one presentation list + adapter status in start summary by @jackspirou
- **cli**: Dev-loop hygiene — adoption outcomes as data, cached covered set by @jackspirou
- **server**: Add WireProtocolAdapter seam for sibling listeners by @jackspirou
- **server**: Make Firestore REST auth structural via route-layer middleware by @jackspirou
- **cli**: Decompose dev.rs — tests to dev/tests/, firebase wiring to dev/firebase.rs by @jackspirou
- **repo**: Docs/private goes fully untracked; pipeline inputs move by @jackspirou
- **bin**: LR1 — finish Service->Engine naming in start/ by @jackspirou

### D5.5

- ListStreams + read-triggered retention (T5 Streams complete) by @jackspirou

### D6.1

- UpdateTimeToLive + DescribeTimeToLive (T6 begins) by @jackspirou

### D6.2

- TTL sweeper integration by @jackspirou

### D6.3

- Tagging surface (T6 complete; full T0-T6 op surface) by @jackspirou

### D7.3

- Nimbus-native persisted access-key management (T7 complete) by @jackspirou

### D8.7

- Five DynamoDB verification-harness cases (PR + nightly lanes) by @jackspirou

### D9.1

- Feature-parity coverage table (T0-T7) by @jackspirou

### D9.3

- Failure-injection + fail-closed proof by @jackspirou

### D9.4

- Tenant + auth isolation proof by @jackspirou

### D9.5

- Mixed-workload soak test by @jackspirou

### D9.6

- Performance benchmark baseline (p50/p95/p99 for every op family) by @jackspirou

### D9.7

- Enterprise-readiness closeout; verifier green (23 passed, 0 failed) by @jackspirou

### Documentation

- Correctness + consistency pass across all six groups by @jackspirou
- **website**: Tighten landing hero to "your cloud, one binary" by @jackspirou
- Retire dead docs/private links from package READMEs by @jackspirou
- DXD1 — flip docs to autodetect + default-on adapter reality by @jackspirou
- **site**: Editorial pass — nimbus dev-led landing, de-self-praised voice, Diátaxis heading fixes by @jackspirou
- **site**: Agents group, value-ladder landing, deploy tutorial, title hygiene by @jackspirou
- Favicon follows the page theme; unique page titles; Firebase tab by @jackspirou
- **site**: Brand glyphs in the landing adapter tabs by @jackspirou
- **site**: Landing tabs named by surface, all six proof snippets by @jackspirou
- **agents**: LR13 — launch-readiness baseline archived by @jackspirou
- **plans**: LR0 — launch-readiness verifier + proof bundle by @jackspirou
- **plans**: Launch-readiness plan — close the 13-item docs-truth gap list by @jackspirou
- **private**: DOC13 closeout — nimbus-docs-site plan done + archived by @jackspirou
- **private**: DOC13 staging retirement sweep + editorial fix by @jackspirou
- **agents**: DOC12 — .agents/skills migration + docs skill + AGENTS.md routing by @jackspirou
- **repo**: DOC11 — README front door refactor + repo metadata by @jackspirou
- **site**: DOC8 — llms-small.txt corpus tuning + scripts/check-docs.sh honesty gate by @jackspirou
- **site**: DOC7 — public architecture pages + ARCHITECTURE.md rewrite by @jackspirou
- **site**: DOC6 — Concepts core + CLI/configuration/SDK/capabilities reference by @jackspirou
- **site**: DOC5 — Operators group, tenancy concepts, server reference by @jackspirou
- **site**: DOC4 — Developers and adapter Reference corpus by @jackspirou
- DOC3 — restructure docs/, five-group IA, landing, get-started by @jackspirou
- DOC9 CI pipeline + DOC10 custom domain — nimbusdocs.com live by @jackspirou
- Tighten verifier condition 3 against comment false-positive by @jackspirou
- DOC2 design harmonization — theme tokens + DESIGN.md docs surface by @jackspirou
- DOC0 verifier + DOC1 Starlight scaffold by @jackspirou
- Archive completed dynamodb-adapter-plan by @jackspirou
- Point NDB routing at archived plan path by @jackspirou
- Close remaining NDB review items (minor + verifier rigor) by @jackspirou
- Harden NDB plan after pre-execution review by @jackspirou

### Fixed

- **engine**: Close lost-wakeup race in applied-visibility wait by @jackspirou
- **cli**: Close artifact-order race in convex adoption test by @jackspirou
- **bin**: Machine API client deadlocked on Connection: close responses by @jackspirou
- **ci,server**: Ring-backed rustls + make-wrapped LR12 lane by @jackspirou
- **ci**: Finish the docs/private relocation sweep + D-Bus lane UI deps by @jackspirou
- **repo**: Restore docs/private/* gitignore pattern + recover orphans by @jackspirou
- **release**: LR11 — apt channel live + release->distribution dispatch by @jackspirou
- Pool floor 8, pinned rustls provider, Waker::noop — three CI reds by @jackspirou
- **storage**: Bounded wait before sqlite read-pool exhaustion by @jackspirou
- **bin**: Retry one-shot machine-API test requests on accept races by @jackspirou
- **runtime**: Convert warm-pool partition test to invocation-kind reuse by @jackspirou
- **nds**: Release-train proof gate paths + regenerated artifacts by @jackspirou
- **runtime,ci**: Restore node22 grant contracts + finish stale-path sweep by @jackspirou
- **ci**: Repair the two remaining red-main causes beyond the path hotfix by @jackspirou
- Repair stale docs/private/staging/architecture paths in crates + NDS scripts by @jackspirou
- Remediate full code review findings by @jackspirou
- **ndb3**: Idempotent Manager.Subscribe (AlreadySubscribed) by @jackspirou

### Miscellaneous

- Point dev-autodetect verifier at the archived plan path by @jackspirou
- Baseline service backend refactor by @jackspirou
- Baseline workspace before service backend refactor by @jackspirou

### Styling

- Rustfmt the NDB systemd D-Bus binding by @jackspirou

### Design

- Nav lockup spacing, ink-cropped transparent marks, favicon tile by @jackspirou
- Unify the sky-cycle default theme across console, docs, and brand by @jackspirou

### Dev

- DXL2 — mid-session app-adapter adoption through the boot-time flow by @jackspirou
- DXL1 — live manifest re-detection with presentation-only adoption by @jackspirou
- DXW2 — shared persisted wire credentials + Nimbus-owned .env.local keys by @jackspirou
- D7 — start serves all adapters by default; reshape verifier condition 3 by @jackspirou
- D6 — always-available wire listeners; reshape verifier condition 10 by @jackspirou
- DXW1 — wire-surface detection reads runtime dependencies only by @jackspirou

### Dev-autodetect

- DXF5 — client-app loop semantics by @jackspirou
- DXF4 — projectId→tenant mapping with live round-trip by @jackspirou
- DXF1-DXF3 — scan-gated FirestoreClient detection + wiring by @jackspirou
- DXA2 — always-on Firestore routes in dev by @jackspirou
- DXA1 — app-adapter/wire-surface model split by @jackspirou
- DXA0 — completion-gate verifier scaffold by @jackspirou

### Hardening

- **H7**: Evidence rigor + doc accuracy; plan complete by @jackspirou
- **H6**: Query skips non-scalar/absent index keys instead of aborting by @jackspirou
- **H5**: Reserved-tenant guard + redacted access-key listing by @jackspirou
- **H4**: DeleteTable reclaims stream/streamseq/ttl/tag sidecars by @jackspirou
- **H3**: Atomic stream capture for batch/transact + atomic sequencing by @jackspirou
- **H2**: Atomic single-item + catalog writes, close conditional TOCTOU by @jackspirou
- **H1**: Bind SigV4 body, harden auth robustness, strict-by-default by @jackspirou
- Scaffold verifier + promote plan to in_progress by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.33...v0.1.34

## [0.1.33] - 2026-05-26

### Documentation

- Update CHANGELOG.md for v0.1.33 by @github-actions[bot]
- Update CHANGELOG.md for v0.1.32 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.32...v0.1.33

## [0.1.32] - 2026-05-26

### A1+A2

- Codegen types every query result; lift ServiceDoc; drop casts by @jackspirou

### A3

- Decompose admin/settings.tsx into concept-owned children by @jackspirou

### A4

- Router-level loaders for five data routes by @jackspirou

### A5

- Component catalog — 11 stories under Storybook by @jackspirou

### A6

- CI browser-smoke harness via playwright-cli by @jackspirou

### A7

- Decompose observability route into tab-body siblings by @jackspirou

### AP

- Wire artifact provenance enforcement paths by @jackspirou

### AP0

- Use maintained OCI reference parser for image admission by @jackspirou

### AP1

- Add artifact verifier adapter contract by @jackspirou

### AP2

- Add cosign verifier backend by @jackspirou

### AP3

- Add SLSA verifier backend by @jackspirou

### AP4

- Gate executable artifact provenance by @jackspirou

### AP5

- Add SBOM evidence backend by @jackspirou

### AP6

- Support offline verifier roots by @jackspirou

### AP7

- Add artifact provenance conformance gate by @jackspirou

### Added

- Add node workload reconciler by @jackspirou
- Wire tenant lifecycle evidence by @jackspirou
- Add compose quadlet export by @jackspirou
- Add node service install surface by @jackspirou
- Add systemd transient backend seam by @jackspirou
- Add direct process backend by @jackspirou
- Add host lifecycle seam by @jackspirou
- Add local enforcement binding by @jackspirou

### Appearance

- Palette/mode switcher + brand-token canonicalization by @jackspirou

### BS

- Archive brand-system-plan by @jackspirou

### BS0-3+9

- Brand-system plan + canonical logo + 9 variants + DESIGN.md tier split by @jackspirou

### BS4-5

- Wire favicon + sidebar mark in nimbus-ui by @jackspirou

### Baseline

- Capture in-flight plan, system-tenant, machine, and verification work by @jackspirou

### CA0

- Scaffold Coverage Acceleration plan + verifier + baseline proof by @jackspirou

### CA1

- Install mold linker in setup-rust-cached composite by @jackspirou

### CA2

- Re-enable parallel coverage link under mold (-j 4) by @jackspirou

### CA3

- Shard Coverage into 3 lanes + cargo llvm-cov reducer by @jackspirou

### CA4

- Migrate release.yml to setup-rust-cached composite by @jackspirou

### CA5

- Closeout — archive plan, promote canonical contract, update routing by @jackspirou

### CC0

- Scaffold ci-caching-canonicalization plan + verifier + baseline proof by @jackspirou

### CC1

- Wire sccache into Coverage job (pilot before full rollout) by @jackspirou

### CC2

- Expand sccache across every Rust job + rotate Swatinem v1→v2 by @jackspirou

### CC3

- Rerun-safe Swatinem saves + main-branch save gate by @jackspirou

### CC4

- Ui-artifacts leader job + harness/coverage consumers by @jackspirou

### CC5

- Warm-sccache leader job + ci-caching contract doc by @jackspirou

### CC8

- Closeout — archive plan, update routing, mark gate complete by @jackspirou

### CC9

- Bump sccache-action v0.0.6→v0.0.10, retract save-always, audit stale pins by @jackspirou

### CD1-CD5

- Canonicalize nimbus start/dev/ui CLI surface by @jackspirou

### CD6+CD7

- Verify Electron CWD contract + land walk-up regression suite by @jackspirou

### CD7

- **j**: Cargo fmt on the new deploy-restart test by @jackspirou
- **j**: Pin deployed-app rehydrate contract across Service restart by @jackspirou

### CD8

- Documentation pass for CLI daemon canonicalization by @jackspirou

### CD9

- Tighten --ensure grep gate, capture smoke matrix in execution log by @jackspirou
- Close + archive CLI daemon canonicalization plan by @jackspirou

### CI/CD

- Unblock workspace + desktop-ui + cargo deny lanes by @jackspirou
- Generate nimbus-ui convex codegen before cargo workspace jobs by @jackspirou

### CM0

- Scaffold CI Modernization plan + verifier + baseline proof by @jackspirou

### CM1

- Extract setup-rust-cached composite action, migrate 12 sites by @jackspirou

### CM2

- SHA-pin every third-party action with version-name comment by @jackspirou

### CM3

- Pin ubuntu runners to ubuntu-24.04 by @jackspirou

### CM5

- Emit job summaries from 4 high-value CI jobs by @jackspirou

### CM8

- Closeout — archive plan, promote canonical contract, update routing by @jackspirou

### CW0

- Scaffold CI Wall Acceleration plan + verifier + baseline proof by @jackspirou

### CW1

- Shard verification-harness corpus across shards per surface by @jackspirou

### CW2

- Shard Rust Workspace Tests via nextest --partition by @jackspirou

### CW3

- Split External Provider Integration Tests by provider by @jackspirou

### CW4

- Drop --tests from warm-sccache + document deferred target-cache lane by @jackspirou

### CW5

- Closeout — archive plan, promote contract, update routing by @jackspirou

### Changed

- Extract nimbus node crate by @jackspirou
- Extract pure tenant crate by @jackspirou
- Move artifact verifier effects out of tenant by @jackspirou
- Audit tenant crate boundary by @jackspirou
- Rename tenant isolation module path by @jackspirou

### DA1

- Auth page logo + version chip + local-only trust line by @jackspirou

### DA10

- Agent auth contract + grep gate by @jackspirou

### DA12

- Post-audit cleanup — bind-gate split, rotate-admin polish by @jackspirou

### DA2

- Nimbus auth url command + login/status/logout scaffold by @jackspirou

### DA3

- Flip nimbus dev to auto-open + add --no-open opt-out by @jackspirou

### DA4

- Emit one-shot first-boot launch URL banner from nimbus start by @jackspirou

### DA5

- Auth page polish — lede, hint, error state, disclosure, brand accent by @jackspirou

### DA6

- Cross-CLI sign-in microcopy cleanup + grep gate by @jackspirou

### DA8

- Deploy auth — login/status/logout + credentials file by @jackspirou

### DA9

- Network-bind guardrails — --allow-network + rotation tripwire by @jackspirou

### DR1

- Copy hygiene + canonical EmptyState (F1) by @jackspirou

### DR2

- Gate ⌘\ system tenant lens to Developer view (F2) by @jackspirou

### DR3

- Section truth on Observability + Schedules (F3, F4) by @jackspirou

### DR4

- Auto-default activeTenant on /app + drop ScopeChip "all" fallback (F5, F12) by @jackspirou

### DR5

- Real shells on /admin index + /admin/observability (F10, F11) by @jackspirou

### DR6

- Prune admin service detail to Placement-only by @jackspirou

### DR7

- Polish breadcrumb, tab casing, sub-drawer grouping by @jackspirou

### DU1

- Embed UI assets at /ui/* with SPA fallback by @jackspirou

### DU10

- Testing pyramid, storybook, react compiler eval by @jackspirou

### DU11

- Hardening — disposable server fixture, rotate/shutdown E2E, perf lane by @jackspirou

### DU2

- Open operator console via nimbus ui (Chromium preferred) by @jackspirou

### DU3

- Scaffold + shell layout by @jackspirou

### DU4

- Overview tab by @jackspirou

### DU5

- Machines tab by @jackspirou

### DU6

- Services and functions tabs by @jackspirou

### DU6.5

- Function runner by @jackspirou

### DU7

- Second-pass audit — token values, focus restoration, destructive confirm by @jackspirou
- Ux/ui audit — state-token compliance, modal confirms, link reservation by @jackspirou
- Data browser, schema, indexes, tenants by @jackspirou

### DU8

- Logs and runs tabs by @jackspirou

### DU9

- Settings, integrations, deploys by @jackspirou

### Documentation

- Scaffold node dbus client binding plan by @jackspirou
- Clarify node lifecycle surfaces by @jackspirou
- Define local enforcement boundary by @jackspirou
- Align tenant module naming by @jackspirou
- Update CHANGELOG.md for v0.1.31 by @github-actions[bot]

### EPS0-EPS2

- Add operator policy spine by @jackspirou

### EPS3-EPS4a

- Add sandbox egress policy seam by @jackspirou

### EPS4b0

- Add egress enforcement contract by @jackspirou

### EPS4b1

- Add sandbox supervisor entrypoint by @jackspirou

### EPS4b2a

- Select supervisor egress launch contracts by @jackspirou

### EPS4b2b

- Fail closed krun egress launches by @jackspirou
- Add container egress smoke proof by @jackspirou
- Deny direct container egress intent by @jackspirou

### EPS4b3

- Prove live container egress reload by @jackspirou
- Wire container egress proxy by @jackspirou
- Add sandbox egress proxy core by @jackspirou

### EPS5

- Export tenant isolation audit events by @jackspirou

### EPS6

- Add external policy backend seam by @jackspirou

### EPS7

- Add denied egress policy drafts by @jackspirou

### EPS8

- Add policy prove advisories by @jackspirou

### EPS9

- Publish policy egress conformance by @jackspirou

### Fixed

- Catch up materialized serving snapshots by @jackspirou

### H1

- Ratify dual-persona Services into DESIGN.md (BLOCKER) by @jackspirou

### H2

- Type-safety pass — derive tab unions, extract TenantScope ADT by @jackspirou

### H3

- Offline + error envelopes — LoadingValue<T>, /admin/tenants 404 envelope, status-bar tenant canonicalization by @jackspirou

### H4

- Surface polish — casing, command palette scroll-fit, encryption dot, lens chevron, EmptyState mono, /admin/network default section by @jackspirou

### H5

- Cleanup + spec backfills — shared tenants fetch, abort guard, narrowing throws by @jackspirou

### I2

- Capture Ubuntu 24.04 fresh install proof; flip to done by @jackspirou

### I3

- Capture Debian 13 + Fedora 42 fresh dep install proofs; flip to done by @jackspirou

### I4

- Capture Apple Silicon macOS cask + machine running proof; flip to done by @jackspirou

### I5

- Wire install.sh into releases + hosted curl|sh end-to-end proof; flip to done by @jackspirou

### IS

- Archive install-script-plan; all 5 lanes done by @jackspirou

### LD1

- Makefile dependency graph for nimbus-ui artifacts by @jackspirou

### LD2

- Delete build.rs stub fallback, error actionably by @jackspirou

### LD3

- Ci.yml — delete inlined npm orchestration, route through make by @jackspirou

### LD4

- Build-contract docs + CLAUDE.md routing entry by @jackspirou

### LD6

- Add /goal control-plane verifier script by @jackspirou

### Miscellaneous

- Update tenant crate lockfile by @jackspirou
- Checkpoint current workspace baseline by @jackspirou

### PW0

- Scaffold ci-pr-wall-sub-15 plan + verifier + baseline proof by @jackspirou

### PW1

- Pin libsql image to v0.24.26 + add docker-image cache lane by @jackspirou

### PW2

- Extract Coverage track to .github/workflows/coverage.yml by @jackspirou

### PW3

- Flip ci.yml cancel-in-progress to branch-conditional by @jackspirou

### PW4c

- Retain warm-sccache with measurement rationale by @jackspirou

### PW5

- Repin libsql to v0.24.33 (v0.24.26 had Host-header routing bug) by @jackspirou
- Switch libsql fixture hosts to localhost for sqld v0.24.26 by @jackspirou

### PW6

- Closeout — promote contract, archive plan, update routing by @jackspirou

### R1

- Producer-side query wrapper — drop as-unknown-as casts by @jackspirou

### R10

- Smoke spec — deterministic fixture seeding by @jackspirou

### R11

- Polish — story state coverage + nit pass by @jackspirou

### R2

- Loaderize _.$service.tsx sibling queries by @jackspirou

### R3

- Codegen specs + audit-comment + JsonValue dedup + convention decision by @jackspirou

### R4

- Loaderize compute_.runs_.$runId.tsx by @jackspirou

### R5

- Loader-error envelope coverage on the four A4 service routes by @jackspirou

### R6

- Extract shared filter + table-cell primitives by @jackspirou

### R7

- LoaderDeps for tenant-switch invalidation by @jackspirou

### R8

- A3 residue cleanup — dead dialogRefs + typed settings sub-drawer by @jackspirou

### R9

- CSP test tolerates attribute-bearing tags + workflow paths widened by @jackspirou

### RAQ0

- Add repo architecture guardrail by @jackspirou

### RAQ1

- Split tenant isolation root by @jackspirou

### RAQ2

- Split system tenant evidence by @jackspirou

### RAQ3

- Canonicalize server construction by @jackspirou

### RAQ4

- Split runtime policy and local ops by @jackspirou

### RAQ5

- Split sandbox service manager lifecycle by @jackspirou

### RAQ6

- Split policy and adapter surfaces by @jackspirou

### RAQ7

- Split CLI workflow surfaces by @jackspirou

### RAQ8

- Split JS compatibility surfaces by @jackspirou

### RAQ9

- Add evidence taxonomy guardrails by @jackspirou

### README

- Document nimbus-desktop install path by @jackspirou

### Security

- Add CodeQL SAST workflow for Rust + JavaScript/TypeScript by @jackspirou
- Bump actions/create-github-app-token v3.2.0 -> v3 by @jackspirou
- Closure — archive Phase 1 + Phase 2 plans by @jackspirou

### Testing

- Add tenant node extraction verifier by @jackspirou

### UL1

- Server /api/system/version-info with stale-while-revalidate by @jackspirou

### UL2

- SPA staleness UX — status-bar slot, sonner toast, upgrade popover by @jackspirou

### UL3

- Capture full-flow screencast at .playwright-cli/ul3/full-flow.webm by @jackspirou

### UX1

- Fix lens path resolution to read /app|/admin segment + lift dev-only gate by @jackspirou

### UX2

- Launch ticket bootstrap and styled /ui/auth page by @jackspirou

### UX3

- Clear toast above status bar via shared --statusbar-height token by @jackspirou

### UX4

- Ship styled Select shell component, migrate observability filter by @jackspirou

### UX5

- Branch storage empty state on tenant existence by @jackspirou

### UX6

- Runtime diagnostics returns 200 with null fields when no app is active by @jackspirou

### UX7

- Ship SegmentedControl shell, migrate mode toggle and view switcher by @jackspirou

### UX8

- Tint light-mode page bg so cards read without relying on the border by @jackspirou

### UX9

- Shell-component grep gate, catalog + DESIGN.md sync, after/ proof captures by @jackspirou

### Auth

- Collapse copy chips + add `nimbus auth token` + `--open` URL flag by @jackspirou

### Auth-page

- Unify How to login copy + rename terminal-chrome label by @jackspirou
- Full-width shell-block recovery surface + How to login catalog by @jackspirou
- Rename label to Enter auth token + hero-scale standalone chip by @jackspirou
- Lift Auth Token chip above Other ways to login disclosure by @jackspirou
- Rename label to Local Token, demote token chip into disclosure by @jackspirou

### Nimbus-bin

- Silence unused-import warnings on Windows release by @jackspirou

### Ui

- Scrub leftover /app and /admin refs missed by rename pass by @jackspirou
- Rename persona URL prefixes /app→/developer, /admin→/operator by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.31...v0.1.32

## [0.1.31] - 2026-05-14

### Documentation

- Update CHANGELOG.md for v0.1.30 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.30...v0.1.31

## [0.1.30] - 2026-05-14

### Documentation

- Update CHANGELOG.md for v0.1.29 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.29...v0.1.30

## [0.1.29] - 2026-05-14

### Documentation

- Update CHANGELOG.md for v0.1.28 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.28...v0.1.29

## [0.1.28] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.27...v0.1.28

## [0.1.27] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.26...v0.1.27

## [0.1.26] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.25...v0.1.26

## [0.1.25] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.24...v0.1.25

## [0.1.24] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.23...v0.1.24

## [0.1.23] - 2026-05-14

### CI/CD

- Stabilize test lanes and node compat catalogs by @jackspirou
- Title case harness check names by @jackspirou
- Make harness gate names event-neutral by @jackspirou
- Speed workspace tests with nextest by @jackspirou
- Clarify workflow gate names by @jackspirou
- Stabilize checks after locker repin by @jackspirou
- Split Rust gates and trim coverage by @jackspirou
- Fix linux sqlcipher package proof by @jackspirou

### Documentation

- Update runtime compatibility and rename plans by @jackspirou
- Archive encryption at rest plan by @jackspirou
- Add generated node lts baseline by @jackspirou
- Update CHANGELOG.md for v0.1.22 by @github-actions[bot]

### Fixed

- Satisfy runtime linux clippy by @jackspirou
- Declare runtime libc dependency by @jackspirou
- Complete neovex→nimbus rename in remaining files by @jackspirou
- Fix hex encoding allocation, stale doc reference, add license path tests by @jackspirou
- Fix sanitize_dir_name edge cases, hoist allocation in env_local writer by @jackspirou

### Cli

- Harden onboarding flow and add node runtime plan by @jackspirou

### Deps

- Repin Deno fork to rusty_v8 locker release by @jackspirou
- Repin Deno fork security release by @jackspirou

### Rename

- Complete neovex→nimbus rebrand across entire codebase by @jackspirou

### Runtime

- Land node22 groundwork and lts plan by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.22...v0.1.23

## [0.1.22] - 2026-04-24

### Codegen

- Replace compile-time new Function paths by @jackspirou

### Engine

- Move provider behavior behind capability methods by @jackspirou

### Runtime

- Make service activation async and type the host ABI by @jackspirou

### Server

- Harden localhost access surface by @jackspirou

### Workspace

- Curate facade and JS verification contract by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.21...v0.1.22

## [0.1.21] - 2026-04-23

### Added

- Support native neovex source roots by @jackspirou

### Build

- Refresh Cargo.lock for v0.1.21 by @jackspirou
- Patch rustls-webpki and stabilize runtime coverage by @jackspirou
- Refresh vite and typescript toolchain by @jackspirou

### CI/CD

- Refresh GitHub Actions versions by @jackspirou

### Documentation

- Promote maintainability control plan by @jackspirou
- Update CHANGELOG.md for v0.1.20 by @github-actions[bot]

### Testing

- Serialize Postgres provider fixtures by @jackspirou

### Release

- V0.1.21 by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.20...v0.1.21

## [0.1.20] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.18 by @github-actions[bot]

### Fixed

- Gate cli progress helpers to unix builds by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.19...v0.1.20

## [0.1.19] - 2026-04-19

### Added

- Close out CLI alignment and add install tooling by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.18...v0.1.19

## [0.1.18] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.17 by @github-actions[bot]

### Testing

- Widen postgres repeated CRUD timeout by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.17...v0.1.18

## [0.1.17] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.16 by @github-actions[bot]
- Update CHANGELOG.md for v0.1.15 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.16...v0.1.17

## [0.1.16] - 2026-04-19



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.15...v0.1.16

## [0.1.15] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.14 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.14...v0.1.15

## [0.1.14] - 2026-04-18

### Machine

- Reflect guest override in non-unix stub by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.13...v0.1.14

## [0.1.13] - 2026-04-18

### Documentation

- Add storage and rename planning research by @jackspirou

### Testing

- Harden runtime isolation under coverage by @jackspirou
- Bound postgres repeated crud lane by @jackspirou
- Fix machine contract assertions off macOS by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.12...v0.1.13

## [0.1.12] - 2026-04-18

### Documentation

- Update CHANGELOG.md for v0.1.11 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.11...v0.1.12

## [0.1.11] - 2026-04-18

### Build

- Add linux distribution release tooling by @jackspirou

### Documentation

- Fix mermaid edge label syntax in bootc evaluation by @jackspirou
- Add bootc adoption evaluation research by @jackspirou
- Update CHANGELOG.md for v0.1.10 by @github-actions[bot]

### Cargo

- Inherit workspace package metadata by @jackspirou

### Dist

- Ship bundled gvproxy for macos by @jackspirou

### Engine

- Relax concurrent materialized load assertion by @jackspirou

### Machine

- Fix stale client fixtures and clippy by @jackspirou
- Harden macos convergence path by @jackspirou
- Harden guest api and service control by @jackspirou

### Sandbox

- Fix windows process handle typing by @jackspirou
- Make pid liveness probing windows-safe by @jackspirou
- Add podman-aligned oci builder by @jackspirou

### Server

- Collapse index read tracking match guards by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.10...v0.1.11

## [0.1.10] - 2026-04-17

### CI/CD

- Restore release target caching safely by @jackspirou
- Avoid stale release target caches by @jackspirou

### Fixed

- Gate unix-only protocol imports by @jackspirou
- Gate unix machine types on windows by @jackspirou
- Repair v0.1.10 ci lanes by @jackspirou

### Release

- Prepare v0.1.10 by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.9...v0.1.10

## [0.1.9] - 2026-04-17

### Documentation

- Add machine flow and deferred machine plans by @jackspirou
- Update CHANGELOG.md for v0.1.8 by @github-actions[bot]

### Testing

- Fix krun fake buildah unshare parsing by @jackspirou
- Harden executable test stubs by @jackspirou
- Run krun fake buildah via shell by @jackspirou
- Harden fake buildah script publishing by @jackspirou

### Release

- Prepare v0.1.9 by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.8...v0.1.9

## [0.1.8] - 2026-04-16

### CI/CD

- Opt release workflow into node24 actions by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.7 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.7...v0.1.8

## [0.1.7] - 2026-04-16

### CI/CD

- Make machine-os watcher attempt-aware by @jackspirou
- Document rerun-safe artifact naming by @jackspirou
- Stabilize machine-os staged artifact naming by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.5 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.6...v0.1.7

## [0.1.6] - 2026-04-16

### CI/CD

- Release machine-os before neovex by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.5...v0.1.6

## [0.1.5] - 2026-04-15

### CI/CD

- Dispatch machine-os publish workflow by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.4 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.4...v0.1.5

## [0.1.4] - 2026-04-15

### Build

- Use stable machine-os workflow ref by @jackspirou
- Repin machine-os workflow refs by @jackspirou
- Cache rusty_v8 artifacts by @jackspirou
- Repin machine-os performance updates by @jackspirou
- Shorten release critical path by @jackspirou
- Fix machine-os workflow pin by @jackspirou
- Reuse staged machine-os release bundles by @jackspirou
- Switch machine-os release flow to app auth by @jackspirou
- Repin machine-os reusable workflow by @jackspirou
- Use reusable machine-os release workflow by @jackspirou
- Dispatch native machine-os releases by @jackspirou

### CI/CD

- Harden workflow timeouts and permissions by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.3 by @github-actions[bot]

### Fixed

- Grant reusable machine-os workflow write access by @jackspirou
- Pin valid machine-os workflow commit by @jackspirou
- Use valid release workflow step ids by @jackspirou
- Match machine-os release run names by @jackspirou
- Account worker load before dispatch send by @jackspirou

### Testing

- Invoke fake buildah via shell launcher by @jackspirou
- Close fake buildah temp path before exec by @jackspirou
- Harden fake buildah helper creation by @jackspirou

### New Contributors
* @github-actions[bot] made their first contribution


**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.3...v0.1.4

## [0.1.3] - 2026-04-15

### Build

- Bump workspace to v0.1.3 by @jackspirou
- Pin machine-os release workflow contract by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.2...v0.1.3

## [0.1.2] - 2026-04-15

### Build

- Bump workspace to v0.1.2 by @jackspirou

### Fixed

- Narrow windows machine compilation seams by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.1...v0.1.2

## [0.1.1] - 2026-04-15

### Build

- Bump workspace to v0.1.1 by @jackspirou
- Patch rustls-webpki advisory by @jackspirou

### Fixed

- Gate machine module on unix hosts by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.0...v0.1.1

## [0.1.0] - 2026-04-15

### Documentation

- Harden machine image release contract by @jackspirou

### Testing

- Derive machine image version from crate version by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/machine-os/v0.1.2...v0.1.0

## [machine-os/v0.1.2] - 2026-04-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/machine-os/v0.1.1...machine-os/v0.1.2

## [machine-os/v0.1.1] - 2026-04-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/machine-os/v0.1.0...machine-os/v0.1.1

## [machine-os/v0.1.0] - 2026-04-14

### CI/CD

- Use authenticated googlesource path and update Cargo.lock by @jackspirou
- Add googlesource auth and cache-on-failure to all Rust jobs by @jackspirou
- Add Rust toolchain and cargo cache to deny job by @jackspirou
- Mark all workspace crates as unpublished for cargo-deny by @jackspirou
- Fix deny.toml for workspace custom license and path deps by @jackspirou
- Fix deny.toml for cargo-deny 0.19.0 by @jackspirou
- Fix deny.toml config, add weekly audit schedule, dependabot, and codecov config by @jackspirou

### Documentation

- Add macos machine support control plane by @jackspirou
- Archive external SQL provider plan by @jackspirou
- Restructure repo guidance and codex roadmap control plane by @jackspirou

### Fixed

- Isolate cooperative locker tests and annotate V8 reset repro by @jackspirou
- **deps**: Update Cargo.lock to submodule-free rusty_v8 tag by @jackspirou

### Miscellaneous

- Checkpoint remaining workspace changes by @jackspirou

### Testing

- Ignore snapshot-aware reset repro that SIGABRTs on cycle 2 by @jackspirou

### New Contributors
* @jackspirou made their first contribution


<!-- generated by git-cliff -->

