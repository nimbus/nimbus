# NNC9.5 Repository Gates

Status: `complete`

Starting checkpoint:
`61da5905d1763b8fd46337c7fe3f8cf44602d5ba`.

## Unit Of Value

NNC9.5 proves that the complete network-control-plane branch satisfies the
repository gates. It records real command exits and exact summaries. It does
not change product behavior unless a required gate exposes an in-scope defect.

## Acceptance Contract

| ID | Verifiable criterion |
| --- | --- |
| K1 | The gate run starts from the dedicated owner worktree at the exact NNC9.4 checkpoint with no unrelated process or dirty path. |
| K2 | `cargo fmt --all --check` exits `0`. |
| K3 | The fixed-seed full `nimbus-network --all-features` suite and the network-control-plane verifier pass with exact counts. |
| K4 | `make clippy` exits `0` through the repository single-flight entrypoint. |
| K5 | Both documentation gates exit `0` with exact page and condition counts. |
| K6 | `make ci` exits `0` through the repository entrypoint and records its Rust, harness, JavaScript, and proof-helper summaries. |
| K7 | The proof preserves the real exit of each command. No pipe, interrupted process, unavailable target, or hidden skip becomes a pass. |
| K8 | Expected ignored tests and hosted-only lanes stay explicit. NNC9.5 does not claim local evidence for a lane that `make ci` does not run. |
| K9 | One GPT-5.6 Sol/xhigh/fast item review runs only after K1-K8 are green. A narrow review runs only for an accepted executable defect. |
| K10 | The proof, concise ledger transition, and exact item commit form one checkpoint. No push or PR occurs. |

## Environment And Start State

| Field | Value |
| --- | --- |
| Worktree | `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit` |
| Branch | `codex/nimbus-network-architecture-audit` |
| Host | Darwin `24.6.0`, `arm64` |
| Rust | `rustc 1.96.1 (31fca3adb 2026-06-26)` |
| Cargo | `cargo 1.96.1 (356927216 2026-06-26)` |
| Node / npm | Node `v26.0.0`; npm `11.12.1` |
| Competing gate processes | None before the item run. |
| Dirty paths | This proof and the NNC9.5 recovery checkpoint only. |

## Gate Ledger

| Gate | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --all --check` | exit `0` |
| Focused behavior | `PROPTEST_RNG_SEED=202608160905 cargo test -p nimbus-network --all-features -- --test-threads=1` | exit `0`; `276` passed, `0` failed, `1` parent-owned child ignored |
| Architecture verifier | `bash scripts/verify-nimbus-network-control-plane.sh` | exit `0`; current regenerated evidence passes `39/39` conditions |
| Clippy | `make clippy` | exit `0`; Nimbus targets passed with warnings denied; unchanged vendored Brotli emitted warnings |
| Docs | `bash scripts/check-docs.sh` | exit `0`; `108` pages passed |
| Docs site | `bash scripts/verify-nimbus-docs-site.sh` | exit `0`; `17/17` conditions passed |
| Required local CI | `make ci` | exit `0`; runtime passed `517` with `134` declared ignores; workspace nextest passed `7,405/7,405` with `107` skipped; Rustdoc, the required harness, JavaScript build/typecheck/tests, and proof helpers passed |
| Focused advisories after correction | `cargo deny -f json check advisories --hide-inclusion-graph` | exit `0`; `0` errors and `2` policy-visible yanked `spin` warnings |
| Full dependency policy after correction | `cargo deny check` | exit `0`; advisories, bans, licenses, and sources passed; the same `2` yanked `spin` warnings stayed visible |
| Hakari | `cargo hakari generate --diff` | exit `0` |
| Attribution | `bash scripts/verify-third-party-attribution.sh` | exit `0`; `1` guarded crate and `5` vendored patches passed |
| Compiler authority | refresh, deep check, and aggregate verifier | exit `0`; `6` packages, `16` resolved calls, `5` generated outputs, and `39/39` aggregate conditions passed |
| Technical prose | technical-writing linter on the complete proof and changed plan, attribution, and notice text | exit `0`; only passive-voice warnings remain |
| Item review | Nimbus autoreview, Sol/xhigh/fast | complete; one P2 and two P3 findings corrected; narrow correction review clean at `0.99` |

## Repository-Gate Fail-Before

The first complete `make ci` run passed its format and Clippy stages. The
command then stopped at `cargo deny check` with these findings:

| Finding | Package path | Fixed state reported by the gate |
| --- | --- | --- |
| `RUSTSEC-2025-0167` | `bitmaps 3.2.1 -> imbl 7.0.0 -> nimbus-engine` | No fixed `bitmaps` release is available. |
| `RUSTSEC-2026-0221` | `event-listener 5.4.1 -> async-broadcast 0.7.2 -> zbus 5.15.0 -> nimbus-node` | Update `event-listener` to `5.4.2` or later. |
| `RUSTSEC-2026-0253` | `lru 0.16.3 -> mysql_async 0.36.2`, `pingora-cache 0.8.1`, and `pingora-core 0.8.1` | The current consumers require `0.16`. Registry `0.16.4` remains affected, so Nimbus must backport the merged upstream fix. |
| `RUSTSEC-2026-0222` | `wasmtime 46.0.1 -> nimbus-runtime` | Update `wasmtime` to `46.0.2` or another listed fixed release. |
| `RUSTSEC-2026-0223` | `wasmtime 46.0.1 -> nimbus-runtime` | Update `wasmtime` to `46.0.2` or another listed fixed release. |
| Yanked package | Direct `fjall 3.1.5` and its `lsm-tree 3.1.5` dependency | Update the compatible `3.1.x` storage release. |
| Yanked package | `spin 0.9.8 -> flume 0.12.0 -> fjall` and `spin 0.10.0 -> crc-fast 1.10.0` | Upstream consumers pin these old major lines; the repository policy reports them as warnings. |

The focused reproduction exited `1` with exactly five advisory errors and four
yanked warnings. Current `main` resolves every listed package at the same
version, so the findings came from current advisory and yank state rather than
the network branch. No advisory is waived. The correction removes the
unmaintained `bitmaps` path and uses compatible fixed releases or the exact
upstream backport for every other advisory. The two transitive `spin` pins stay
visible under the repository's existing warning policy unless their upstream
owners publish compatible updates.

## Bounded Correction

| Defect | Correction |
| --- | --- |
| `bitmaps 3.2.1` has no fixed release. | Replace the only `imbl::OrdMap` use with `rpds::RedBlackTreeMapSync`. This keeps ordered structural sharing and removes `imbl` and `bitmaps` from the graph. |
| `event-listener 5.4.1` is affected. | Resolve `event-listener 5.4.2` through the existing `zbus` consumer graph. |
| Registry `lru 0.16.4` remains affected. | Patch `lru 0.16.4` locally with upstream commit `f9a7f00fcf2d33e00adb03758cb350aaaa52cddb`. The patch detaches a node before key destruction can panic and includes the upstream regression test. |
| `wasmtime 46.0.1` has two advisories. | Update the complete Wasmtime component set and the runtime engine-cache identity to `46.0.2`. |
| `fjall 3.1.5` and `lsm-tree 3.1.5` are yanked. | Update both compatible packages to `3.1.9`. |

No advisory waiver was added. The repository policy reports yanked packages as
warnings. `flume 0.12.0` and `crc-fast 1.10.0` still require `spin 0.9.8` and
`spin 0.10.0`, so those two warnings remain visible.

## Focused Correction Evidence

| Seam | Command | Result |
| --- | --- | --- |
| Engine persistent write log | `cargo test -p nimbus-engine --lib write_log` | exit `0`; `21` passed and `652` filtered |
| Patched `lru` regression | `cargo test test_pop_panicking_key_drop_keeps_list_consistent` from `third_party/lru-0.16.4` | exit `0`; `1` passed |
| Patched `lru` suite | `cargo test` from `third_party/lru-0.16.4` | exit `0`; `53` unit tests and `43` documentation tests passed |
| Wasmtime runtime | `cargo test -p nimbus-runtime --lib wasmtime -- --test-threads=1` | exit `0`; `17` passed and `1,376` filtered |
| Storage | `cargo test -p nimbus-storage --lib -- --test-threads=1` | exit `0`; `303` passed and `2` expected harness tests ignored |
| Node consumers | `cargo check -p nimbus-node --all-targets` | exit `0`; all targets compiled with only unchanged vendored Brotli warnings |

## Compiler Evidence Refresh

The first aggregate verifier after the dependency correction passed `38/39`.
NNCV038 failed because its authenticated input and parsed-source digests still
described the pre-correction tree. The first baseline refresh then exited `101`
when Rustc reported `No space left on device`. It did not produce a passing
baseline.

The owner worktree held 136.5 GiB of reproducible Cargo output. A target-scoped
`cargo clean` removed those artifacts and left source and evidence intact. The
next refresh completed with six packages and five generated outputs. A separate
deep collection reproduced all 16 resolved calls and generated outputs. The
aggregate verifier then passed `39/39`.

## Candidate CI Workspace Reds

The post-correction candidate `make ci` preserved exit `2`. Its runtime lane
passed `517` with `134` declared ignores. The workspace nextest lane then ran
all `7,405` selected tests in `407.354s`: `7,401` passed, three failed, one
timed out, and `107` were skipped.

| Case | Observed boundary |
| --- | --- |
| Container two-thread network teardown | The losing claimant timed out after five seconds on the provider journal lock while the winner still held it through detach. |
| Container fresh-process network checkpoint matrix | Nextest stopped the parent at its `135s` per-test limit. |
| Workload-saga fresh-process phase matrix | The writer reached `ready` but not its durable phase checkpoint within the harness's `20s` bound. |
| Workload-saga fresh-process restart matrix | The writer reached `ready` but not its durable restart checkpoint within the harness's `20s` bound. |

All four cases ran during the same loaded workspace window. This proof does not
classify them as flakes or passes. The next action is one serialized exact-case
reproduction, followed by source diagnosis and the smallest owning correction.
K6 remains red, so K9 review is not authorized.

The exact serialized reproduction passed `4/4` in `55.765s`. The Container
contender passed in `1.242s`, the Container checkpoint matrix in `27.116s`,
and the two saga matrices in `13.687s` and `13.712s`. This proved that the
failures depend on full-suite load, but it did not make the failed candidate a
pass.

## Workspace Scheduling Correction

The two backend thread-contender helpers let the winning claimant start its
provider work before the other thread completed the same claim decision. Under
load, that let the loser wait behind the live effect lock until its unchanged
five-second fail-closed bound expired. Container and Krun now synchronize after
both claim decisions and before the sole winner starts provider work. This
keeps the intended two-claim/one-effect proof deterministic without changing a
production lock or timeout.

The three failed child-process matrices perform durable recovery work and
launch synchronized child fleets inside one nextest case. A narrow nextest
override gives each of those exact cases all configured test threads. Nextest
therefore runs each without unrelated workspace tests while preserving their
existing semantic checkpoint and outer test timeouts. No retry or timeout was
increased.

The corrected focused lane selected both backend contender tests and all three
process matrices under the default nextest profile. It passed `5/5` with
`2,046` skipped in `54.818s`. The sum of the five case durations was `54.812s`,
which confirms that the three all-thread cases excluded concurrent test load.

## Second Candidate And Test-Contract Correction

The second complete candidate preserved exit `2`. Workspace nextest ran all
`7,405` selected tests in `480.983s`: `7,399` passed, six failed, and `107`
were skipped. The failures identified three test-contract defects:

| Cases | Defect | Bounded correction |
| --- | --- | --- |
| KV two-process contention | The winner exited before the contender classified the same lifetime window, so both processes could report `Won`. | The shared process harness now holds a completed winner until the parent validates both outcomes and sends an explicit `finish` command. |
| Container fresh-process contention matrix | This remaining synchronized process fleet did not have the exact all-thread nextest classification used by its sibling matrices. | Add only this exact case to the existing exclusive all-thread override. |
| Four readiness-probe cases | The accepted test socket inherited nonblocking mode, so bounded reads could return `WouldBlock` before the fixture wrote its response. | Set only the accepted fixture stream to blocking mode before the existing bounded read. |

No production timeout, provider effect, or product behavior changed. The
process-harness suite then passed `13` tests with its two child entrypoints
ignored. The exact six-case nextest lane passed `6/6` with `1,286` skipped in
`1.712s`. Strict affected Clippy passed for `nimbus-process-harness`,
`nimbus-kv`, and `nimbus-sandbox`.

The source change invalidated the compiler authority digest as intended.
Baseline refresh plus the deep and aggregate
checks restored authenticated evidence at six packages, 16 resolved calls,
five generated outputs, and `39/39` conditions.

## Final Candidate

The final `make ci` run exited `0` through the repository single-flight
entrypoint. Its acceptance-bearing summaries were:

- Runtime: `517` passed, `134` declared ignores.
- workspace nextest: `7,405/7,405` passed in `457.346s`, with six slow and two
  leaky classifications visible and `107` tests skipped by the selected lane.
- Rustdoc: all workspace documentation tests passed, including the two
  compile-fail workload-identity cases.
- Required verification harness: storage `1`, engine `1`, and server generated
  histories `3` passed. The server transport campaign passed 12 named cases.
  The runtime liveness campaign passed six named cases.
- JavaScript: all workspace builds and typechecks passed. The UI passed
  `51/51` files and `336/336` tests. All other package self-tests passed.
- proof helpers: tenant lifecycle, PPSC, Elle, external providers, mutation
  committer, runtime tenant isolation (`19/19`), release/package, machine,
  SQLCipher, and installer (`44/44`) helpers passed.

The local lane does not claim hosted coverage uploads or provider campaigns.
It does not claim release artifact builds. It also excludes the Linux package
build that the helper skipped because `nfpm` is not installed. The repository
declares these as hosted or optional evidence.

## Item Review

The configured repository cadence is `pre-pr`, so the requested `item` gate
skipped without contacting a reviewer. The canonical manual invocation then
ran the required item review. The wrapper confirmed GPT-5.6 Sol, xhigh
reasoning, fast service, one 186,490-byte pass, and a clean secret scan.

| Finding | Disposition |
| --- | --- |
| P2: `get_or_insert_mut_ref` returns a lifetime that is not bound to the cache borrow. | Accepted. Backport upstream commit `a615a5b29f21de6dd222394da91ab4e2c6918016`. |
| P3: an error before the new contender barrier can leave the peer blocked. | Accepted. Preserve the claim `Result` through the rendezvous and unwrap it afterward in both backends. |
| P3: the LRU panic regression asserts no unwind or post-panic state. | Accepted. Assert the unwind, length, missing removed key, surviving entries, and exact LRU order. |

All three findings change executable code or its behavioral proof. K9 permits
one narrow correction review after the affected proofs pass.

## Review Correction Evidence

Upstream commit `a615a5b29f21de6dd222394da91ab4e2c6918016` is an exact
one-line fix that binds `get_or_insert_mut_ref` to `&'a mut self`. Nimbus
backports that line and adds a compile-fail proof that rejects a returned
`'static` reference after cache destruction.

The LRU suite passes `53` unit and `44` documentation tests. The panic test now
asserts the unwind and the exact surviving length, keys, values, and LRU order.
Strict library Clippy passes. An exploratory vendored all-target Clippy command
preserved exit `101` for three unchanged upstream test-only lints. It was not
converted to a pass or used as an acceptance gate.

Both corrected sandbox contender tests pass under nextest: `2/2` passed and
`1,257` were skipped by the exact filter. Strict sandbox Clippy and the full
repository `make clippy` gate pass. The compiler baseline refresh and deep
reproduction pass with six packages, 16 resolved calls, and five generated
outputs. The aggregate verifier passes `39/39`.

These affected proofs satisfy the correction rule. They do not replace or
rerun the complete K6 candidate. The one permitted narrow review now owns only
the three accepted corrections and their evidence.

The narrow GPT-5.6 Sol/xhigh/fast correction review reported no finding and
accepted the corrected patch at `0.99`. Review cadence is exhausted. The
commit containing this proof and the NNC9.5 `done` row is the exact K10 item
checkpoint. No push or PR occurred.

## Scope

NNC9.5 owns this proof and concise plan/index routing. A gate failure receives
source diagnosis. The item fixes only a real defect that is in the network
plan's scope. It records an unrelated failure without converting that failure
to a pass.
