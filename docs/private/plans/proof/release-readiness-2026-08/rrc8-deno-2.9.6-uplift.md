# RRC8 Deno 2.9.6 Uplift

Date: 2026-08-29

Status: `in_progress`.

## Outcome

Nimbus will consume a reviewed, immutable fork of current upstream Deno
`v2.9.6`. The consumed Deno graph will use the corresponding reviewed Nimbus
V8 150.4 fork. No temporary revision, local path, or older V8 line can remain
in the final dependency closure.

## Baseline

- Upstream Deno `v2.9.6` peels to
  `e518fbd66dda5debcbdefc0beb0b3756b37b64fa`.
- The working Nimbus Deno checkpoint `v2.9.3-nimbus.2` peels to
  `d0a6b9094e0da6acbb53ecd0d88ed6b81a142e63`.
- Deno 2.9.3 through 2.9.6 contains 256 upstream commits and changes 918
  files. None of the 21 Nimbus commits is patch-identical to a 2.9.6 commit.
- Deno 2.9.6 introduces `deno_v8` 0.3.0 and requires rusty_v8 150.4.0.
- Upstream rusty_v8 `v150.4.0` peels to
  `5c15a6995c9bb4bacd3e341b59fff32c909c80bf`. It is eight commits and seven
  changed files after `v150.2.0`.
- The existing Nimbus V8 150.2 release peels to
  `4786595e29679ee5ad9ba4925cdcd1cc83ab6448`. Its 19 fork commits have no
  patch-identical adoption in upstream 150.4.
- The canonical Deno checkout retains the user's dirty `tests/wpt/suite`
  submodule. The canonical rusty_v8 checkout retains its five unrelated
  isolate-group files. All uplift edits use clean dedicated worktrees.
- The valid `v2.9.3-nimbus.2` Nimbus checkpoint uses
  `304a2e677293fec7d150e12ffc0ba98960917753`.

## Execution Ledger

| ID | Work | Status | Evidence |
| --- | --- | --- | --- |
| U1 | Port and verify the Nimbus rusty_v8 carries on upstream 150.4. | `complete` | Runtime candidate `dbb70a973d28cfe8cd6a2ea66d4f3d14fee488f0`; local source-build gates and final Sol xhigh review pass. U2 adds hosted-workflow repairs without changing runtime code. |
| U2 | Review, publish, and re-query an immutable rusty_v8 150.4 release. | `complete` | Public non-draft, non-prerelease release `v150.4.0-nimbus.1` peels to exact candidate `961a76d0cee88efdecfa9224c519fd153c404b51`. Branch runs `33361461904` and `33361461885` pass. Same-commit tag reruns `33376770979` and `33376771045` pass. A fresh download verifies 44 payloads and 44 checksum sidecars. |
| U3 | Audit the 21 Deno carries and replay only product-required concepts on upstream 2.9.6. | `complete` | Seven local commits end at exact reviewed correction `8d48dc4a68df8e083ed4b17855440b1df6405620`. Nimbus commit `d6636b980deedfeee8a64afb06230fa8a19a10a9` records the paired cleanup and public-tag repin. Controlled A/B, observed-mode crossover, full local CI, macOS, Linux debug-profile, application, desktop, and final Sol review evidence pass. The 8 GiB Linux full-LTO attempt is recorded as resource-blocked. |
| U4 | Test Deno itself and Nimbus against the exact unpublished candidate. | `complete` | Exact candidate `6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3` uses immutable rusty_v8 tag `v150.4.0-nimbus.1`. Locked workspace checks, formatting, warning-denied carry Clippy, 183 focused carry tests, 1,517 Node AES-GCM tests, and focused Nimbus integration pass. |
| U5 | Review and publish an immutable Deno 2.9.6 release. | `complete` | Public non-draft, non-prerelease release `v2.9.6-nimbus.1` uses annotated tag object `4d8b978255e8ca9a78d040531ee764d695fd3bcf`, which peels to exact candidate `6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3`. Branch run `33442674740` and tag run `33444743536` pass. The default branch is `nimbus/v2.9.6`. Sol xhigh review of the product code and each workflow correction passes. |
| U6 | Repin Nimbus, update fork policy, and run the exact release replay. | `in_progress` | Initial Nimbus candidate `d6636b980deedfeee8a64afb06230fa8a19a10a9` resolves 41 Deno packages at `v2.9.6-nimbus.1#6c37e683` and rusty_v8 at `v150.4.0-nimbus.1#961a76d0`. Fork provenance, upstream policy, standardization, the exact-tag all-target runtime check, the canonical runtime gate, and full repository CI pass. The final Sol review found eight valid corrections; their focused regressions pass. A clean follow-up review, release-critical smoke, artifact replay, and the higher-memory Linux release build remain. |

## Carry Rules

1. Preserve upstream 2.9.4 through 2.9.6 security and lifecycle fixes.
2. Retain a Nimbus carry only when current Nimbus source or a direct regression
   needs its contract.
3. Rework a carry at the current concept boundary when upstream refactored its
   old location. Do not restore removed upstream structure.
4. Bind Deno candidate tests to the same Nimbus rusty_v8 150.4 revision that
   Nimbus will consume.
5. Publish only annotated, never-moved fork tags after local checks and hosted
   CI pass. Run independent Sol xhigh review before publication. Do not invoke
   Opus 5 or Fable until the owner explicitly permits those reviewers again.
6. Preserve all unrelated dirty files and old branches, tags, releases, and
   proof artifacts.
7. Do not publish or repin the Deno candidate before the runtime-strategy
   disposition and controlled replay-scaffolding A/B are complete.

## U1 Evidence

The exact local rusty_v8 candidate is
`961a76d0cee88efdecfa9224c519fd153c404b51`. Runtime parent
`dbb70a973d28cfe8cd6a2ea66d4f3d14fee488f0` contains the V8 150.4 carry port
plus the persistent-handle, offline-binding, and release-workflow repairs.
Commit `0990fe0da72431f86bcebfd2dc9a5145dd7fcc00` adds the V8 Linux `glib-2.0`
development prerequisite to both hosted source-build workflows. Commit
`2eed57dce3eb88a2937318276481d92095057580` declares `rust-src` so the three
standard-library-referencing compile-fail snapshots have the same source
context on hosted Linux. Commit `62a8eddbfc3fa1f4d6a8554c87eb58cc898cbfe5`
tests checksum failures without panic unwinding after native Windows ARM64
aborted during a caught panic.

Commit
`597ebc820d8de0039ec10b84f9f7adc0645c6db9` gives only Nimbus public runners
360 minutes for cold matrix builds. The final commit adds bounded retries to
the public `sccache` download and disables matrix fail-fast only on Nimbus
versioned branches. Upstream branch behavior keeps its 180-minute limit and
fail-fast policy.

- `V8_FROM_SOURCE=1 cargo nextest run --all-targets --locked` passes 308 tests
  across 25 binaries.
- Warning-denied all-target Clippy and Rust formatting pass.
- Documentation tests pass 13 tests and ignore 13 examples by declaration.
- C++ formatting and the Nimbus release workflow pass their local syntax
  checks.
- The release tool suite passes 15 tests. The candidate selects
  `v150.4.0-nimbus.1` and defines exactly 44 assets across seven targets.
- Action lint accepts the exact workflow. Local `curl` help confirms support
  for its retry options. The final exact-commit Sol review reports no finding.
- Sol xhigh found and closed two P2 defects: stale offline binding reuse and
  mutable release action references. The final exact-commit review reports no
  P0 through P3 finding.
- The owner prohibited Opus 5 and Fable review use. The active Opus 5 review
  then stopped with exit 130. It is not acceptance evidence. No Opus 5 or Fable
  review can run until the owner changes this constraint.

The owner authorized the versioned branch, annotated tag, release, and
downstream repin on 2026-08-30. Public release `v150.4.0-nimbus.1` is complete
at exact commit `961a76d0cee88efdecfa9224c519fd153c404b51`.

## U3 Carry Audit

The pre-edit audit used the clean Deno 2.9.6 worktree at `e518fbd66`. Upstream
source contained none of the Nimbus Locker, egress, or near-heap policy
symbols. It already contained the independent residual lazy-source design. The
candidate keeps that design without the rejected extension replay tables.

Current Nimbus source directly consumes the Locker API, warm-runtime safety and
reset methods, the shared read-only heap lock, public `JsRealm`, and the bounded
foreground-task drain. The last helper supports the Node compatibility GC test
operation and is not a fresh-realm-only consumer.

| Old commit | Contract | U3 disposition |
| --- | --- | --- |
| `8d92e814b9` | Mixed core seam | Split. Rework Locker ownership, warm reset, shared-heap serialization, public `JsRealm`, and foreground-task access. Drop fresh-realm creation, realm module APIs, and replay-source state. |
| `a4ae13cbd0` | Lazy ESM termination | Retain at `modules/map/ext_script.rs`. Deno 2.9.6 still unwraps four lazy-module evaluation results. |
| `b126c80e82` | Target-realm module pump | Drop with fresh-realm execution. |
| `c0f65c495b` | Fetch URL gateway hook | Retain and rework at the 2.9.6 fetch seam. |
| `6eb1f60880` | Runtime option defaults | No independent carry. Add only defaults that retained fields require in current constructors. |
| `24ee10a3e0` | Nimbus fork CI | Retain as one 2.9.6 workflow. |
| `c506b7038b` | Mixed realm hardening | Retain the `ManagedIsolate` correction. Drop replay policy and realm-only tests. |
| `b1eecac7eb` | Node near-heap policy | Retain. Deno 2.9.6 has no equivalent embedder policy. |
| `a5c8262747` | Fetch pre-transport proof | Retain with the gateway contract. |
| `56d3232925` | Patched-crate CI coverage | Fold into the one 2.9.6 fork workflow. |
| `0cc2cd33d0` | Extension replay policy | Drop with replay-source state. |
| `4f59b4246c` | Security-fix CI coverage | Fold current patched and security-sensitive crates into fork CI. |
| `5d07e09121` | WebSocket gateway hook | Retain and rework at both WebSocket client seams. |
| `4136492f7f` | TCP linger maintenance | Retain. Deno 2.9.6 still calls the deprecated Tokio method. |
| `63551c0aaf` | WebSocket target binding | Retain. |
| `c13c73ce75` | WebSocket decision cache | Retain as part of the final bounded client design. |
| `585370ebeb` | Resolved-address checks | Retain for fetch, proxy, KV, and WebSocket connect paths. |
| `a0d2eb8329` | Checked custom clients | Retain the fail-closed contract. |
| `1c17e86b29` | Proxy and custom-client guards | Retain the non-vacuous regressions. |
| `0492e1acc8` | Gateway client contracts | Retain the complete checker propagation rules. |
| `d0a6b9094e` | Equivalent checked-client cache | Retain the bounded 16-entry reuse design and opaque policy key. |

The audit found no supported CLI, package, server, or compute selector for
`WarmContextRecycle`. Nimbus still exposes the serialize-only Rust value and
product branches. The paired Nimbus cleanup must remove them before U4. Any
supported external consumer found during compilation reopens this disposition.

The controlled replay-scaffolding result is in
`rrc8-replay-scaffolding-ab.md`. WebStandard showed no detected construction
change. A counterbalanced Node22 row measured a small favorable replay-off
result. The exact Node22 replay tables were empty and cost 16 encoded bytes.
The old Web pool label was not used as proof of startup-snapshot restoration.

## U3 Local Candidate

The unpublished Deno candidate contains seven concept commits over upstream
`e518fbd66dda5debcbdefc0beb0b3756b37b64fa`:

1. `350847b6bb4120433de27de5f545acdbe14e830e` ports Locker ownership,
   warm-runtime reset, shared-heap safety, and the generic lazy-ESM error
   contract without fresh-realm APIs.
2. `15abd6ff0e10348978767d0eb801b027a6e9b80d` preserves the Node near-heap
   policy and applicable TCP maintenance.
3. `9ca28c0bc15899fbd2437372c9935c163fea5440` ports fetch and WebSocket
   egress enforcement, resolved-address checks, fail-closed custom-client and
   proxy behavior, and bounded equivalent-client reuse.
4. `c1ca1bf3d155684e17e82061f59727232794acc5` adds the current fork CI.
5. `792fc50fa9cce5122f08fff997ab3dfac29470ae` restores the versioned Node
   domain and capture-callback contract required by Nimbus fixtures.
6. `e8fffe9029283b5f51111647ce5e2a79eadf5ef2` repairs a snapshot defect.
   Upstream 2.9.6 omitted recursively consumed lazy ESM dependencies from the
   companion sources. Its regression restores a root module with a lazy
   dependency.
7. `8d48dc4a68df8e083ed4b17855440b1df6405620` generation-fences retained task
   spawners across warm resets. It binds foreground draining to a live
   `PinScope` and repairs current initializers. Fork CI runs the changed Node
   module integration lane.

The seven-commit checkpoint contains no `create_realm`, realm-scoped module
pump, extension replay table, or other rejected fresh-realm carry. A reviewed
correction is the seventh commit. The branch remains local until U4 passes. U5,
the Deno tag, release, and Nimbus repin remain unstarted.

The Sol-only branch review found three valid defects and one refuted claim:

- A cross-thread task spawner retained by an old request could enqueue after a
  warm reset. The correction generation-fences queued work and rotates both
  spawner types in `OpState` for each new lease.
- The public foreground-task drain accepted only a copied isolate key. The
  correction now accepts a live mutable `PinScope`, which pins the isolate and
  proves exclusive V8 and Locker access. Nimbus passes that scope directly.
- Fork CI compiled the changed Node polyfill but did not run its module
  integration test. The workflow now builds the fork CLI and test server, then
  runs `module_test`.
- The proposed `deno_kv` unit lane was not valid. The crate has zero Rust
  tests, and the only fork change is a required fetch-client initializer.
  Workspace check and the existing carry-crate Clippy lane cover it.

The Sol xhigh follow-up review reports no actionable P0 through P3 finding in
the corrective diff. No Opus 5 or Fable reviewer ran.

A later full Nimbus Sol review reported two items. Its P2 crossover finding was
valid: separate trace greps could combine fields from different JSONL rows.
The verifier now validates the complete identity and construction mode on each
selected Node and Web record. Three helper tests include the mixed-row
regression. The reported P3 cancellation-test deletion was false. The test is
still registered, and its body matches `HEAD` at SHA-256
`725682b6e2515ba0e46fabcbd40e696d92af684b8c338dd20d17e680147945cf`.

The Sol-only crossover follow-up found three valid evidence defects:

- The trace reported a construction mode inferred from the requested profile.
  Successful runtime construction now increments exactly one runtime-owned
  startup-snapshot or unsnapshotted counter after bootstrap finalization. The
  trace derives its mode from those counters and rejects absent or mixed modes.
- Strategy was not a separate trace field. Each row now reports both the
  policy pool kind and the selected strategy. WebStandard reports
  `unsnapshotted_runtime_cache` instead of reusing the misleading
  `startup_snapshot_cache` label.
- The success test did not assert its validator result. It now requires the
  exact two-value pool-kind set.

The next Sol-only review found two valid verifier defects. A caller-provided
trace directory could retain valid rows from an earlier run, and an unknown
expected construction mode could pass when both counters were zero. Each
script invocation now creates a unique run directory below the requested root.

The validator also rejects every mode except `startup_snapshot` and
`unsnapshotted` before it reads rows. The Sol-only follow-up found no remaining
trace defect. Its only P1 is the known release-order gate: the repository's
normal Cargo graph cannot use the unpublished Nimbus Deno and V8 revisions.
The temporary U3 Cargo config selects both exact local worktrees. No Opus 5 or
Fable review ran.

The paired Nimbus candidate removes `WarmContextRecycle` from the public policy
and all product execution paths. It removes the realm lease, realm lifecycle,
fresh-realm metrics, replay companion tables, and realm-only verifiers. It
keeps exact-key `WarmPool`, `CooperativeLocker`, residual lazy sources, the
Node startup snapshot, Web unsnapshotted construction, heap limits, pointer
compression, shared-heap safety, and reset-or-destroy behavior.

The Nimbus integration also makes three independent corrections:

- Guest-visible Deno and V8 versions now come from a runtime op instead of
  stale JavaScript literals.
- Snapshot companion parsing rejects trailing legacy replay tables and
  impossible table counts before allocation.
- The scheduled crossover and execution-classification verifiers now name the
  selected runtime strategies and current source owners. Web rows must report
  actual unsnapshotted construction.

Focused exact-candidate evidence is green:

- Deno snapshot tests pass 11 tests. Lazy-ESM tests pass 6 tests. Node module
  tests pass 29 tests. The Node startup-snapshot fixtures pass 4 tests.
- Full `deno_core` passes 476 tests with 2 declared ignores. Its task-spawner
  subset passes 4 tests, including stale-after-reset and concurrent-reset
  regressions. Both warm-reset tests pass.
- The exact fork-built Node module integration lane passes. Warning-denied
  `deno_core` Clippy and formatting pass.
- The full Deno workspace check passes against the local V8 candidate. The
  carry-crate test lane passes 183 tests. Warning-denied carry-crate Clippy,
  `denort_helper` Clippy, and Deno binary Clippy pass.
- Nimbus runtime compilation and benchmark compilation pass against the local
  Deno and V8 candidates.
- Nimbus focused runtime suites pass 124 tests. The cases cover limits,
  execution planning, warm pooling, cooperative scheduling, and snapshot
  lifecycle. They also cover retained pooling, metrics, dispatch, ordering,
  and bundle integrity. The harness declares 62 subprocess wrappers as ignored.
- The runtime-version regression passes 1 test, and snapshot parser
  hardening passes 3 tests.
- Tenant isolation passes 19 checks. Execution classification passes 24
  checks.
- The broad non-Node runtime replay passes 498 tests with 94 declared ignores.
  The generated anchor passes 1 test, all 8 Locker integration tests pass, and
  the active runtime doctest passes.
- The crossover trace helper passes 8 tests. Focused Node22 and WebStandard
  tests prove the mutually exclusive successful-construction counters.
  Warning-denied all-target runtime Clippy passes.
- The exact local graph passes both record-aware crossover lanes. Node22
  startup snapshots measure 7.8742 to 7.9433 milliseconds. Warm-pool hits
  measure 2.4789 to 2.6142 milliseconds. The WebStandard unsnapshotted cache
  measures 18.785 to 19.211 milliseconds. Warm-pool hits measure 1.2286 to
  1.2669 milliseconds. A replay against a reused trace root created a fresh
  nested directory and passed on newly emitted rows.
- A final 10-sample replay stored both raw traces under the RRC8 artifact
  directory. Node22 startup snapshots measured 7.3336 to 7.4671 milliseconds,
  and warm-pool hits measured 2.3735 to 2.4068 milliseconds. WebStandard
  unsnapshotted construction measured 17.574 to 17.703 milliseconds, and
  warm-pool hits measured 1.1454 to 1.1801 milliseconds. Each schema-v3 trace
  has 16 records and the shared run ID `nimbus-pir-crossover.edYwHZ`. Their
  SHA-256 values are
  `35ac582453af2e249814c5b34af526ab15ee891e4a876bdf6b36d66fe680023e`
  for Node and
  `2a83b81438c5e02e5ac74cbdede41797a6a4094fef3bea551801cbdb9e46dfa6`
  for Web.

The broad Nimbus replay exposed a `TempDir::into_path()` warning. Nimbus
resolves a newer `tempfile` 3.x than Deno's locked 3.10.1. That locked version
does not have `TempDir::keep()`. The two persistent-directory call sites now
have narrow, reasoned deprecation allowances. This keeps both supported graphs
compilable without an unrelated dependency-floor uplift. Deno's locked helper
check, the local-candidate Deno binary check, and warning-denied Clippy pass.

## U3 Local QA Replay

The unpublished local graph passed the release-critical executable replay:

- macOS native product smoke passed fresh-root health, authorization, schema,
  indexes, CRUD, pagination, WebSocket, scheduler, diagnostics, shutdown,
  restart, durability, and delete flows. The candidate binary has SHA-256
  `6be740f7bc43ffd4dd1256b674b20aab2d5198ed6b7633fd28ceb58d90e11b2b`.
- The embedded UI passed one Chromium smoke with a 10-step product walk. The
  application harness passed all 9 applications and 37 anchors. Its report has
  SHA-256
  `3714b49a8c37f0306ebfdc5ff655910341631d19fcf733a3a6298bf3cf583f53`.
- The clean Desktop worktree is at
  `bbc103f84b2a88e2baa4b522e45447bed31e04c7`. It passed lint, typecheck, 186
  unit tests, and 5 packaged Electron tests. Fuse, signature, arm64, and x86_64
  checks also passed.
- An isolated Debian 13.4 x86_64 debug-profile replay ran on
  `minicloud.local`. It built Nimbus 0.1.46 and passed the native product
  smoke. All 9 applications also passed. The Linux binary has SHA-256
  `f49d5d59297835196cabaee728ff9a9112a43924b6547516728b6dfe4a5d3536`.
  The application report has SHA-256
  `bce219cd315f119556465e9f810ed96b0d7fc74f52555c8e914efa1b74314b57`.
- Full local `make ci` exited zero. It passed 498 focused runtime tests and
  7,722 workspace tests. Required liveness, protocol, and JavaScript lanes also
  passed. The gate passed 846 UI tests, proof helpers, and 60 installer checks.
  Declared inventory was 94 focused runtime ignores and 111 workspace skips.

### Exact Linux release-build attempt

The same Debian host attempted the exact unpublished graph with release
optimization, full LTO, one code-generation unit, pointer compression, and one
Cargo job. The temporary Cargo configuration had SHA-256
`551a67844077e3954183a3231fe42562eae8bf1301830957d029475c8d8bdce1`.
The build completed the UI, embedded package generation, and every Rust
dependency. It then entered the final `nimbus-bin` link with this effective
Cargo command:

```text
cargo build --release --config /home/nimbus/nimbus-rrc8-u3-linux-patches.toml \
  -p nimbus-bin --features v8-pointer-compression -j 1
```

This attempt did not produce a release binary. After 9 hours 57 minutes, the
final `rustc` process used only 11 minutes 29 seconds of CPU time. It retained
approximately 6.8 GiB of resident memory on a 7.7 GiB host. It recorded
approximately 4.0 million major page faults.

Its LLVM worker waited in
`folio_wait_bit_common`. The original 8 GiB swap device was full. A temporary
second 8 GiB swap file and `vm.page-cluster=0` reduced some read amplification
but did not restore useful progress. This is a host-memory limit, not a
compiler or product failure.

The release manager stopped the resource-bound process after the bounded
health check. The 4.6 GiB release compilation cache remains for a higher-memory
continuation. Cleanup disabled and removed the temporary swap file. Cleanup
also restored `vm.page-cluster` to `3`.

The host ended with 6.9 GiB free memory
and 129 GiB free disk. The 8 GiB host cannot produce the exact full-LTO release
artifact. Repeat this lane on a higher-memory runner after the immutable graph
exists. Do not substitute the debug-profile smoke binary as release evidence.

The replay also repaired four release-harness defects:

- Source capture now fails closed and ignores generated `.env.local` files.
- Cargo-deny understands the Deno 2.9.6 libuv and bindgen graph.
- The lockfile replaces yanked `chacha20` 0.10.0 with 0.10.2.
- Test-only snapshot fallback uses an accurate diagnostic.

One Sol xhigh review repeated the known normal-graph P1. It also found
that deleting the historical PIR verifier deleted its durable entry point. U3
restored that name as a compact current-contract aggregator. It did not restore
rejected realm checks.

The gate now executes 24 REC checks, 19 tenant-isolation
checks, 32 TFA checks, 8 crossover-trace tests, and a rejected-symbol check.
It validates the two saved raw benchmark traces. The validator requires one
nonempty run identity across every selected record in one file. The closeout
gate derives the validated Node identity and requires the Web artifact to use
the same value. It also rejects duplicate or non-increasing measurement series.
These checks prevent mixed or appended prior runs from satisfying the evidence
gate.

The repair also updated stale TFA paths and the compute ownership boundary. Focused
replay passes. No Opus 5 or Fable review ran.

The final Sol xhigh follow-up reports no remaining trace-integrity defect. Its
two P1 findings repeat the known requirement to publish immutable fork sources
and repin the normal Cargo graph.

## U4 Exact Candidate

Deno commit `15b0156a3033bcb327b92a4200355aca82ac23be` corrects the
versioned Node AES-GCM short-tag contract. Node 20 and Node 22 allow an
implicit short tag and warn only with `--pending-deprecation`. Node 24 allows
it and warns by default.

Node 26 denies the short tag. Native input validation occurs
before the one-shot warning state changes, so invalid input cannot consume the
warning. Nimbus selects these policies by compatibility target. Focused tests
cover the Node 20 silent case and the Node 24 warning-order case.

Deno commit `96832ab2dbbe711842d13d0d0aeaf88f8387a5b3` pins
`v150.4.0-nimbus.1`. The lockfile resolves rusty_v8 to
`961a76d0cee88efdecfa9224c519fd153c404b51`. The exact unpublished candidate
passes these local gates:

- locked workspace check and formatting.
- warning-denied Clippy for all carry crates.
- 477 `deno_core` tests with two declared ignores.
- 183 combined `deno_fetch`, `deno_node`, `deno_runtime`, `deno_websocket`,
  and `serde_v8` tests.
- all 1,517 tests in `crypto_cipher_gcm_test.ts`.
- three focused Nimbus mapping and Node 20 and Node 24 regression tests.

The Sol xhigh product review found two valid GCM defects. The first policy did
not distinguish Node 20 and 22 pending deprecation from Node 24 default
deprecation. It also changed the warning state before it validated the input.
The correction closes both defects. The exact follow-up reports no accepted or
actionable finding. No Opus 5 or Fable review ran.

The first hosted branch run `33437871563` found that the isolated `deno_node`
command did not enable a V8 backend. Commit
`8b103c94398f74a0a970ea29cef4632b41f0ad4b` adds
`deno_core/default`. All 98 tests pass locally and on the next hosted run.
Commit `93e9cb769a5ee427bf6f76cdd08d5600e0642e91` also adds the focused
AES-GCM integration case. Run `33439285625` then found that the Node module
test imports the pinned `tests/util/std` submodule, while the fork checkout
intentionally omits submodules. Commit
`079a91b1ceb8089c1d004cfb6d5c4a72a555593f` initializes only that depth-one
fixture.

Run `33440827042` passed Node integration and then found the same missing-backend
condition in isolated fetch tests. Commit
`6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3` selects V8 for each remaining
isolated carry test. Exact branch run `33442674740` passes. Focused Sol-only
reviews of all three workflow corrections report no accepted or actionable
finding.

Annotated tag object `4d8b978255e8ca9a78d040531ee764d695fd3bcf`
peels to the candidate. Branch run `33442674740` and tag run `33444743536`
pass. Public release `v2.9.6-nimbus.1` is non-draft and non-prerelease, and the
fork default branch is `nimbus/v2.9.6`.

## Candidate Identity

The fork identities are:

- Public rusty_v8 tag `v150.4.0-nimbus.1` peels to
  `961a76d0cee88efdecfa9224c519fd153c404b51`.
- Public Deno tag `v2.9.6-nimbus.1` peels to clean candidate
  `6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3`.
  Its seventh commit, `8d48dc4a68df8e083ed4b17855440b1df6405620`,
  contains the reviewed nine-file diff with pre-commit SHA-256
  `6bcaa1948d86ef65af2a7ccb65f4a1d21ee7687fbd98d9730e35ab6de1d57b55`.
  Later commits add the GCM policy, immutable V8 pin, and hosted gate repairs.
- Nimbus is at `e0cbb5937d5390d44a597b6ef45ed7003e267a03` plus a
  product, verifier, and proof change bundle. U6 owns its exact commit and
  public-tag repin.
- Nimbus `Cargo.toml` and `Cargo.lock` now select only the two public Nimbus
  tags. The lock resolves all 41 Deno packages to
  `6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3` and rusty_v8 to
  `961a76d0cee88efdecfa9224c519fd153c404b51`. No temporary revision or
  local-path fork source remains.

## U3 Architecture Disposition

Decision: omit the proven realm-only carries during the first 2.9.6 replay.
Port by product contract. Do not cherry-pick all 21 commits.

Evidence:

- The U3 product checkpoint is
  `8d48dc4a68df8e083ed4b17855440b1df6405620`, seven commits after upstream
  `e518fbd66dda5debcbdefc0beb0b3756b37b64fa`.
- Commit `8d92e814b9` mixes Locker ownership, teardown, shared-heap work, and
  fresh-realm replay in one 1,705-line change. A mechanical replay cannot keep
  those contracts separate.
- Nimbus defaults to exact-key `WarmPool` with `CooperativeLocker`. Node targets
  use startup snapshots, while WebStandard uses unsnapshotted construction.
- `WarmContextRecycle` is a public, serialize-only Rust value with live product
  branches. Both facade crates re-export it. Server and UI surfaces report it,
  but the audit found no CLI, package, server, or compute selector.
- PIR2 rejects realm recycle at 2.23 to 2.50 times its Web comparison lane.
  NFR6 rejects it at 5.38 to 10.01 times Node startup snapshots. It is 13.35 to
  16.13 times exact warm pools.
- The exact A/B found no detected Web construction change and a small favorable
  Node result after counterbalancing. The two Node22 replay tables were empty
  and cost 16 encoded bytes. Removal is primarily a product and fork
  simplification, not a material snapshot-size optimization.

U3 must preserve Locker ownership, egress enforcement, the Node near-heap
policy, applicable TCP maintenance, and fork CI. It must audit shared-heap and
teardown hunks independently. It must also keep the generic lazy-ESM termination
error contract unless a current regression proves that upstream replaced it.

The paired Nimbus candidate removes `WarmContextRecycle` product selection
before U4. It also removes fresh-realm execution and replay companion data. It
keeps residual lazy sources, exact owner and reuse authority, heap limits,
shared-heap safety, pointer compression, and reset-or-destroy behavior. U3 must
stop before realm omission if its caller audit finds a supported external
consumer.

`rrc8-warm-context-recycle-archive.md` archives the rejected experiment.
`rrc8-replay-scaffolding-ab.md` contains the controlled construction and size
study. Both records bind the exact source SHAs and runtime settings.

The proposed `runtime-strategy-lifecycle-plan.md` consumes this proof after U6.
Its activation trigger is terminal U6 evidence with exact commits and A/B data,
followed by owner approval. RRC8 keeps exclusive ownership until that trigger.

## Acceptance

- rusty_v8 formatting, compile-fail, Locker, weak-handle teardown, selector,
  release-manifest, and candidate asset checks pass at one exact 150.4 commit.
- Deno locked workspace checks and all Nimbus-patched crate tests pass at one
  exact 2.9.6 commit. Egress regressions prove URL and resolved-address policy,
  proxy behavior, custom-client handling, and bounded equivalent-client reuse.
- The Deno candidate contains no realm-only carry without a supported consumer.
  The archived experiment and the controlled replay-scaffolding A/B exist before
  any Deno tag or Nimbus repin.
- Nimbus compiles and passes runtime, bridge, NodeFull, egress, teardown,
  snapshot, and cage gates against the exact unpublished candidate.
- Both fork releases are public, non-draft, non-prerelease, immutable, and have
  successful branch and tag CI at the peeled commits.
- Nimbus consumes published tags only. Provenance, upstream standardization,
  Deno/V8 coupling, `make ci`, macOS, Linux, application, desktop, archive, and
  OCI candidate proofs pass before the release verdict can change.

## Current Next Action

Record the exact Nimbus candidate commit on the published-tag graph. Repeat the
release-critical smoke and artifact lanes from that commit, produce the
higher-memory Linux full-LTO binary, and update the final release verdict.
