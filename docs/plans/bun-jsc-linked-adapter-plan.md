# Plan: Bun/JSC Linked Adapter And Execution

This plan owns the next product-moving Bun/JSC wave after
`docs/plans/bun-jsc-embedder-api-and-pool-plan.md`.

The target is a linked in-process Bun/JSC runtime backend beside the existing
Deno/V8/Node-compatible backend. This is still not "Bun in a microVM"; Bun in
an OCI image remains a sandbox workload mode. This plan is about making the
existing `RuntimeBackendKind::BunJsc` lane execute through a verified Bun
embedder adapter, a dedicated Bun/JSC pool, Nimbus HostBridge policy, and
fresh/discard lifecycle controls.

## Status

- **Status:** active; `BJA5` HostBridge integration is done; `BJA6`
  cancellation, teardown, and memory policy is next
- **Primary owner:** this plan
- **Nimbus worktree:** `/Users/jack/src/github.com/nimbus/nimbus`
- **Bun worktree:** `/Users/jack/src/github.com/nimbus/bun`
- **Nimbus starting baseline:** `40d1a8ea`
  (`Harden Bun runtime diagnostics contract`)
- **Current Bun proof source:** `nimbus/bun` tag `bun-v1.4.0-nimbus.4`
  (`7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8`)
- **Predecessor:** `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`
- **Default posture:** Deno/V8 remains the default runtime; Bun/JSC remains
  optional and fail-closed unless a verified adapter is linked
- **Linux finding:** the source-owned `nimbus_bun_simdutf` namespace fixed the
  first Bun/WebKit vs V8 collision, but Debian 13 `lld` then exposed additional
  global static co-link collisions across Bun's Rust staticlib, Highway, and
  Bun's V8 shim. Static same-binary linking is not product-safe until the full
  global-symbol set is isolated. Gate 48 selected a source-owned shared
  in-process adapter as the next product path.

## Product Shape

```text
Convex/runtime manifest
  -> RuntimeBackendKind::BunJsc
  -> RuntimePolicy::application_bun_jsc()
  -> RuntimeBackendFactory
  -> BunJscRuntimeBackend
  -> BunJscPool
  -> BunJscExecutionAdapter
  -> Bun/JSC embedder API
  -> Nimbus HostBridge
```

The key seam is the `BunJscExecutionAdapter`. It must be backend-owned and
smaller than the full runtime worker surface. The worker loop should continue
to pass one `RuntimeBackendInvocation`; the Bun adapter should own only:

- VM construction using the proven lockdown profile
- self-contained program-wrapper evaluation
- JSON args/result transport
- HostBridge callback transport and grant checks
- resolver/package denial
- native permission denial/hook propagation
- cancellation, teardown, and fresh/discard lifecycle acknowledgement
- memory/outer-quota enforcement evidence
- adapter-linked diagnostics

## Non-Goals

- Do not make Bun/JSC the default JavaScript runtime.
- Do not share Deno/V8 VM internals with the Bun/JSC pool.
- Do not enable retained untrusted Bun/JSC VM reuse.
- Do not route Bun packages through `node_external_packages`.
- Do not make product correctness depend on an uncommitted local Bun checkout.
- Do not treat a wrapper-level deletion of unsafe globals as sufficient
  containment.
- Do not hide missing Bun embedder APIs behind a feature flag that claims
  product support without passing the execution gates below.

## Fork And Source Rule

The plan starts upstream-first, but the completion gate is stricter than the
research gate:

```text
complete product source
  = upstream Bun release/tag with required APIs
  OR Nimbus-owned Bun fork/tag with the required APIs
```

Local proof commits are acceptable while proving gates, but `BJA8` cannot be
marked complete until Nimbus references a reproducible source. If current Bun
does not expose the required embedder API, create the smallest maintainable
Nimbus-owned Bun fork/tag rather than relying on `~/src/github.com/oven-sh/bun`.

## Execution Gates

| Gate | Status | Goal | Verifiable success criteria |
| --- | --- | --- | --- |
| BJA0 | `done` | Rebaseline the linked-adapter architecture against current Nimbus and Bun. | `docs/plans/proof/runtime-engine/bun-jsc/gate-35-linked-adapter-rebaseline.md` records current Nimbus commit, Bun commit/tag, Bun local dirty state, required Bun-side APIs, whether upstream current Bun is enough, and the exact fork trigger. `git diff --check`, `cargo fmt --all --check`, and `make verify-bun-jsc-runtime-contract` passed. |
| BJA1 | `done` | Introduce a real Bun/JSC execution-adapter boundary while preserving fail-closed defaults. | `crates/nimbus-runtime/src/backends/bun_jsc/adapter.rs` contains the concept-owned adapter trait/factory and default no-link adapter. `RuntimeExecutionAdapterState` is runtime-owned and shared by Convex diagnostics. Default builds keep `execution_adapter_state = "not_linked"` and return the existing contract error. Tests prove default failure, fake-linked dispatch through the seam, lazy lane diagnostics, and no Bun/JSC embedder symbols in the default envelope. Evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-36-linked-adapter-seam.md`. |
| BJA2 | `done` | Prove or implement the Bun-side embedder execution API. | Bun proof head `2f09ba33b1` exposes the native `check-bun-embed-probe` target and configures the StackCheck entry root before constructing the VM outside the Bun CLI path. The target constructs the locked-down VM, executes the self-contained program wrapper, returns JSON, emits resolver/permission/lifecycle evidence, and fails unsafe surfaces. The proof target passed locally and on Debian 13 `minicloud` with home-backed temp/cache paths. Evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-37-embedder-execution-api.md`. |
| BJA3 | `done` | Make Nimbus build with an optional linked Bun/JSC adapter without regressing default builds. | Default `cargo check --workspace` and `make verify-bun-jsc-runtime-contract` still pass without Bun. `nimbus-runtime` now exposes the opt-in `bun-jsc-linked-adapter` feature, which compiles the linked-source adapter contract against Bun proof head `2f09ba33b184a541e2ade24bf6e46bebc971a262` without replacing the default no-link backend. `make verify-bun-jsc-linked-adapter` verifies the default contract, feature tests, exact Bun source revision, required proof exports, Bun format, native `check-bun-embed-probe`, and diff checks. CI/docs name both the default no-link lane and linked proof lane. Evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-38-linked-adapter-build-lane.md`. |
| BJA4 | `done` | Execute pure Bun/JSC functions through the Bun pool. | Gate 39 proved local pure invocation, Gates 40-50 found and fixed the Linux static co-link blockers through the source-owned shared in-process adapter, and Gate 51 reran the full linked adapter verifier on Debian 13 with the HostBridge-capable `.4` source. |
| BJA4L | `done` | Prove Linux in-process symbol isolation for Bun/JSC beside Deno/V8. | Gates 41-48 audited the collision surface and selected the shared in-process adapter. Gates 49-50 proved hidden/local native symbols, no unsafe duplicate-symbol link policy, no `STATIC_TLS`, and same-process V8 plus Bun/JSC coexistence. Gate 51 kept that isolation green after adding HostBridge transport. |
| BJA5 | `done` | Wire Nimbus HostBridge, tenant identity, and permission grants through Bun/JSC. | Bun/JSC functions perform explicitly granted HostBridge operations through the same `HostBridge` seam as V8. Gate 51 proves allowed operation, denied operation, forged tenant/context rejection, and no raw host token exposure inside Bun/JSC guest code locally and on Debian 13 `minicloud`. |
| BJA6 | `pending` | Enforce cancellation, teardown, and memory policy for untrusted Bun/JSC. | Tests cover before-entry cancellation, after-entry sync loop cancellation, promise/microtask progress, in-flight HostBridge cancellation, normal completion, teardown, fresh/discard after each untrusted invocation, and outer memory quota behavior. Evidence passes locally and on `minicloud`. |
| BJA7 | `pending` | Promote Bun/JSC selection into product metadata only after linked execution is real. | Codegen/server artifacts can express function-level Bun selection only when the linked adapter gate is available; no-link builds continue to reject selection clearly. `/debug/runtime/metrics` has a documented or golden-tested contract for lane state, execution adapter state, executor startup, and memory enforcement. The operator UI shows default V8, Node LTS lanes, and Bun/JSC `linked` or `not_linked` state without eager executor startup. |
| BJA8 | `pending` | Close the plan with reproducible verification, CI, docs, and source ownership. | A reusable gate such as `make verify-bun-jsc-linked-adapter` runs the linked adapter tests, default no-link contract tests, Bun source proof, UI/API diagnostics checks, and fail-closed regression checks. The gate passes locally and on Debian 13 `minicloud`. Broader baseline checks pass before completion: `make check`, `make clippy`, `npm run typecheck`, `npm run test`, `npm run build`, docs reference validation when available, and `git diff --check`. Docs record the final upstream-or-fork source, tag/revision, residual risks, and exact operator/product behavior. The plan is updated to `complete` only after focused commits record the implementation and evidence. |

## BJA4L Symbol-Isolation Work Plan

BJA4L is the current execution slice. It exists because Linux proved that the
initial static same-binary link is not a safe product shape. First, V8 and
Bun/WebKit both exported native `simdutf` symbols; forcing that link through
with `--allow-multiple-definition` crashed. After Nimbus fixed that first
family in `nimbus/bun`, `lld` exposed additional hard duplicates across Bun's
Rust staticlib, Highway, and Bun's V8 shim. The product decision is now broader
than `simdutf`: Nimbus needs a complete in-process linkage isolation contract.

Do not start BJA5 until this slice either proves an in-process isolation design
or records that same-binary Bun/JSC is not maintainable.

| Step | Status | Goal | Verifiable success criteria |
| --- | --- | --- | --- |
| BJA4L0 | `done` | Audit the colliding symbol set and owning build artifacts. | Gate 41 records the exact V8/rusty_v8 and Bun/WebKit archives that export `simdutf`, the duplicate C++ and C wrapper symbol families, the link order, and why `--allow-multiple-definition` and similar interposition workarounds are disallowed. Evidence includes `nm`/linker output from `minicloud`: 488 shared global `simdutf::` definitions and 34 shared `simdutf__` wrappers. |
| BJA4L1 | `done` | Choose the least risky first isolation strategy. | Gate 41 compared namespacing or hiding `simdutf` in a Nimbus-owned Bun/WebKit fork, namespacing or hiding it in `nimbus/rusty_v8`, and an in-process dynamic-library Bun adapter with hidden/local native symbols plus an explicit Nimbus C ABI. Gate 42 then rejected repurposing the current non-PIC Bun/WebKit release objects as a shared object. The selected BJA4L2 first implementation path was a Nimbus-owned Bun/WebKit `simdutf` namespace change that keeps the same-binary static link; Gate 47 later proved that first repair is not sufficient for the whole static co-link contract. |
| BJA4L2 | `done` | Implement the smallest source-owned isolation change. | Implement source-owned Bun/WebKit-side namespace isolation for the colliding `simdutf::` family and Bun's `simdutf__` wrappers while preserving the static same-binary link. Gate 43 proves the patched Bun build seam can configure local WebKit with `-Dsimdutf=nimbus_bun_simdutf`, route Bun C++ through `-DBUN_PRIVATE_SIMDUTF_NAMESPACE`, bind Rust FFI to `nimbus_bun_simdutf__*`, and fail closed for prebuilt WebKit or unsupported namespaces on Debian 13 `minicloud`. Gate 44 completes the source-owned Debian build and symbol audit: `libWTF.a` contains 526 `nimbus_bun_simdutf::` definitions and 0 plain `simdutf::` definitions, Bun's wrapper object contains 60 `nimbus_bun_simdutf__*` definitions and 0 plain `simdutf__*` definitions, and Linux V8/rusty_v8 owns no Nimbus Bun namespace symbols. Gate 45 commits and tags the patch in `nimbus/bun` at `bun-v1.4.0-nimbus.1` (`5ba54ccecdfabd857a7ca362c14c0f614d25b21b`). |
| BJA4L3 | `blocked/replanned` | Harden the linked verification gate against unsafe symbol fixes and capture the broader blocker. | `scripts/verify-bun-jsc-linked-adapter.sh` rejects `--allow-multiple-definition`, records the selected source revision, audits `nimbus_bun_simdutf`, and fails on unsafe duplicate-symbol policy. Gate 47 records the Debian 13 `lld` result proving that simdutf isolation alone is incomplete because Rust staticlib personality symbols, Highway, and Bun's V8 shim still collide. |
| BJA4L4 | `done` | Complete the full global co-link collision audit and choose the product linkage shape. | Gate 48 enumerates the known colliding families and owners: Rust staticlib/runtime symbols, Highway, Bun's V8 shim, and simdutf. It compares source-owned PIC/shared loading against deeper static namespace/hiding and selects the shared in-process adapter path. |
| BJA4L5 | `done` | Implement the selected source-owned shared in-process adapter shape. | Gate 49 proves Debian 13 can build `libnimbus_bun_jsc_embedder.so` from the Nimbus Bun fork with PIC WebKit/JSC, PIC Rust staticlib, no unsafe duplicate-symbol workarounds, 10 defined dynamic exports matching only the Nimbus C ABI, and 0 leaked defined native symbols for `v8::`, `hwy::`, Rust personality, or simdutf families. The BJA4L5 source is reproducible from `nimbus/bun` tag `bun-v1.4.0-nimbus.3` at `ed8d05f17ee2803520440a07bcc7f6f47f2f68b8`; this supersedes `.2` after the BJA4L6 static-TLS dlopen failure. |
| BJA4L6 | `done` | Prove V8 and Bun/JSC coexist in the same product process shape. | Gate 50 proves the `.2` shared adapter failed late `dlopen` with ELF `STATIC_TLS`, then proves the `.3` Bun fork tag fixes the shared lane by changing mimalloc to `-ftls-model=local-dynamic` for `--embedder-shared`. The Debian 13 `minicloud` linked gate passed with no unsafe duplicate-symbol policy, no `STATIC_TLS`, exactly 10 Nimbus C ABI exports, 0 leaked defined native symbols, passing simdutf namespace separation, 10 linked unit tests, and 1 same-process integration test that invokes V8 and linked Bun/JSC in one process. |
| BJA4L7 | `done` | Preserve default DX and enterprise diagnostics. | Default no-link builds pass `make verify-bun-jsc-runtime-contract`; linked builds report Bun/JSC as `linked`, retain `outer_quota_required`, and preserve V8, Node LTS, and Bun/JSC operator-visible diagnostics. Gate 51 reran the default contract before linked execution. |
| BJA4L8 | `done` | Checkpoint the decision and unblock or stop the plan. | The source-owned shared adapter passed, BJA4/BJA4L are closed, and BJA5 is complete on top of that shape. Gate 51 records the HostBridge-capable checkpoint and keeps Bun/JSC as an optional in-process backend rather than an external sandbox-only workload. |

### BJA4L Goal Prompt

Complete the replanned BJA4L in
`docs/plans/bun-jsc-linked-adapter-plan.md` autonomously. Treat the plan file,
proof docs, and focused commits as the control plane. Do not proceed to BJA5
HostBridge work until a Linux-safe in-process linkage-isolation strategy is
selected, implemented, and verified against the full global collision set.

Required BJA4L completion evidence:

- The colliding Rust, V8/rusty_v8, Bun/WebKit, Bun V8 shim, Highway, and
  simdutf symbols are audited with reproducible `minicloud` evidence.
- Gate 48 records the selected shared in-process adapter shape and why rejected
  alternatives are less appropriate for Nimbus' simplicity, enterprise trust,
  maintainability, and runtime-isolation goals.
- The selected implementation is source-owned by an upstream release/tag or a
  Nimbus fork/tag.
- The linked gate fails closed if `--allow-multiple-definition`, `muldefs`, or
  another unsafe duplicate-symbol workaround is used.
- A same-process test proves an existing V8 lane and the linked Bun/JSC lane
  can coexist.
- The linked gate passes on Debian 13 `minicloud` using home-backed
  temp/cache paths and without duplicate-symbol linker errors or crashes.
- `make verify-bun-jsc-runtime-contract`, `cargo fmt --all --check`,
  `npm run typecheck`, and `git diff --check` pass before the checkpoint
  commit, unless a command is explicitly recorded as blocked by the unresolved
  symbol-isolation decision.

## Current Completion Plan

This is the active batch order for the existing autonomous goal. It is the
single control-plane checklist for the remaining BJA4L6-BJA8 work. Keep this
table, the gate proof docs, and focused git commits aligned after each status
change.

| Step | Status | Verifiable success criteria |
| --- | --- | --- |
| Record `BJA4L3` blocker | `done` | Gate 47 records the Debian 13 `lld` result: `nimbus_bun_simdutf` removed the first collision family, but static co-linking still fails on Rust staticlib personality symbols, Highway globals, and Bun V8 shim `v8::` symbols. The plan stops treating the current static link as product-ready. |
| Complete `BJA4L4` collision audit and linkage decision | `done` | Gate 48 enumerates all known duplicate families and selects a source-owned PIC/shared in-process Bun adapter with hidden/local symbols. The decision includes rejection criteria, fork/tag ownership, and verifier impact. |
| Complete `BJA4L5` selected implementation | `done` | Gate 49 proves the selected Bun/Nimbus shared adapter build on Debian 13 and the export audit shows exactly the 10 Nimbus ABI symbols with 0 leaked native symbols. The BJA4L5 Bun fork patch is committed and tagged as `bun-v1.4.0-nimbus.3` at `ed8d05f17ee2803520440a07bcc7f6f47f2f68b8`, so source ownership is reproducible for that gate. |
| Complete `BJA4L6` same-process proof | `done` | Gate 50 proves `.3` passes on `minicloud`: generated build graph safety policy passed, the shared object has no `STATIC_TLS`, export/native leak audits passed, simdutf namespace separation passed, 10 linked unit tests passed, and `tests/bun_jsc_linked_adapter.rs` passed the same-process V8 plus Bun/JSC integration test. |
| Checkpoint the BJA4L6 baseline | `done` | Commit `d41edf7e` records only owned Nimbus runtime, plan, proof, and verifier files. Unrelated generated files, screenshots, package-lock churn, and unrelated plans remain unstaged. |
| Run broad baseline gates | `done` | From checkpoint `d41edf7e`, `make check` passed, `make clippy` passed with `-D warnings`, `npm run typecheck` passed, `npm run test` passed including 42 UI test files / 278 UI tests, `npm run build` passed, and `git diff --check` passed. `npm run docs:validate-refs:strict` is unavailable because `package.json` has no such script. |
| Contract runtime diagnostics | `done` | `docs/adapters/native/http-api.md` documents the stable `/debug/runtime/metrics` lane contract. `cargo test -p nimbus-server registry_and_license::runtime_metrics --lib` passed 2 tests that now assert every lane's name, default flag, runtime backend, compatibility target, `execution_adapter_state`, `executor_started`, and memory/tenant-budget enforcement. |
| Update operator diagnostics | `done` | The operator settings UI already displays runtime backend, compatibility target, adapter, executor, and memory columns. `npm run test --workspace packages/nimbus-ui -- src/routes/operator/settings/configuration.spec.tsx src/test/msw.spec.ts` passed 2 files / 5 tests covering default V8, Node 20/22/24, and Bun/JSC `not_linked` + `outer_quota_required`. |
| Add CI or verifier regression gates | `done` | `make verify-bun-jsc-runtime-contract` is wired in CI and now proves lazy executors, no-link fail-closed behavior, memory-policy mismatch rejection, V8/Node lane separation from Bun backend axes, runtime diagnostics, tenant admission, and operator UI diagnostics. The gate passed outside the sandbox: 11 runtime policy tests, 7 Bun/JSC scaffold tests, 13 registry tests, 2 runtime metrics tests, 1 tenant admission test, and 2 UI files / 5 tests. |
| Complete `BJA5` HostBridge integration | `done` | Gate 51 proves Bun/JSC code can perform explicitly granted HostBridge operations through the same `HostBridge` seam as V8. Tests cover allowed operation, denied operation, forged tenant/context rejection, and no raw host token exposure inside guest code. The source-owned Bun fork tag `bun-v1.4.0-nimbus.4` points at `7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8`. |
| Complete `BJA6` lifecycle and memory policy | `pending` | Tests cover before-entry cancellation, after-entry sync-loop cancellation, promise or microtask progress, in-flight HostBridge cancellation, normal completion, teardown, fresh/discard for untrusted invocations, and `outer_quota_required` diagnostics. Evidence passes locally and on `minicloud`. |
| Complete `BJA7` product metadata and diagnostics | `pending` | Codegen/server artifacts express function-level Bun/JSC selection only when the linked adapter gate is available. No-link builds reject selection clearly. `/debug/runtime/metrics` has a documented or golden-tested diagnostics contract, and the operator UI distinguishes V8, Node LTS lanes, and Bun/JSC `linked` or `not_linked` state without eager executor startup. |
| Complete `BJA8` final baseline | `pending` | `make verify-bun-jsc-runtime-contract`, `make verify-bun-jsc-linked-adapter`, Debian `minicloud` linked proof, `make check`, `make clippy`, `npm run typecheck`, `npm run test`, `npm run build`, docs reference validation when available, and `git diff --check` pass. Docs record the final Bun source tag/revision, residual risks, operator behavior, and all gate evidence before marking the plan complete. |

## Enterprise Trust Execution Order

This is the remaining work order under the active BJA0-BJA8 goal. It keeps the
plan product-moving while preserving the default runtime lane and Nimbus'
single-binary default. If Bun/JSC needs a source-owned shared adapter for safe
in-process loading, that adapter remains optional and explicit rather than
silently changing the default runtime envelope.

| Order | Scope | Verifiable success criteria |
| --- | --- | --- |
| 1 | Prove BJA4L6 same-process coexistence. | One focused Nimbus test invokes an existing V8-backed lane and the linked Bun/JSC lane in the same process by loading `libnimbus_bun_jsc_embedder` with local symbol scope. It passes locally where supported and on `minicloud` without duplicate-symbol link errors, `STATIC_TLS` dlopen failures, symbol interposition, crashes, or eager Bun executor startup. |
| 2 | Checkpoint the proven BJA4L6 baseline cleanly. | Commit only owned Nimbus plan/runtime/verifier/proof files after the BJA4L6 proof passes. The Bun fork source change is already committed, tagged, and pushed as `bun-v1.4.0-nimbus.4`. Unrelated generated files, screenshots, package-lock churn, and unrelated plans remain unstaged. |
| 3 | Re-run the broader baseline gates. | `make check`, `make clippy`, `npm run typecheck`, `npm run test`, `npm run build`, docs reference validation when available, and `git diff --check` pass from the checkpoint baseline. Disk-heavy verification uses the repo shared target or home-backed `minicloud` caches rather than `/tmp` or `/private/tmp` sprawl. |
| 4 | Make runtime diagnostics a contract. | `/debug/runtime/metrics` has API documentation or golden tests that lock lane state, `execution_adapter_state`, `executor_started`, and memory enforcement semantics. The contract distinguishes default V8, Node 20/22/24, and Bun/JSC. |
| 5 | Update operator-visible diagnostics. | The operator UI shows the same lane states as the backend: default V8, Node LTS lanes, Bun/JSC `not_linked` or `linked`, and Bun/JSC `outer_quota_required` memory semantics. |
| 6 | Add CI regression gates. | CI or a checked verifier proves Bun/JSC does not eagerly start executors, remains fail-closed when not linked, cannot leak Bun backend axes into V8/Node lanes, and rejects memory-policy mismatches. |
| 7 | Complete BJA5-BJA6 execution hardening. | End-to-end tests cover pure invocation, HostBridge allow, HostBridge deny, forged tenant/context rejection, cancellation, teardown, fresh/discard lifecycle, and memory policy locally and on `minicloud`. |
| 8 | Close BJA8 with broad verification. | `make verify-bun-jsc-runtime-contract`, `make verify-bun-jsc-linked-adapter`, the Debian linked proof, `make check`, `make clippy`, `npm run typecheck`, `npm run test`, `npm run build`, docs reference validation when available, and `git diff --check` pass. The plan and proof docs record the source tag/revision, residual risks, operator behavior, product behavior, and disk/cache hygiene before marking complete. |

## Verification Baseline

Every implementation batch must keep the existing default lane green:

```sh
cargo fmt --all --check
make verify-bun-jsc-runtime-contract
bash scripts/verify-bun-jsc-in-process-lockdown.sh
npm run typecheck
git diff --check
```

Linked-adapter batches must add a stronger gate before `BJA8` can close:

```sh
make verify-bun-jsc-linked-adapter
```

That target should prove, at minimum:

- default no-link build remains fail-closed
- linked build reports Bun/JSC as `linked`
- pure Bun/JSC invocation succeeds end to end
- HostBridge grants are enforced per tenant/workload
- forged tenant identity is rejected
- cancellation and teardown are deterministic
- outer-quota memory policy is visible in diagnostics
- V8/Node lanes do not inherit Bun/JSC backend axes
- Linux symbol isolation is explicit; the gate must not use
  `--allow-multiple-definition` to paper over V8/Bun native symbol collisions
- Linux shared-adapter artifacts are late-`dlopen` safe; the generated build
  graph must not use static TLS models and the ELF dynamic section must not
  contain `STATIC_TLS`

When Bun source changes are part of a batch, run in the selected Nimbus Bun
fork, currently `/Users/jack/src/github.com/nimbus/bun`:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=release \
  --webkit=local \
  --embedder-shared=on \
  --build-dir=/private/tmp/nimbus-bun-shared-adapter-release \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-shared
git diff --check
```

For `minicloud`, avoid `/tmp` because it has filled during prior proof runs.
Use home-backed paths such as:

```sh
TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp
NIMBUS_BUN_BUILD_DIR=$HOME/.cache/nimbus-bun-proof/embed-native
NIMBUS_BUN_CACHE_DIR=$HOME/.cache/nimbus-bun-proof/cache
NIMBUS_BUN_CARGO_TARGET_DIR=$HOME/.cache/nimbus-bun-proof/cargo-target
```

Noninteractive `minicloud` sessions also need the user-local tool paths:

```sh
PATH=$HOME/.cargo/bin:$HOME/.bun/bin:$PATH
```

## Autonomous Goal Prompt

Complete `docs/plans/bun-jsc-linked-adapter-plan.md` from `BJA0` through
`BJA8` autonomously. Treat the plan file and git history as the control plane:
update gate status and evidence as work progresses, keep unrelated dirty files
out of commits, and do not mark the plan complete until every verifiable
success criterion passes.

Required completion evidence:

- Focused commits checkpoint the baseline and exclude unrelated local changes.
- The execution order is followed: BJA4L6 same-process coexistence proof,
  clean baseline checkpoint, broad baseline gates, diagnostics contract,
  operator-visible diagnostics, CI regression gates, BJA5-BJA6 execution
  hardening, and final BJA8 verification.
- `make verify-bun-jsc-runtime-contract` passes for the default no-link build.
- The linked adapter gate, `make verify-bun-jsc-linked-adapter`, exists and
  passes locally.
- The linked adapter gate or equivalent proof passes on Debian 13 `minicloud`.
- Bun source ownership is reproducible: upstream release/tag or Nimbus-owned
  fork/tag, never an uncommitted local checkout. For this wave the expected
  source is `nimbus/bun` tag `bun-v1.4.0-nimbus.4` at
  `7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8`.
- Disk/cache hygiene is preserved during verification: avoid `/tmp` on
  `minicloud`, prefer home-backed proof paths there, and do not create new
  throwaway checkouts when canonical worktrees already exist.
- `/debug/runtime/metrics` diagnostics are documented or golden-tested so lane
  state, `execution_adapter_state`, `executor_started`, and memory enforcement
  cannot drift quietly.
- The operator UI displays default V8, Node 20/22/24 lanes, Bun/JSC
  `not_linked` or `linked`, and Bun/JSC `outer_quota_required` memory
  semantics consistently with the backend.
- End-to-end Bun/JSC invocation tests cover pure execution, HostBridge allow,
  HostBridge deny, forged tenant/context rejection, cancellation, teardown, and
  diagnostics.
- CI or verifier checks prove Bun/JSC does not eagerly start executors,
  remains fail-closed when the adapter is not linked, rejects memory policy
  mismatches, and does not leak Bun backend axes into V8/Node lanes.
- `make check`, `make clippy`, `npm run typecheck`, `npm run test`,
  `npm run build`, docs reference validation when available, and
  `git diff --check` pass.
- The implementation and plan evidence are committed in focused commits.

## Progress Log

| Date | Gate | Status | Notes | Verification | Next |
| --- | --- | --- | --- | --- | --- |
| 2026-05-23 | BJA0 | `done` | Added Gate 35, the linked-adapter rebaseline. Current Nimbus is `9b575308`; local Bun proof head is `4b5de5ee5d`, clean and 16 commits ahead of upstream `origin/main` at `f161e0311d`. Current upstream Bun is not sufficient for product source ownership because the embed probe, resolver denial, native permission denial, host-call, lifecycle, and cancellation proof surfaces exist only in the local proof delta. The fork trigger for this wave is product dependency ownership, not merely the existence of local proof commits. | `cargo fmt --all --check` passed; `make verify-bun-jsc-runtime-contract` passed outside the Codex filesystem sandbox: 11 runtime policy tests, 4 Bun/JSC pool scaffold tests, 13 Convex registry tests, 2 runtime diagnostics tests, 1 tenant-admission test, and 2 UI test files / 5 tests; `git diff --check` passed. | Start BJA1 by introducing the explicit `BunJscExecutionAdapter` boundary while preserving default no-link behavior. |
| 2026-05-24 | BJA1 | `done` | Added `BunJscExecutionAdapterFactory`, `BunJscExecutionAdapter`, and the default `BunJscNoLinkExecutionAdapter` under `crates/nimbus-runtime/src/backends/bun_jsc/adapter.rs`. `BunJscRuntimeBackend` now dispatches through that seam, records disabled invocations only for `not_linked`, and has fake-linked test coverage proving the future linked adapter gets the invocation after policy/scaffold checks. `RuntimeExecutionAdapterState` moved to the runtime crate so Convex diagnostics and HTTP metadata share the same state vocabulary. | `make verify-bun-jsc-runtime-contract` passed outside the Codex filesystem sandbox: 11 runtime policy tests, 7 Bun/JSC pool/adapter seam tests, 13 Convex registry tests, 2 runtime diagnostics tests, 1 tenant-admission test, and 2 UI test files / 5 tests. `cargo test -p nimbus-server registry_and_license::registry --lib` passed 13 tests; `cargo test -p nimbus-server registry_and_license::runtime_metrics --lib` passed 2 tests; `cargo fmt --all --check` passed; `git diff --check` passed. | Start BJA2 by proving or productizing the Bun-side embedder execution API behind this linked-adapter seam. |
| 2026-05-24 | BJA2 | `done` | Bun proof head advanced to `2f09ba33b1` with `bun_core::StackCheck::configure_thread()` in the native embed probe root, matching the CLI entry-root requirement before `VirtualMachine` construction. The `check-bun-embed-probe` target now proves the Bun-side execution API shape needed by Nimbus: locked-down VM construction, generated program-wrapper evaluation, JSON result transport, resolver/package denial, native permission denial, cancellation recovery, memory pressure evidence, and fresh/discard lifecycle policy. | In `/Users/jack/src/github.com/oven-sh/bun`, `cargo fmt --all --check`, `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe`, and `git diff --check` passed. On Debian 13 `minicloud` at `/home/nimbus/src/github.com/oven-sh/bun`, the same proof passed with `clang-21`, `ld.lld-21`, `TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp`, `CARGO_TARGET_DIR=$HOME/.cache/nimbus-bun-proof/cargo-target`, build dir `$HOME/.cache/nimbus-bun-proof/embed-native`, and cache dir `$HOME/.cache/nimbus-bun-proof/cache`. | Start BJA3 by wiring an optional linked adapter build path in Nimbus while preserving the default no-link build and diagnostics contract. |
| 2026-05-24 | BJA3 | `done` | Added the opt-in `nimbus-runtime/bun-jsc-linked-adapter` feature, `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs`, and `make verify-bun-jsc-linked-adapter`. The feature compiles the linked-source adapter contract and required Bun proof export declarations, but default `BunJscRuntimeBackendFactory` still constructs the no-link adapter and reports `not_linked`. The feature-owned adapter factory is guarded with a BJA4 execution error so no product build can silently claim working Bun/JSC execution before end-to-end invocation lands. CI syntax-checks the linked proof script and docs now name the default no-link lane plus the opt-in linked proof lane. | `cargo test -p nimbus-runtime --lib backends::bun_jsc` passed 7 tests. `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc` passed 9 tests. `make verify-bun-jsc-linked-adapter` passed outside the Codex filesystem sandbox: default runtime contract, linked feature tests, exact Bun source revision, nine required proof exports, Bun format, native `check-bun-embed-probe`, Nimbus diff check, and Bun diff check. `cargo check --workspace` passed. | Start BJA4 by wiring the first real pure-function Bun/JSC invocation path through the Bun pool while keeping the old adapter-not-linked error for no-link builds. |
| 2026-05-24 | BJA4 | `local-done; linux-needs-symbol-isolation` | Bun proof head advanced to `a409f596e8` with the `nimbus_bun_embed_invoke_program_wrapper_json` ABI and release-profile link-manifest generation. Nimbus now compiles a manifest-backed `nimbus_bun_jsc_linked_ffi` variant, requires the Bun static-archive symbol explicitly, links the platform C++ runtime for Rust's `-nodefaultlibs` link, validates bundle integrity/content kind, serializes `InvocationRequest`, calls Bun/JSC, parses JSON output, and records linked pool lifecycle through teardown. Debian 13 `minicloud` proved the current static co-link is unsafe because V8 and Bun/WebKit both export `simdutf` symbols. | In `/Users/jack/src/github.com/oven-sh/bun`, `cargo fmt --all --check`, `git diff --check`, and `CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target-release bun scripts/build.ts --profile=release --build-dir=/private/tmp/nimbus-bun-linked-adapter-release --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe` passed. In Nimbus, the focused linked invocation test passed 1 test. `bash scripts/verify-bun-jsc-linked-adapter.sh` passed locally: default no-link contract, 10 no-manifest linked tests, exact Bun revision/export checks, release native probe, linked pure invocation, and diff checks. On `minicloud`, the gate passed default no-link contract, linked no-manifest tests, export checks, Bun format, and native probe, then failed linking the Bun/JSC FFI test with duplicate `simdutf` symbols from `libv8-*.rlib` and `libWTF.a`. A diagnostic `--allow-multiple-definition` retry linked but crashed with `SIGSEGV`, so that workaround is rejected. | Add and pass BJA4L: a Linux symbol-isolated in-process Bun/JSC adapter proof. Do not start BJA5 HostBridge product work on top of unsafe static co-linking. |
| 2026-05-24 | BJA4L0-BJA4L1 | `done` | Gate 41 audited the static co-link failure on `minicloud`: V8/rusty_v8 `libv8-60dc74d54503132f.rlib` member `simdutf.o` and Bun/WebKit `libWTF.a` member `SIMDUTF.cpp.o` share 488 global `simdutf::` definitions, and Rusty V8 plus Bun also share 34 `simdutf__` C wrapper definitions. The first selected proof path was an in-process dynamic Bun adapter that exposes only `nimbus_bun_embed_*` and hides WebKit/Bun native symbols, gated by a PIC-capable artifact proof. | `minicloud` audit recorded link manifest line count, archive owners, representative linker diagnostics, `nm -C` symbol counts, shared wrapper names, current non-PIC Bun/WebKit build flags, and the fact that 13 manifest objects/archives have undefined `simdutf::` references. `scripts/verify-bun-jsc-linked-adapter.sh` now rejects `--allow-multiple-definition`, `-z muldefs`, and `-z,muldefs` in Rust flags before running the gate. | Prove or reject the dynamic/PIC lane before committing to BJA4L2 implementation. |
| 2026-05-24 | BJA4L2-feasibility | `done; dynamic lane rejected` | Gate 42 attempted to repurpose the current Bun release-profile static embed manifest as a dynamic artifact after removing obvious executable-only flags. `clang++-21 -shared` failed with `R_X86_64_TPOFF32` relocations from WebKit's bmalloc/libpas TLS state, including `pas_thread_local_cache_pointer` and `pas_thread_local_cache_is_exiting`. This proves the current release objects cannot support the dynamic adapter without Nimbus owning a PIC WebKit/Bun artifact. | Debian 13 `minicloud` command in Gate 42 used `/home/nimbus/.cache/nimbus-bun-proof` and failed with status `1`, with the first actionable diagnostics recorded from `dynamic-feasibility.log`. | Start BJA4L2 by implementing source-owned Bun/WebKit namespace isolation for `simdutf::` and Bun's `simdutf__` wrappers while preserving the static same-binary link. |
| 2026-05-24 | BJA4L2-source-namespace | `in_progress` | Gate 43 patched Bun proof head `a409f596e8` to add `--simdutf-namespace=nimbus_bun_simdutf`. The patch wires local WebKit CMake with `-Dsimdutf=nimbus_bun_simdutf`, Bun C++ with `-DBUN_PRIVATE_SIMDUTF_NAMESPACE -Dsimdutf=nimbus_bun_simdutf`, and Rust FFI with `--cfg=bun_private_simdutf_namespace` so Bun's wrapper ABI binds as `nimbus_bun_simdutf__*`. Guards reject prebuilt WebKit, invalid identifiers, and unsupported namespaces. | Local Bun `git diff --check` and `cargo fmt --all --check` passed. On Debian 13 `minicloud`, patch application, `git diff --check`, `cargo fmt --all --check`, configure-only, generated-build flag audit, and fail-closed guard probes passed. A narrow `ninja ... obj/src/simdutf_sys/bun-simdutf.cpp.o` proof pulled in static local WebKit/JSC, installed normal host prerequisites (`ruby`, `libicu-dev`), configured WebKit successfully, and compiled JSC/WTF sources with `-Dsimdutf=nimbus_bun_simdutf`; it was intentionally stopped after about 19 minutes with partial cache preserved. | Complete a dedicated source-build gate, audit actual archives/objects for namespaced Bun/WebKit symbols, make the source revision reproducible, and only then update the linked-adapter verification script and same-process V8+Bun/JSC link gate. |
| 2026-05-24 | BJA4L2-source-build-symbol-audit | `build-and-symbol-proof-done; source-ownership-open` | Gate 44 completed the dedicated source-owned Debian build on `minicloud`: local WebKit linked `libJavaScriptCore.a`, Bun linked and ran `bun-embed-probe`, and the embedder proof still passed cancellation, permission, package/resolver, memory, and lifecycle probes. The built artifacts prove the namespace repair is in the actual static archives/objects, not only in generated flags. | `ninja -C $HOME/.cache/nimbus-bun-proof/configure-namespaced -j4 check-bun-embed-probe` passed with `status=0`. Symbol audit found `libWTF.a`: 526 `nimbus_bun_simdutf::` definitions and 0 plain `simdutf::`; `libJavaScriptCore.a`: 0 for both families; `bun-simdutf.cpp.o`: 60 `nimbus_bun_simdutf__*` definitions and 0 plain `simdutf__*`. Remote Linux V8/rusty_v8 artifacts still contain plain V8-side `simdutf::` and `simdutf__` definitions but 0 Nimbus Bun namespace symbols. | Make the Bun namespace patch reproducible from a committed/tagged source revision, then promote the proof into the linked verification script and same-process V8+Bun/JSC gate. |
| 2026-05-24 | BJA4L2-source-ownership | `done` | Gate 45 created the Nimbus-owned Bun source checkpoint at `https://github.com/nimbus/bun`. The branch `nimbus/bja4l2-simdutf-namespace` and tag `bun-v1.4.0-nimbus.1` both resolve to `5ba54ccecdfabd857a7ca362c14c0f614d25b21b`. The local source lives at `/Users/jack/src/github.com/nimbus/bun` as a disk-saving Git worktree. | In `/Users/jack/src/github.com/nimbus/bun`, `git diff --check` and `cargo fmt --all --check` passed before commit. `git ls-remote nimbus refs/heads/nimbus/bja4l2-simdutf-namespace refs/tags/bun-v1.4.0-nimbus.1` returned the expected revision for both refs. | Start BJA4L3 by making the linked verifier consume `nimbus/bun` `bun-v1.4.0-nimbus.1`, audit namespaced symbols, and fail closed on unsafe link workarounds. |
| 2026-05-24 | BJA4L3-linked-verifier-hardening | `local-done; linux-pending` | Gate 46 updates the linked verifier to consume `nimbus/bun` `bun-v1.4.0-nimbus.1`, checks the exact source ref and commit, defaults Linux to `release-local` plus `nimbus_bun_simdutf`, requires Linux symbol audit, and rejects unsafe duplicate-symbol link workarounds in both shell env flags and `nimbus-runtime` build manifests. | `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc` passed 10 tests. Local `bash scripts/verify-bun-jsc-linked-adapter.sh` passed on macOS with the release/prebuilt path and skipped the Linux-only symbol audit. Negative probes rejected `RUSTFLAGS='-Wl,--allow-multiple-definition'` and a manifest containing `-Wl,--allow-multiple-definition`. `bash -n`, `cargo fmt --all --check`, and `git diff --check` passed. | Run the linked verifier on Debian 13 `minicloud`, where it must use `release-local`, build from local WebKit, require the symbol audit, and pass without unsafe linker policy. |
| 2026-05-24 | BJA4L3-global-static-colink-collision | `blocked/replanned` | Gate 47 records the Debian 13 `lld` result after the `nimbus_bun_simdutf` source fix: static co-linking still fails because the Bun embedder lane also brings duplicate Rust staticlib/runtime symbols, Highway symbols, and Bun V8 shim `v8::` symbols into the same Rust/V8 product process. The current static link is therefore not product-ready, and BJA4L is replanned around a complete global-symbol isolation contract. | The focused same-process proof was rerun on `minicloud` with `clang++-21`, `-fuse-ld=lld`, `CARGO_BUILD_JOBS=1`, the home-backed `configure-namespaced` manifest, and shared Nimbus target cache. It failed with duplicate-symbol diagnostics for `rust_eh_personality`, `hwy::platform::GetCpuString`, `hwy::DisableTargets`, and Bun V8 shim symbols such as `v8::Array::New` and `v8::Boolean::Value`. | Start BJA4L4: complete the full collision audit and choose between a source-owned PIC/shared in-process Bun adapter with hidden/local symbols or a deeper source-owned static namespace/hiding strategy. |
| 2026-05-24 | BJA4L4-global-collision-audit-and-linkage-decision | `done` | Gate 48 audits the broader Linux collision surface: `libbun_embed_probe.a` owns `rust_eh_personality`; V8/rusty_v8 artifacts own 30 `hwy::` definitions while Bun owns 39 Highway definitions; Bun's two unified V8 shim objects own 170 `v8::` definitions; and `nimbus_bun_simdutf` remains correctly isolated. | Read-only `minicloud` artifact audit inspected `/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/nimbus-bun-embed-link-args.txt`, `libbun_embed_probe.a`, and Nimbus V8 artifacts under `/home/nimbus/src/github.com/nimbus/nimbus/target`. | Start BJA4L5 by adding a source-owned PIC/shared Bun/JSC embedder adapter that exports only the Nimbus C ABI and is loaded in-process with local symbol scope. |
| 2026-05-24 | BJA4L5-shared-adapter-build-and-export-audit | `done` | Gate 49 proves the selected shared in-process adapter can build on Debian 13 from the Nimbus Bun fork. The proof fixed two concrete source issues: Rust needed `-Crelocation-model=pic` for `--embedder-shared`, and the shared adapter target needed to drop `--exclude-libs,ALL` so the version script could export the explicit Nimbus ABI. The Bun fork patch is committed, tagged, and pushed as `bun-v1.4.0-nimbus.2` at `c0896b441c89402c8af0ade847f806f2fcc5fece`. | `ninja -C /home/nimbus/.cache/nimbus-bun-proof/shared-adapter-configure -j4 check-bun-embed-shared` passed. `libnimbus_bun_jsc_embedder.so` has SONAME `libnimbus_bun_jsc_embedder.so`, `BIND_NOW`, no reported `TEXTREL`, exactly 10 defined dynamic exports under `NIMBUS_BUN_JSC_EMBEDDER_1.0`, and 0 leaked defined native symbols for `v8::`, `hwy::`, Rust personality, or simdutf families. `git ls-remote nimbus refs/heads/nimbus/bja4l2-simdutf-namespace refs/tags/bun-v1.4.0-nimbus.2^{}` resolved the branch and peeled tag to the expected revision. | Implement the Nimbus dynamic-loader seam and BJA4L6 same-process V8 plus Bun/JSC coexistence proof. |
| 2026-05-24 | BJA4L6-static-tls-dlopen-proof | `done` | The first shared-adapter `minicloud` rerun proved the export/symbol audit but failed the same-process lane because `dlopen` returned `cannot allocate memory in static TLS block`. The produced `.so` had ELF `STATIC_TLS`, traced to Bun's direct mimalloc build adding `-ftls-model=initial-exec` even for `--embedder-shared`. The Bun fork now has `bun-v1.4.0-nimbus.3` at `ed8d05f17ee2803520440a07bcc7f6f47f2f68b8`, changing the shared embedder lane to use `-ftls-model=local-dynamic` for mimalloc while preserving `initial-exec` for Bun's normal static executable. | Passing `.3` gate evidence on Debian 13 `minicloud`: default no-link runtime contract passed; linked no-shared-library unit contract passed 10 tests; Bun source exports and Rust format passed; generated build graph safety passed; shared adapter export audit found exactly 10 Nimbus ABI exports and 0 leaked defined native symbols; ELF audit rejected no `STATIC_TLS`; simdutf namespace audit passed; linked same-process unit lane passed 10 tests; `tests/bun_jsc_linked_adapter.rs` passed 1 integration test; Nimbus and Bun whitespace diff checks passed. Local `bash -n scripts/verify-bun-jsc-linked-adapter.sh`, `cargo fmt --all --check`, and `git diff --check` passed after updating the verifier/source contract. | Checkpoint the BJA4L6 baseline cleanly, then run broad baseline gates before diagnostics/UI/CI regression work. |
| 2026-05-24 | BJA4L6-checkpoint-and-broad-gates | `done` | Checkpoint commit `d41edf7e` records the Bun/JSC shared-adapter dynamic loader, same-process V8 plus Bun/JSC integration proof, verifier hardening, and Gate 47-50 proof docs. Unrelated generated Convex files, `package-lock.json`, screenshots, and unrelated plan files remain outside the checkpoint. | `make check` passed in 36.08s. `make clippy` passed with `-D warnings` in 26.94s. `npm run typecheck` passed with existing route-helper warnings. `npm run test` passed, including 42 UI test files and 278 UI tests. `npm run build` passed with existing route-helper warnings and a Vite chunk-size warning. `npm run docs:validate-refs:strict` is unavailable because no such npm script exists. `git diff --check` passed. | Contract `/debug/runtime/metrics` diagnostics so lane state, `execution_adapter_state`, `executor_started`, and memory semantics cannot drift quietly. |
| 2026-05-24 | BJA7-diagnostics-contract-and-operator-ui | `done` | Tightened the runtime diagnostics contract and operator UI assertions around the lane model. The API doc now names the stable default, Node 20/22/24, and Bun/JSC lanes with backend, compatibility target, adapter state, executor laziness, and memory enforcement. The server test now asserts the full lane contract instead of only spot-checking default and Bun/JSC. The operator UI test and MSW fixture test assert the same lane matrix. | `cargo test -p nimbus-server registry_and_license::runtime_metrics --lib` passed 2 tests. `cargo fmt --all --check` passed after formatting the server test. `npm run test --workspace packages/nimbus-ui -- src/routes/operator/settings/configuration.spec.tsx src/test/msw.spec.ts` passed 2 files / 5 tests. `git diff --check` passed before the diagnostics changes and will be rerun before checkpoint. | Add CI/verifier regression gates for no eager Bun executor startup, no-link fail-closed behavior, memory-policy mismatch rejection, and no Bun backend-axis leakage into V8/Node lanes. |
| 2026-05-24 | BJA7-ci-regression-gate | `done` | The fast CI contract lane now covers the enterprise regressions for this wave: Bun/JSC executors stay lazy, no-link builds fail closed, memory policy mismatches are rejected, Bun backend axes do not leak into V8/Node lanes, runtime metrics expose the full lane contract, tenant admission permits only the proven Bun/JSC profile, and the operator UI renders the same diagnostics. | First sandboxed run failed at the HTTP fixture with local listener `Operation not permitted`, so it was rerun outside the sandbox. `make verify-bun-jsc-runtime-contract` then passed: 11 runtime policy tests, 7 Bun/JSC scaffold tests, 13 registry tests, 2 runtime metrics tests, 1 tenant admission test, and 2 UI files / 5 tests. | Start BJA5 HostBridge integration through the linked Bun/JSC adapter. |
| 2026-05-24 | BJA5-hostbridge-linked-adapter | `done` | Bun fork tag `bun-v1.4.0-nimbus.4` adds the HostBridge-capable embedder entrypoint `nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge`. Nimbus loads that ABI, passes a synchronous C callback backed by the existing `HostBridge`, and keeps the pure ABI present for source-contract drift detection. | `cargo check -p nimbus-runtime` passed locally. `cargo fmt --all --check` and `git diff --check` passed locally. On Debian 13 `minicloud`, the warmed `bash scripts/verify-bun-jsc-linked-adapter.sh` gate passed against `7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8`: default no-link contract, linked no-shared-library unit contract, 11 Bun exports, Bun Rust format with no owned embed-probe deprecation warnings, shared adapter build/export/leak/simdutf audit, 10 linked unit tests, 4 linked integration tests, and Nimbus/Bun whitespace diff checks. | Start BJA6 cancellation, teardown, and memory policy hardening. |
