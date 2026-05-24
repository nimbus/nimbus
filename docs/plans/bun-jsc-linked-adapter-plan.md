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

- **Status:** active; `BJA4L` symbol isolation is next
- **Primary owner:** this plan
- **Nimbus worktree:** `/Users/jack/src/github.com/nimbus/nimbus`
- **Bun worktree:** `/Users/jack/src/github.com/oven-sh/bun`
- **Nimbus starting baseline:** `40d1a8ea`
  (`Harden Bun runtime diagnostics contract`)
- **Current Bun proof head:** `a409f596e8`
  (`Add Nimbus Bun embed invocation ABI`)
- **Predecessor:** `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`
- **Default posture:** Deno/V8 remains the default runtime; Bun/JSC remains
  optional and fail-closed unless a verified adapter is linked
- **Linux finding:** current static co-linking of V8 and Bun/WebKit archives
  collides on native `simdutf` symbols and is unsafe without symbol isolation

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
| BJA4 | `in_progress` | Execute pure Bun/JSC functions through the Bun pool. | Bun proof head `a409f596e8e1394d8860e2cd8b2bb558ff1afcac` exports `nimbus_bun_embed_invoke_program_wrapper_json` and a release-profile link manifest. Nimbus consumes that manifest only when `NIMBUS_BUN_EMBED_LINK_ARGS` is set, links the Bun/JSC ABI, invokes a self-contained program wrapper with JSON args, receives JSON output, and records pool lifecycle transitions through teardown. No-manifest builds still fail closed. Local macOS evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-39-linked-pure-invocation.md`. Linux evidence found an unsafe static co-link blocker recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-40-linux-static-colink-symbol-collision.md`; BJA4 cannot close until BJA4L passes. |
| BJA4L | `in_progress` | Prove Linux in-process symbol isolation for Bun/JSC beside Deno/V8. | Debian 13 `minicloud` can run the linked pure invocation gate without `--allow-multiple-definition`, without duplicate `simdutf` linker errors, and without runtime crashes. Gate 41 selected a dynamic-library adapter as the first feasibility lane, but Gate 42 proved the current Bun/WebKit release objects cannot be repurposed as a shared object because bmalloc/libpas contain non-PIC TLS relocations. BJA4L2 is now the source-owned static Bun/WebKit namespace-isolation path, preserving Nimbus' single-binary preference while isolating the optional Bun/JSC backend from the default Deno/V8 lane. Evidence must prove both the existing V8 lane and the Bun/JSC lane can coexist in the same product process shape. |
| BJA5 | `pending` | Wire Nimbus HostBridge, tenant identity, and permission grants through Bun/JSC. | Bun/JSC functions can perform explicitly granted HostBridge operations through the same service/engine path as V8, and denied operations fail with audited, tenant-scoped errors. Tests cover allowed operation, denied operation, forged tenant/context attempt, and no raw host token exposure inside Bun/JSC guest code. |
| BJA6 | `pending` | Enforce cancellation, teardown, and memory policy for untrusted Bun/JSC. | Tests cover before-entry cancellation, after-entry sync loop cancellation, promise/microtask progress, in-flight HostBridge cancellation, normal completion, teardown, fresh/discard after each untrusted invocation, and outer memory quota behavior. Evidence passes locally and on `minicloud`. |
| BJA7 | `pending` | Promote Bun/JSC selection into product metadata only after linked execution is real. | Codegen/server artifacts can express function-level Bun selection only when the linked adapter gate is available; no-link builds continue to reject selection clearly. `/debug/runtime/metrics` and the operator UI show `linked` only for linked builds, executor started only after actual invocation, and Bun/JSC memory as `outer_quota_required`. |
| BJA8 | `pending` | Close the plan with reproducible verification, CI, docs, and source ownership. | A reusable gate such as `make verify-bun-jsc-linked-adapter` runs the linked adapter tests, default no-link contract tests, Bun source proof, and UI/API diagnostics checks. The gate passes locally and on Debian 13 `minicloud`. Docs record the final upstream-or-fork source, tag/revision, residual risks, and exact operator/product behavior. The plan is updated to `complete` only after a clean commit records the implementation and evidence. |

## BJA4L Symbol-Isolation Work Plan

BJA4L is the current execution slice. It exists because Linux proved that the
current static same-binary link is not a safe product shape: V8 and Bun/WebKit
both export native `simdutf` symbols, and forcing the linker through with
`--allow-multiple-definition` caused a crash.

Do not start BJA5 until this slice either proves an in-process isolation design
or records that same-binary Bun/JSC is not maintainable.

| Step | Status | Goal | Verifiable success criteria |
| --- | --- | --- | --- |
| BJA4L0 | `done` | Audit the colliding symbol set and owning build artifacts. | Gate 41 records the exact V8/rusty_v8 and Bun/WebKit archives that export `simdutf`, the duplicate C++ and C wrapper symbol families, the link order, and why `--allow-multiple-definition` and similar interposition workarounds are disallowed. Evidence includes `nm`/linker output from `minicloud`: 488 shared global `simdutf::` definitions and 34 shared `simdutf__` wrappers. |
| BJA4L1 | `done` | Choose the least risky in-process isolation strategy. | Gate 41 compared namespacing or hiding `simdutf` in a Nimbus-owned Bun/WebKit fork, namespacing or hiding it in `nimbus/rusty_v8`, and an in-process dynamic-library Bun adapter with hidden/local native symbols plus an explicit Nimbus C ABI. Gate 42 then rejected the current dynamic/PIC lane because the available Bun/WebKit release objects contain non-PIC bmalloc/libpas TLS relocations. The selected BJA4L2 implementation path is now a Nimbus-owned Bun/WebKit namespace-isolation change that keeps the same-binary static link. |
| BJA4L2 | `done` | Implement the smallest source-owned isolation change. | Implement source-owned Bun/WebKit-side namespace isolation for the colliding `simdutf::` family and Bun's `simdutf__` wrappers while preserving the static same-binary link. Gate 43 proves the patched Bun build seam can configure local WebKit with `-Dsimdutf=nimbus_bun_simdutf`, route Bun C++ through `-DBUN_PRIVATE_SIMDUTF_NAMESPACE`, bind Rust FFI to `nimbus_bun_simdutf__*`, and fail closed for prebuilt WebKit or unsupported namespaces on Debian 13 `minicloud`. Gate 44 completes the source-owned Debian build and symbol audit: `libWTF.a` contains 526 `nimbus_bun_simdutf::` definitions and 0 plain `simdutf::` definitions, Bun's wrapper object contains 60 `nimbus_bun_simdutf__*` definitions and 0 plain `simdutf__*` definitions, and Linux V8/rusty_v8 owns no Nimbus Bun namespace symbols. Gate 45 commits and tags the patch in `nimbus/bun` at `bun-v1.4.0-nimbus.1` (`5ba54ccecdfabd857a7ca362c14c0f614d25b21b`). |
| BJA4L3 | `in_progress` | Harden the linked verification gate against unsafe symbol fixes. | `scripts/verify-bun-jsc-linked-adapter.sh` or its `make` wrapper rejects `--allow-multiple-definition`, records the selected source revision, audits exported symbols, and fails if unsafe duplicate symbol families are present. |
| BJA4L4 | `pending` | Prove V8 and Bun/JSC coexist in the same product process shape. | A focused Nimbus test invokes an existing V8-backed lane and the linked Bun/JSC lane in one test binary/process. The test passes locally where supported and on Debian 13 `minicloud` without duplicate-symbol link errors or runtime crashes. |
| BJA4L5 | `pending` | Preserve default DX and enterprise diagnostics. | Default no-link builds still pass `make verify-bun-jsc-runtime-contract`; linked builds report Bun/JSC as `linked`, retain `outer_quota_required`, and record lifecycle acknowledgements. Operator-visible diagnostics remain accurate for V8, Node LTS lanes, and Bun/JSC. |
| BJA4L6 | `pending` | Checkpoint the decision and unblock or stop the plan. | If symbol isolation passes, mark BJA4 and BJA4L done and resume BJA5. If it does not pass, record the blocker and keep Bun/JSC as an external sandbox workload rather than claiming same-binary runtime support. The checkpoint commit includes the implementation, proof docs, and updated plan state. |

### BJA4L Goal Prompt

Complete BJA4L in `docs/plans/bun-jsc-linked-adapter-plan.md`
autonomously. Treat the plan file, proof docs, and focused commits as the
control plane. Do not proceed to BJA5 HostBridge work until a Linux-safe
in-process symbol-isolation strategy is selected, implemented, and verified.

Required BJA4L completion evidence:

- The colliding V8/rusty_v8 and Bun/WebKit symbols are audited with
  reproducible `minicloud` evidence.
- The plan records a selected isolation strategy and why rejected alternatives
  are less appropriate for Nimbus' single-binary and enterprise-trust goals.
- The selected implementation is source-owned by an upstream release/tag or a
  Nimbus fork/tag.
- The linked gate fails closed if `--allow-multiple-definition` or another
  unsafe duplicate-symbol workaround is used.
- A same-process test proves an existing V8 lane and the linked Bun/JSC lane
  can coexist.
- The linked gate passes on Debian 13 `minicloud` using home-backed
  temp/cache paths and without duplicate-symbol linker errors or crashes.
- `make verify-bun-jsc-runtime-contract`, `cargo fmt --all --check`,
  `npm run typecheck`, and `git diff --check` pass before the checkpoint
  commit, unless a command is explicitly recorded as blocked by the unresolved
  symbol-isolation decision.

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

When Bun source changes are part of a batch, run in
`/Users/jack/src/github.com/oven-sh/bun` or the selected Nimbus Bun fork:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=release \
  --build-dir=/private/tmp/nimbus-bun-linked-adapter-release \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
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

- `make verify-bun-jsc-runtime-contract` passes for the default no-link build.
- The linked adapter gate, `make verify-bun-jsc-linked-adapter`, exists and
  passes locally.
- The linked adapter gate or equivalent proof passes on Debian 13 `minicloud`.
- Bun source ownership is reproducible: upstream release/tag or Nimbus-owned
  fork/tag, never an uncommitted local checkout.
- End-to-end Bun/JSC invocation tests cover pure execution, HostBridge allow,
  HostBridge deny, forged tenant/context rejection, cancellation, teardown, and
  diagnostics.
- `cargo fmt --all --check`, `npm run typecheck`, and `git diff --check` pass.
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
