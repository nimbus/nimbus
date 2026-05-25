# Nimbus Fork Upstream Standardization Plan

Status: in progress
Owner: Nimbus runtime and distribution work
Created: 2026-05-25
Final review: 2026-05-25

## Purpose

Nimbus depends on a small set of source forks that are now part of the product
surface, not just local proof work. Those forks need one consistent operating
model so a clean machine, CI, release automation, and an enterprise customer can
answer the same questions:

- Which upstream release is this fork based on?
- Which Nimbus patches are applied?
- Which branch is active?
- Which tag should consumers pin?
- Which remote is upstream and which remote is the Nimbus fork?
- Which local checkout is canonical?

This plan standardizes the local layout, Git remotes, branch names, tags,
release pairings, installer pins, and verification evidence for all Nimbus-owned
forks currently needed by runtime and sandbox distribution.

## Sources Verified

- Deno latest release verified during final review: `v2.8.0`, released
  2026-05-22.
  <https://github.com/denoland/deno/releases/tag/v2.8.0>
- Deno `v2.8.0` declares `v8 = "149.0.0"` in its workspace manifest.
  <https://raw.githubusercontent.com/denoland/deno/v2.8.0/Cargo.toml>
- Bun latest GitHub release verified during final review: `v1.3.14`.
  <https://github.com/oven-sh/bun/releases>
- crun latest GitHub release verified during final review: `1.27.1`.
  <https://github.com/containers/crun/releases/tag/1.27.1>
- libkrun latest upstream tag verified during final review: `v1.18.1`.
  <https://github.com/containers/libkrun/releases>
- rusty_v8 upstream teardown fix merged 2026-05-25:
  `e5abf2b25ab2faf784ee40dd1b25f15c489f8c0b`.
  <https://github.com/denoland/rusty_v8/commit/e5abf2b25ab2faf784ee40dd1b25f15c489f8c0b>

## Scope

In scope:

- `/Users/jack/src/github.com/nimbus/deno`
- `/Users/jack/src/github.com/nimbus/rusty_v8`
- `/Users/jack/src/github.com/nimbus/bun`
- `/Users/jack/src/github.com/nimbus/nimbus-crun`
- `/Users/jack/src/github.com/nimbus/nimbus-libkrun`
- Nimbus repo pins, installer/package defaults, release helper scripts,
  operator docs, and active plan references that consume those forks.

Out of scope:

- Rewriting archived proof logs or completed historical plans.
- Deleting historical `locker` tags or branches.
- Making upstream pull requests.
- Depending on upstream fork metadata through GitHub's fork button. Nimbus forks
  are source-owned mirrors with explicit remotes, branches, tags, and releases.
- Forking first-party Nimbus repos such as `machine-os`, `desktop`, or
  `homebrew-tap`; those may have related release references, but they are not
  upstream source forks.

## Standard Contract

### Local Paths

Canonical fork checkouts live under:

```text
/Users/jack/src/github.com/nimbus/<repo>
```

Canonical upstream checkouts, when useful for local comparison, live under:

```text
/Users/jack/src/github.com/<upstream-org>/<upstream-repo>
```

Temporary proof work may use worktrees, but the source of truth is the canonical
fork checkout plus pushed Nimbus tags.

### Remotes

Every fork repo must use:

```text
origin    git@github.com:nimbus/<repo>.git
upstream  git@github.com:<upstream-org>/<upstream-repo>.git
```

Optional local comparison remotes may use:

```text
local-<upstream-org>  /Users/jack/src/github.com/<upstream-org>/<upstream-repo>
```

Do not use a remote named `nimbus` for the Nimbus fork. `origin` is always the
Nimbus fork. `upstream` is always the upstream project.

### Branch Names

Active Nimbus maintenance branches are named from the upstream source tag:

```text
nimbus/<upstream-source-tag>
```

Examples:

- `nimbus/v2.8.0`
- `nimbus/v149.0.0`
- `nimbus/bun-v1.3.14`
- `nimbus/1.27.1`
- `nimbus/v1.18.1`

No new active branch should use `locker`.

### Tag Names

Nimbus release tags append `-nimbus.N` to a normalized upstream release name:

```text
<normalized-upstream-release>-nimbus.N
```

Rules:

- Upstream `vX.Y.Z` tags become `vX.Y.Z-nimbus.N`.
- Upstream semver-only tags such as crun `1.27.1` become
  `v1.27.1-nimbus.N`, while the release notes must record the exact upstream
  source tag `1.27.1`.
- Product-prefixed upstream tags such as Bun `bun-v1.3.14` become
  `bun-v1.3.14-nimbus.N`.
- `N` is monotonically increasing for the same upstream source release.
- Historical `*-locker.*` tags remain immutable evidence, but no new product
  pins should use them after this plan completes.

### Default Branches

Source forks should default to the active Nimbus maintenance branch. If a fork
temporarily keeps `main` as default for release automation, the exception must
be documented in the fork README and the active branch must still exist.

The target steady state is:

```text
origin/HEAD -> nimbus/<upstream-source-tag>
```

### Pairing Rules

Deno and rusty_v8 move as one release family:

- Pick the Deno upstream release first.
- Read the `v8` crate version from that Deno release's `Cargo.toml`.
- Base the Nimbus rusty_v8 fork on that exact rusty_v8 release, not on the
  newest independent rusty_v8 tag.
- Patch Deno to consume the matching Nimbus rusty_v8 tag.
- Patch Nimbus to consume the matching Nimbus Deno and rusty_v8 tags.

crun and libkrun move as the sandbox runtime family:

- `nimbus-crun` must build against the selected `nimbus-libkrun` release.
- Installers and Linux packages must agree on both tags.
- The Linux package workflow must download the selected Nimbus releases, not
  upstream artifacts.

Bun is a runtime backend family:

- Bun source forks must not pretend that a mainline proof commit is an
  upstream release.
- If the latest official Bun release contains the required embedder APIs, rebase
  to that release and tag it with the standard `bun-vX.Y.Z-nimbus.N` form.
- If the required APIs exist only on upstream `main`, keep the branch as a
  documented proof exception and do not present it as a release-standard fork
  until an official upstream release contains the needed base.

## Baseline Inventory Notes

This table records the fork state that motivated each plan row as it was
written or updated. It is intentionally historical once an execution item
completes. The live state of the forks is owned by
`scripts/verify-fork-upstream-standardization.sh` and the per-item evidence
below.

| Fork | Current local branch/tag | Current remote issue | Upstream release target | Required action |
| --- | --- | --- | --- | --- |
| `nimbus/deno` | `locker-v2.7.14`, `v2.7.14-locker.43` | Uses `locker` branch/tag names | `v2.8.0` | Rebase/port Nimbus deltas to `nimbus/v2.8.0`, tag `v2.8.0-nimbus.2`, update Nimbus pins |
| `nimbus/rusty_v8` | `locker-v147.4.0`, `v147.4.0-locker.7` | Uses `locker` branch/tag names | Deno-required `v149.0.0` | Rebase/port Nimbus deltas to `nimbus/v149.0.0`, include/evaluate upstream teardown fix `e5abf2b`, tag `v149.0.0-nimbus.1` |
| `nimbus/bun` | `nimbus/bun-main-20260525`, `nimbus-bun-jsc-proof-main-20260525` | Was using a release-looking proof tag before FUS3 | Latest official release observed: `bun-v1.3.14` | Keep Bun as mainline-proof until an official upstream release contains the required Rust/embedder surfaces |
| `nimbus/nimbus-crun` | `main`, `v1.27.1-nimbus.1` | Missing `upstream` remote | `1.27.1` | Add upstream remote, create active `nimbus/1.27.1` branch or document `main` exception, verify package consumers |
| `nimbus/nimbus-libkrun` | `main`, `v1.17.4-nimbus.1` plus untagged release tooling | Behind latest upstream release | `v1.18.1` | Rebase/port Nimbus patch and release tooling to `nimbus/v1.18.1`, tag `v1.18.1-nimbus.1`, update installers/packages |

## Current Execution State

Final audit on 2026-05-25 re-ran the fork inventory gate against upstream
remotes after the FUS4 source/package updates:

- `nimbus/deno` is clean on `nimbus/v2.8.0`; upstream source tag `v2.8.0`
  and Nimbus release tag `v2.8.0-nimbus.2` are present locally and remotely.
  `origin/HEAD` still points at `locker-v2.7.14`; FUS6 owns default-branch
  alignment or a documented exception.
- `nimbus/rusty_v8` is clean on `nimbus/v149.0.0`; upstream source tag
  `v149.0.0` and Nimbus release tag `v149.0.0-nimbus.1` are present locally
  and remotely. `origin/HEAD` still points at `locker-v147.4.0`; FUS6 owns
  default-branch alignment or a documented exception.
- `nimbus/bun` is clean on `nimbus/bun-main-20260525`; the active source
  contract is proof tag `nimbus-bun-jsc-proof-main-20260525`. FUS6 renamed the
  active default branch to the same `nimbus/<source>` shape as the other forks
  without pretending the source is based on an official `bun-vX.Y.Z` release.
- `nimbus/nimbus-crun` is clean on `nimbus/1.27.1`; upstream source tag
  `1.27.1` and Nimbus release tag `v1.27.1-nimbus.2` are present locally and
  remotely. `origin/HEAD` still points at `main`; FUS6 owns default-branch
  alignment or a documented patch-carrier exception.
- `nimbus/nimbus-libkrun` is clean on `nimbus/v1.18.1`; upstream source tag
  `v1.18.1` and Nimbus release tag `v1.18.1-nimbus.1` are present locally and
  remotely. `origin/HEAD` still points at `main`; FUS6 owns default-branch
  alignment or a documented full-source fork exception.

## Important Finding: rusty_v8 `e5abf2b`

The upstream commit
`e5abf2b25ab2faf784ee40dd1b25f15c489f8c0b` is directly relevant to Nimbus.
It fixes an isolate teardown panic by keeping `ANNEX_SLOT` alive until after V8
isolate disposal. The upstream PR says the regression appeared when Deno moved
to rusty_v8 `147.4.0`, which is the same release family Nimbus currently pins
through `v147.4.0-locker.7`.

Nimbus' fork has nearby custom work:

- `v8::Locker` and `v8::UnenteredIsolate`
- weak-handle teardown hardening
- `active_weak_data` bookkeeping
- explicit weak handle clearing before isolate teardown

That means the upstream commit is not a clean "already handled" case. It is
overlapping safety work. The rebase must either:

- include the upstream fix exactly, then reapply Nimbus' weak teardown registry
  without regressing the new `ANNEX_SLOT` lifetime invariant, or
- document a stronger equivalent Nimbus implementation with tests that cover
  both the upstream regression and Nimbus' weak teardown cases.

Success requires tests for all three paths:

- upstream regression: slot drop can access annex during isolate teardown;
- Nimbus regression: active weak handles are cleared before isolate disposal;
- locker/unentered isolate teardown preserves the same annex lifetime invariant.

## Upstream Impact Audit

This plan is not a mechanical tag refresh. Each fork has Nimbus-local behavior
that must be re-evaluated against upstream's current implementation before we
carry it forward. The execution rule is:

```text
start from selected upstream release
  -> run Nimbus proof/tests against the mostly-upstream candidate
  -> port only the remaining product-required deltas
  -> prefer upstream APIs, lifecycle hooks, and safety fixes when they now cover
     the same behavior
```

### Deno Runtime Family

State at FUS4 start:

- `nimbus/deno`: `locker-v2.7.14`, `v2.7.14-locker.43`.
- `nimbus/rusty_v8`: `locker-v147.4.0`, `v147.4.0-locker.7`.
- Selected upstream family: Deno `v2.8.0`, which declares rusty_v8 `149.0.0`.
- Upstream rusty_v8 also has later tags `v149.1.0` and `v149.2.0`; the
  teardown fix `e5abf2b25ab2faf784ee40dd1b25f15c489f8c0b` is contained by
  `v149.2.0`, not by `v149.0.0`.

Nimbus rusty_v8 delta over `v147.4.0`:

- Adds `v8::Locker` and `v8::UnenteredIsolate`.
- Adds panic-safety, compile-fail tests, scope annex initialization, and
  Enter/Exit ordering hardening for the locker path.
- Adds weak-handle teardown hardening through active weak handle tracking.
- Includes CI and release-contract changes, plus `agentstation`/`neovex` to
  `nimbus` rename work.
- Touches upstream-overlap files including `Cargo.toml`, `build.rs`,
  `src/binding.cc`, `src/handle.rs`, `src/isolate.rs`, `src/lib.rs`,
  `src/scope.rs`, `src/scope/raw.rs`, and `tests/test_api.rs`.

Relevant upstream rusty_v8 changes from `v147.4.0` to `v149.0.0`:

- Adds `IsolateLiveness` and moves `Global<T>` liveness tracking from
  `IsolateHandle` storage to a liveness pointer.
- Reduces global handle liveness overhead.
- Adds fused callback-info parts and V8 14.9 binding surface changes.
- Adds String::Concat, Object::SetLazyDataProperty, inspector async-task, and
  debug break-on-next-call bindings.

Relevant upstream rusty_v8 changes after `v149.0.0`:

- `e5abf2b` keeps the isolate annex slot alive through V8 isolate disposal and
  splits teardown into prepare, remaining-finalizer, and finish phases.
- The commit changes `IsolateHandle::dispose(self)` to `dispose(&self)` and
  adds an upstream regression test for annex access during teardown.

Execution implication:

- Base the Nimbus branch on upstream `v149.0.0` for Deno `v2.8.0`
  compatibility.
- Do not blindly reapply Nimbus' old `take_annex`/weak-teardown patch shape.
  Port it onto upstream's `IsolateLiveness` and annex-dispose sequence.
- Either cherry-pick `e5abf2b` onto the `v149.0.0` base or implement an
  equivalent patch with the upstream regression test preserved.
- Treat any attempt to pin Deno `v2.8.0` to rusty_v8 `v149.2.0` as an explicit
  compatibility decision that must be proven separately; Deno's declared crate
  version remains `149.0.0`.

Nimbus Deno delta over `v2.7.14`:

- Ports the locker lifecycle to Deno `v2.7.14`.
- Adds a large Node-compat patch stack across SQLite optionality, subprocess
  host-env clearing, legacy URL behavior, assert/util/perf_hooks/process
  compatibility, dgram/TCP/UDP fd helpers, HTTP2 binding surface, TLS/HTTPS
  compatibility, async_hooks, CommonJS wrapper behavior, `node:vm`
  compatibility, `node:test`, readline promises, and realm teardown.
- Carries dependency/advisory and release rename work.
- Overlaps upstream files heavily in `ext/node`, `ext/net`, `ext/fetch`,
  `libs/core/runtime`, resolver code, and Node compatibility tests.

Relevant upstream Deno `v2.8.0` changes:

- Bumps V8 to 14.9.
- Fixes a `vm.createContext` isolate teardown panic path.
- Refactors core module registration and synthetic ESM extension support.
- Restores or expands several Node compatibility surfaces including
  `module.registerHooks`, CommonJS load-hook behavior, `node:vm`
  `linkRequests`/`moduleRequests`/`instantiate`, `displayErrors`, compile cache
  companions, TLS session and peer-certificate compatibility, HTTPS agent
  behavior, dgram/net/internal module exposure, async_hooks, `node:test`, TAP
  reporting, perf_hooks histograms, and trace_events.
- Lazy-loads many Node polyfills, which may obsolete Nimbus-local performance
  or compatibility work.

Execution implication:

- Create a temporary mostly-upstream Deno `v2.8.0` candidate that points at the
  Nimbus rusty_v8 candidate before reapplying the full 43-commit Nimbus patch
  stack.
- Run Nimbus' Node/runtime focused tests against that candidate and classify
  each failing behavior as:
  - upstream already fixed and no Nimbus patch needed;
  - still missing and worth porting;
  - obsolete because upstream implemented a better seam;
  - intentionally different and requiring a new Nimbus policy decision.
- Specifically audit whether upstream Deno's `vm.createContext` teardown fix
  supersedes Nimbus' `core: defer realm slot teardown to V8 GC` patch.
- Do not carry broad Node compatibility shims when upstream `v2.8.0` now owns
  the behavior.

### Bun Runtime Family

Current local state:

- `nimbus/bun`: historical proof branch `nimbus/bja4l2-simdutf-namespace`,
  historical proof tag `bun-v1.4.0-nimbus.5`; FUS3 canonical source branch
  `nimbus/bun-main-20260525` and proof tag
  `nimbus-bun-jsc-proof-main-20260525`.
- The local branch is based on an upstream-main proof commit after
  `bun-v1.3.14`, not on an official `bun-v1.4.0` release.
- Latest official release observed during this audit: `bun-v1.3.14`.
- Local upstream main has advanced beyond the Nimbus proof base.

Nimbus delta after the proof base includes:

- JSC embed probe targets.
- Sync and async HostBridge call proofs.
- Program-bundle execution proof.
- Timeout and pre-entry cancellation proofs.
- Permission inventory and native permission deny profile proofs.
- Memory behavior proof.
- Package/module resolver denial proof.
- Lifecycle reuse proof.
- Shared embedder build mode.
- Dynamic TLS for the shared embedder.
- HostBridge embed entrypoint and invocation ABI.
- Private simdutf namespace build option.
- Stack-size filtering for shared embedder builds.

Relevant upstream main changes after the Nimbus proof base:

- Adds macOS cross-compilation support from Linux, including build-tool and SDK
  handling.
- Ports `Bun.stringWidth` to C++ with explicit SIMD.
- Restricts crate-internal items to `pub(crate)` and removes exposed dead code.
- Adds broader hardening and bounds validation across multiple subsystems.
- Touches files that Nimbus also changed: build scripts, Rust build flags,
  WebKit/mimalloc dependency helpers, `src/jsc/ModuleLoader.rs`,
  `src/jsc/bindings/bindings.cpp`, `src/runtime/api/BunObject.rs`, and
  `src/simdutf_sys/simdutf.rs`.

Execution implication:

- Bun cannot currently be treated as a release-standard fork unless the latest
  official release contains the Rust/embedder surfaces Nimbus needs. If those
  surfaces exist only on upstream main, Bun remains a proof-only exception.
- Do not tag the current source as `bun-v1.4.0-nimbus.N` unless upstream
  actually publishes a matching official `bun-v1.4.0` source release.
- Rebase the proof branch onto current upstream main to discover conflicts, but
  keep that branch clearly named as a proof-main branch, for example
  `nimbus/main-<date>` or another documented non-release form.
- Prefer upstream's new macOS cross-compilation and build-tool support over
  maintaining Nimbus-only build machinery for the same problem.
- Do not reverse upstream's `pub(crate)` hardening by broadly widening
  internals. Nimbus' embedder ABI should live behind a narrow, explicit exported
  C/Rust boundary with tests that prove the exported symbols and fail-closed
  no-link behavior.
- Keep resolver, permission, memory, and cancellation hooks only where no
  upstream embedder hook exists. If upstream introduces formal embedder seams,
  use those instead of carrying parallel hooks.

### crun/libkrun Sandbox Family

Current local state:

- `nimbus-crun`: `main`, `v1.27.1-nimbus.1`.
- `nimbus-crun` now has the standard `upstream` remote and local upstream
  source tag `1.27.1` after FUS1. It still needs an active
  `nimbus/1.27.1` branch and an updated release tag when the paired
  libkrun contract changes.
- `nimbus-libkrun`: `main`, `v1.17.4-nimbus.1` plus untagged release-tooling
  commits.
- Selected upstream sandbox family: crun `1.27.1`, libkrun `v1.18.1`.

Nimbus crun delta:

- `nimbus-crun` is a small patch-carrier repo, not a full crun source mirror.
- The active patch adds `krun.port_map` parsing with:
  - legacy `HOST_PORT:GUEST_PORT`;
  - IPv4 `ADDR:HOST_PORT:GUEST_PORT`;
  - bracketed IPv6 `[ADDR]:HOST_PORT:GUEST_PORT`;
  - length, duplicate, range, and malformed-input checks;
  - fail-closed behavior when address-bearing mappings require
    `krun_set_port_map_with_bind_address` but the loaded libkrun does not
    export it.
- Build and CI helpers now verify the paired Nimbus libkrun root, RUNPATH, and
  the required exported symbol.

Execution implication:

- The crun source base is already intended to be upstream `1.27.1`, and FUS1
  made that mechanically auditable by adding/fetching upstream refs. FUS4 must
  keep proving the patch applies to the exact tag before publishing another
  Nimbus release.
- Keep crun as a patch-carrier repo unless a later product need requires a full
  source mirror. That is simpler and easier to audit for a one-patch integration
  point.
- Update the paired libkrun pin before publishing another crun release, because
  the README, build workflow, and helper scripts still point at
  `v1.17.4-nimbus.1`.

Nimbus libkrun delta ported from upstream `v1.17.4` to upstream `v1.18.1`:

- Adds `krun_set_port_map_with_bind_address` to `include/libkrun.h`.
- Changes TSI host port maps from `HashMap<u16, u16>` to a typed mapping that
  can carry an optional `IpAddr`.
- Parses legacy and bind-address-aware port map syntax.
- Denies unmapped guest listen requests with `EPERM` when an explicit port map
  is configured.
- Adds unit tests for legacy, IPv4, IPv6, and legacy-denies-bind-address syntax.
- Adds release archive scripts, GitHub release workflow, and README release
  documentation.

Relevant upstream libkrun `v1.18.1` changes:

- Refactors virtio queues so devices receive `DeviceQueue` values at
  activation instead of owning raw queues internally.
- Changes vsock TSI behavior, including `sendto_addr` binding fixes, listen
  backlog handling, edge-triggered OUT handling, and log severity reduction for
  some OUT errors.
- Adds virtio-fs read-only support via `krun_add_virtiofs3` and
  `KRUN_FS_ROOT_TAG`.
- Adds DHCP client networking flags, nested virtualization checks on Linux,
  crate/package metadata cleanup, macOS test/build support, and expanded
  network integration tests.
- Replaces the `cap-ng` dependency with `caps`.
- Touches the same libkrun files Nimbus patched: `include/libkrun.h`,
  `src/libkrun/src/lib.rs`, vsock device/module/proxy files, TSI stream/dgram
  files, unix proxy files, and `src/vmm/src/vmm_config/vsock.rs`.

Execution implication:

- Port Nimbus' bind-address map onto upstream `v1.18.1` after the virtio queue
  refactor, not onto the old `with_queues` shape.
- Reuse upstream's new TSI fixes rather than reimplementing around them.
- Add tests that prove bind-address maps still work with the `v1.18.1` vsock
  queue activation model, dgram `sendto_addr` behavior, and explicit-deny
  unmapped guest listen behavior.
- Update release tooling for upstream's `libcap-ng` to `caps` dependency shift
  and any changed package prerequisites.
- Re-tag `nimbus-libkrun` as `v1.18.1-nimbus.1`, then update `nimbus-crun`
  package and README pins to that tag.

## Required Rebase Discipline

For every fork, execution must produce a short port log that answers:

- Which upstream release or main commit was selected?
- Which Nimbus commits were dropped because upstream now covers them?
- Which Nimbus commits were ported, and why?
- Which upstream APIs replaced Nimbus-local seams?
- Which tests prove the retained Nimbus behavior?
- Which branch and tag were pushed?

This prevents the fork set from becoming a second upstream implementation by
accident. The desired end state is small, explicit, product-owned deltas on top
of current upstream code.

## Execution Plan

### FUS0 - Baseline Inventory and Guardrails

Status: completed 2026-05-25

Produce a machine-readable fork inventory file or verification script that
records:

- local path;
- current branch;
- `origin` URL;
- `upstream` URL;
- default branch from `git ls-remote --symref origin HEAD`;
- latest upstream release tag;
- selected Nimbus source tag;
- selected Nimbus release tag;
- clean/dirty state.

Success criteria:

- The inventory command runs from the Nimbus repo.
- It reports all five in-scope forks.
- It fails if `origin` is not a Nimbus GitHub URL or `upstream` is missing.
- It reports Bun's current remote inversion as a failure before FUS1.
- It records whether each local fork contains the selected upstream source tag
  locally, so missing upstream refs like the current `nimbus-crun` `1.27.1`
  tag gap are visible before port work starts.
- It records the current Nimbus delta base for each fork, including whether the
  repo is a full source fork or a patch-carrier repo.
- It re-checks upstream release/tag heads at execution start. If a newer
  upstream release appears, stop and update this plan before mutating fork
  source branches.

Evidence:

- Added `scripts/verify-fork-upstream-standardization.sh`.
- Initial offline run reported the expected FUS1 drift: Deno/rusty_v8/libkrun
  used `ssh://` upstream URLs, Bun had inverted `origin`/`nimbus` remotes, and
  `nimbus-crun` was missing `upstream`.
- Networked run re-checked selected upstream source tags and latest tracked
  release tags before source mutation.

### FUS1 - Normalize Remotes and Local Upstream Checkouts

Status: completed 2026-05-25

Without changing source code, normalize Git remotes:

- `origin` points to `git@github.com:nimbus/<repo>.git`.
- `upstream` points to `git@github.com:<upstream-org>/<repo>.git`.
- optional `local-<upstream-org>` remotes point to local upstream checkouts.
- `nimbus/bun` no longer uses a remote named `nimbus`.
- `nimbus-crun` has an `upstream` remote.

Success criteria:

- `git remote -v` matches the standard in all fork repos.
- `git remote set-head origin -a` and `git remote set-head upstream -a`
  complete or produce documented upstream limitations.
- No fork repo has a `nimbus` remote.
- No source changes are introduced by this phase.

Evidence:

- Normalized:
  - `nimbus/deno`: `upstream` -> `git@github.com:denoland/deno.git`.
  - `nimbus/rusty_v8`: `upstream` ->
    `git@github.com:denoland/rusty_v8.git`.
  - `nimbus/bun`: `origin` -> `git@github.com:nimbus/bun.git`,
    `upstream` -> `git@github.com:oven-sh/bun.git`, removed remote named
    `nimbus`.
  - `nimbus/nimbus-crun`: added `upstream` ->
    `git@github.com:containers/crun.git`.
  - `nimbus/nimbus-libkrun`: `upstream` ->
    `git@github.com:containers/libkrun.git`.
- Fetched `nimbus-crun` upstream refs so local tag `1.27.1` is present.
- Ran `git remote set-head origin -a` and
  `git remote set-head upstream -a` across all five forks.
- `bash scripts/verify-fork-upstream-standardization.sh` passed after remote
  normalization.

### FUS2 - Deno Family Rebase and Rename

Status: completed 2026-05-25

Move the Deno runtime family from historical `locker` pins to Nimbus release
pins:

1. Base `nimbus/rusty_v8` on upstream `v149.0.0`, because Deno `v2.8.0`
   declares `v8 = "149.0.0"`.
2. Port Nimbus' required runtime deltas onto upstream `IsolateLiveness` and
   annex teardown semantics.
3. Include or equivalently implement the upstream teardown fix `e5abf2b`.
4. Run focused rusty_v8 teardown tests before tagging.
5. Tag `nimbus/rusty_v8` as `v149.0.0-nimbus.1`.
6. Base `nimbus/deno` on upstream `v2.8.0`.
7. Build a mostly-upstream Deno `v2.8.0` candidate against the Nimbus
   rusty_v8 candidate before reapplying the old Nimbus Deno stack.
8. Classify every old Nimbus Deno patch as dropped, ported, replaced by
   upstream, or newly required.
9. Patch Deno to use `https://github.com/nimbus/rusty_v8` tag
   `v149.0.0-nimbus.1`.
10. Tag `nimbus/deno` as `v2.8.0-nimbus.2`.
11. Update Nimbus `Cargo.toml` and `Cargo.lock` to consume the new Nimbus tags.

Success criteria:

- `nimbus/rusty_v8` has branch `nimbus/v149.0.0` and tag
  `v149.0.0-nimbus.1`.
- `nimbus/deno` has branch `nimbus/v2.8.0` and tag `v2.8.0-nimbus.2`.
- `nimbus/deno` no longer pins `v147.4.0-locker.7`.
- Nimbus root no longer pins `v2.7.14-locker.43` or `v147.4.0-locker.7`.
- Focused rusty_v8 teardown tests pass, including the upstream `e5abf2b`
  regression and Nimbus weak teardown regression.
- The Deno port log identifies which historical Node/runtime patches were
  dropped because upstream `v2.8.0` now owns the behavior.
- The Deno port log explicitly resolves whether upstream's
  `vm.createContext` teardown fix supersedes Nimbus'
  `core: defer realm slot teardown to V8 GC` patch.
- Nimbus focused runtime tests pass against the repinned tags.

Evidence:

- Created `nimbus/rusty_v8` branch `nimbus/v149.0.0` from upstream tag
  `v149.0.0`.
- Ported the upstream teardown fix
  `e5abf2b25ab2faf784ee40dd1b25f15c489f8c0b` onto the Deno-required
  `v149.0.0` base.
- Ported the Nimbus `v8::Locker`/`v8::UnenteredIsolate` API, locker
  panic-safety/lifetime tests, and weak-handle teardown hardening onto
  upstream's `IsolateLiveness` and annex disposal sequence.
- Preserved upstream's
  `isolate_slot_drop_can_access_annex_during_teardown` regression test and
  Nimbus' `leaked_raw_weak_handle_survives_isolate_teardown` regression test.
- Attempted focused teardown verification with upstream `v149.0.0` prebuilts;
  it failed as expected with missing Nimbus-only `v8__Locker__*` C++ symbols,
  proving that locker verification requires a Nimbus source-built archive or
  published Nimbus prebuilt artifact.
- Quarantined stale generated Chromium toolchain/build outputs under
  `/private/tmp/rusty-v8-generated-stale-20260525` after the first source-build
  attempt exposed an overlay extraction conflict in `third_party/rust-toolchain`
  (`simd.rs` plus `simd/mod.rs`).
- `env V8_FROM_SOURCE=1 cargo test
  isolate_slot_drop_can_access_annex_during_teardown -- --nocapture` passed
  after a clean local source build (`1 passed`, `245 filtered out`;
  build/test completed in `20m 52s`).
- `env V8_FROM_SOURCE=1 cargo test
  leaked_raw_weak_handle_survives_isolate_teardown -- --nocapture` passed
  (`1 passed`, `245 filtered out`; build/test completed in `11.01s` after
  source archive reuse).
- `env V8_FROM_SOURCE=1 cargo test --test test_locker -- --nocapture` passed
  (`9 passed`).
- `env V8_FROM_SOURCE=1 cargo test --test test_ui -- --nocapture` passed
  (`1 passed`), with all `15` compile-fail cases matching, including
  `locker_double_borrow`, `locker_not_send`, and `locker_scope_outlives`.
- `cargo fmt --all --check` and `git diff --check` passed in
  `/Users/jack/src/github.com/nimbus/rusty_v8`; the `rusty_v8` worktree was
  clean after focused verification.
- Ported the Nimbus release workflow/README/prebuilt lookup contract so the
  fork publishes and consumes `nimbus/rusty_v8` release assets by default.
- Created local annotated tag `v149.0.0-nimbus.1` on
  `nimbus/rusty_v8` commit `9b77553`.
- Created `nimbus/deno` branch `nimbus/v2.8.0` from upstream tag `v2.8.0`.
- Verified Cargo can patch Deno `v2.8.0` to the sibling
  `/Users/jack/src/github.com/nimbus/rusty_v8` checkout without committing a
  local path.
- Initial Deno `deno_core` check hit the known checked-in macOS
  `-fuse-ld=lld` target rustflag; the successful proof used
  `CARGO_ENCODED_RUSTFLAGS` to drop only that linker selection while preserving
  required framework links and `tokio_unstable`.
- `env V8_FROM_SOURCE=1 CARGO_ENCODED_RUSTFLAGS=...
  cargo --config 'patch.crates-io.v8.path="../rusty_v8"' check -p deno_core`
  passed on mostly-upstream Deno `v2.8.0` against the sibling Nimbus
  `rusty_v8` candidate (`Finished dev profile` in `19m 22s`).
- The Deno worktree was restored to clean after removing the temporary
  local-patch `Cargo.lock` side effect.
- Patched Deno `v2.8.0` to consume the published
  `https://github.com/nimbus/rusty_v8` tag `v149.0.0-nimbus.1`.
  `Cargo.lock` now resolves `v8 v149.0.0` from
  `git+https://github.com/nimbus/rusty_v8?tag=v149.0.0-nimbus.1#9b775538`.
- Added `.cargo/config.toml` `RUSTY_V8_VERSION = "149.0.0-nimbus.1"` so the
  default rusty_v8 prebuilt lookup follows the Nimbus release tag.
- `V8_FROM_SOURCE=1 CARGO_NET_GIT_FETCH_WITH_CLI=true
  CARGO_ENCODED_RUSTFLAGS=... cargo check -p deno_core --locked` passed
  against the published `nimbus/rusty_v8` tag (`serde_v8 v0.310.0` and
  `deno_core v0.401.0`; `Finished dev profile` in `23m 27s`).
- Created Deno commit `7530d3c1a1` (`build: pin rusty_v8 to nimbus v149`) on
  branch `nimbus/v2.8.0` and annotated tag `v2.8.0-nimbus.1`.
- Pushed `nimbus/deno` branch `nimbus/v2.8.0` and tag `v2.8.0-nimbus.1`.
  GitHub initially rejected the push because the remote was missing upstream
  object `d0fe7ce6e83047e3768f00b26a5917f067062c6f`; a temporary
  `nimbus/object-repair-d0fe` branch was pushed and then deleted after the
  real branch/tag push succeeded.
- Nimbus consumer verification against `v2.8.0-nimbus.1` failed because
  Nimbus still requires Deno-side locker and warm-reuse APIs:
  `RuntimeOptions.use_locker`, `JsRuntime::{acquire_v8_lock,release_v8_lock,
  is_v8_lock_held,reset_request_state}`.
- Ported the minimal Nimbus locker lifecycle seam onto Deno `v2.8.0` in
  commit `363de88e0d` (`runtime: restore nimbus locker lifecycle seam`).
  This restored the managed isolate wrapper, cooperative lock guard, and
  warm-reuse reset helpers while preserving upstream `v2.8.0` runtime shape.
- `V8_FROM_SOURCE=1 CARGO_NET_GIT_FETCH_WITH_CLI=true
  CARGO_ENCODED_RUSTFLAGS=... cargo check -p deno_core --locked` passed
  after the lifecycle seam port (`Finished dev profile` in `4.70s`).
- Pushed updated `nimbus/deno` branch `nimbus/v2.8.0` and annotated tag
  `v2.8.0-nimbus.2`.
- Repinned Nimbus root `Cargo.toml`, `Cargo.lock`, and `.cargo/config.toml` to
  `nimbus/deno` `v2.8.0-nimbus.2` plus `nimbus/rusty_v8`
  `v149.0.0-nimbus.1`. The root `deno_node` dependency now uses only the
  `sync_fs` feature because Deno `0.186.0` moved SQLite into the separate
  `deno_node_sqlite` crate without a `deno_node/sqlite` feature.
- Updated Nimbus runtime bootstrap Deno/V8 version strings to
  `2.8.0-nimbus` and `149.0.0-nimbus.1`.
- Updated Nimbus' `deno_web::deno_web::init(...)` call for the Deno `v2.8.0`
  `enable_css_parser_features` option, passing `false`.
- `V8_FROM_SOURCE=1 CARGO_NET_GIT_FETCH_WITH_CLI=true
  cargo check -p nimbus-runtime --locked` passed against the repinned Nimbus
  root (`Finished dev profile` in `3.58s` after the Deno web option fix).
- `V8_FROM_SOURCE=1 CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test -p
  nimbus-runtime locker -- --nocapture` passed: `4 passed`, `0 failed`,
  `4 ignored`, `503 filtered out` in the unit-test binary, plus
  `tests/locker_smoke.rs` `5 passed`, `0 failed`.
- `V8_FROM_SOURCE=1 CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test -p
  nimbus-runtime warm_pooled_runtime_rebinds_host_bridge_per_invocation --
  --nocapture` passed: `1 passed`, `0 failed`, `510 filtered out`.

Deno port log:

- Ported: build/pin commits `13ca08223a`, `6d24238601`, `699c163c82`, and
  `b58241635c` are represented by new commit `7530d3c1a1`, which replaces
  the old `locker` source URL/tag with the release-standard Nimbus tag
  `v149.0.0-nimbus.1`.
- Ported: `84b679af6a` (`runtime: port locker lifecycle to deno v2.7.14`) is
  represented by new commit `363de88e0d` on `v2.8.0-nimbus.2`. The initial
  mostly-upstream `v2.8.0-nimbus.1` proof compiled Deno itself, but Nimbus
  consumer verification proved these Deno-side lifecycle APIs remain
  product-required for cooperative locking and warm runtime reuse.
- Dropped as superseded by upstream Deno `v2.8.0` unless a future focused
  compatibility test proves a narrower regression: Node compatibility commits
  `4231c41072`, `a2cc5bfdc7`, `955c380cb0`, `cfc77f10d7`, `303711bb9a`,
  `c593ebb2c0`, `016fe4d404`, `f705183e8c`, `fe598a4780`, `92799e7927`,
  `066a963f2c`, `2909a8765b`, `ae84be3077`, `c2e2ad2cb6`, `c240cfa7c3`,
  `ca348ad326`, `455194bda4`, `d4ae0dac4d`, `e79138ef03`, `830d4a51d3`,
  `40915d490e`, `0f311eb29b`, `bcdbd4c809`, `cccaf433ee`, `94658dae7f`,
  `8503bcdde4`, `0619cfd52c`, `4729bd842d`, `7793d825f7`, `505e6344b7`,
  `d4ece4fcc2`, `61aa46dd4b`, `1cb25cbaa5`, `b4945bfbfa`, `8ffed8eac6`,
  `21972e139a`, `f2e2a9fdd8`, `e92524eb07`, `3744ab9e5d`, `5499c6d9fd`,
  `7292844924`, `8f3574a26c`, `1a97c7f868`, `7809d42ef9`, `b2ff78080e`,
  `b964e6f79f`, `8325875e61`, `f95ca0c9df`, `40adb8a27e`, `964ebfd9a3`,
  `bf73259373`, `9ea0541200`, `59e56e1b4c`, `51615ddb20`, `5cddcae579`,
  `e9113a89da`, `23359c157c`, `664c2b64bc`, `98d09eab1e`, `b748ccc7f6`,
  `5854015a03`, `7d7165f9ed`, `d1371b8fbe`, `b550a6c5fa`, `59847f42f0`,
  `e6321ef2ea`, `a5143cfc58`, `b6334f07be`, `40e690b3f9`, `c8710a20b9`,
  `0decba9128`, `b558da020f`, `f96d25f859`, `dde1e16731`, and
  `d7d2124330`. Upstream `v2.8.0` includes broad overlapping Node work across
  `node:vm`, TLS/HTTPS, dgram/net, async_hooks, CommonJS hooks, perf_hooks,
  `node:test`, and lazy Node polyfill loading, so Nimbus is not carrying these
  as blind fork deltas.
- Dropped as superseded by upstream/runtime-family teardown fixes:
  `ead1570f78` (`core: defer realm slot teardown to V8 GC`). Upstream Deno
  `v2.8.0` includes the `vm.createContext` teardown fix, and Nimbus'
  additional teardown safety is represented by the `nimbus/rusty_v8`
  `IsolateLiveness`/annex/weak-handle regression tests.
- Dropped from the fork baseline as release hygiene/advisory churn rather than
  a product delta: `da4e244297`, `319792a528`, and `57ae90dd86`. Any current
  advisory remediation should be handled as a fresh dependency audit against
  the `v2.8.0-nimbus.2` baseline, not by replaying historical lockfile edits.

### FUS3 - Bun Release Baseline Decision

Status: completed 2026-05-25

Normalize Bun remotes first, then decide whether the current Bun fork can be a
release-standard fork.

Decision gate:

- If upstream `bun-v1.3.14` contains the required embedder APIs and build
  surfaces, create `nimbus/bun-v1.3.14`, port Nimbus deltas, tag
  `bun-v1.3.14-nimbus.1`, and update Nimbus adapter metadata.
- If required APIs are only available after `bun-v1.3.14`, record the current
  fork as a proof-only mainline exception and block product release pins from
  presenting it as a release-derived fork.
- If upstream main has introduced canonical build, cross-compile, permission,
  resolver, ABI, or lifecycle seams that overlap Nimbus proof patches, use the
  upstream seam and shrink the Nimbus delta.

Success criteria:

- `nimbus/bun` has standard `origin` and `upstream` remotes.
- Bun branch/tag names are either release-standard or explicitly marked
  proof-only; the active source contract uses
  `nimbus-bun-jsc-proof-main-20260525`, not a fake `bun-v1.4.0` tag.
- Adapter manifests, package helpers, installer fixtures, docs, and diagnostics
  do not claim a mainline proof commit is an upstream release.
- The Bun port log records whether upstream main's macOS cross-compilation
  support replaces Nimbus-local build assumptions.
- The Bun port log records whether upstream `pub(crate)` hardening required a
  narrower Nimbus exported ABI instead of broadening upstream internals.
- No Bun branch/tag named like an official release is created unless the
  matching official upstream release exists.
- The Bun verification gate passes for the chosen posture.

Evidence:

- Refreshed `nimbus/bun` from `upstream` before making the decision. Latest
  official upstream release remains `bun-v1.3.14`; upstream `main` has advanced
  beyond Nimbus' proof base.
- Verified `bun-v1.3.14` does not contain the Rust workspace/embedder source
  surfaces Nimbus consumes: the release tree has no root `Cargo.toml`, no
  `src/embed_probe`, and no source-owned shared embedder build lane. Current
  upstream `main` has the Rust migration, but it still does not contain
  Nimbus' embedder proof target or HostBridge ABI.
- Verified the Nimbus proof branch is based on upstream-main commit
  `f161e0311d56ece228d71de12b7747f9c2591303`, not an official `bun-v1.4.0`
  release. The Nimbus delta after that base includes the embed probe, sync and
  async HostBridge proof, program bundle execution, timeout/cancellation,
  native permission denial, resolver/package denial, lifecycle reuse, shared
  embedder mode, dynamic TLS, HostBridge ABI, private `simdutf` namespace, and
  executable-only stack-size filtering.
- Created and pushed mainline-proof branch `nimbus/bun-main-20260525` and
  annotated proof tag `nimbus-bun-jsc-proof-main-20260525`, both pointing at
  `ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57`.
- Updated the active Nimbus Bun/JSC adapter source contract, manual CI default,
  installer constants, package/release helpers, runtime diagnostics tests,
  manifest tests, and API/operator docs to reference
  `nimbus-bun-jsc-proof-main-20260525`.
- Left historical `bun-v1.4.0-nimbus.*` tags and proof logs intact as evidence,
  but they are no longer the active Nimbus adapter source contract.
- `cargo test -p nimbus-runtime backends::bun_jsc --lib -- --nocapture`
  passed (`10 passed`, `0 failed`).
- `cargo test -p nimbus-server registry_and_license::runtime_metrics --lib --
  --nocapture` passed (`2 passed`, `0 failed`).
- `bash scripts/verify-bun-jsc-adapter-package-helper.sh` passed with the new
  proof-main adapter version/ref.
- `bash scripts/verify-bun-jsc-release-assets-helper.sh` passed with the new
  proof-main adapter version/ref.
- `git diff --check`, `cargo fmt --all --check`, and
  `bash scripts/verify-fork-upstream-standardization.sh` passed after the
  source-contract update.

Bun port log:

- Release-standard fork: deferred. The latest official upstream release
  `bun-v1.3.14` is too early for Nimbus' Rust/embedder API surface.
- Proof-main branch: retained as the only honest posture until Bun publishes an
  official release that contains the required Rust/embedder base.
- Upstream macOS cross-compilation: record as a future shrink opportunity. It
  is on upstream `main`, not in `bun-v1.3.14`; do not duplicate that machinery
  if a later Bun release makes it usable for the adapter lane.
- Upstream `pub(crate)` hardening: keep Nimbus' adapter ABI narrow and explicit
  through exported C symbols and manifest checks. Do not widen upstream
  internals broadly just to satisfy Nimbus.
- Product metadata: active manifests and diagnostics now say proof-main source
  ref, not `bun-v1.4.0-nimbus.5`.

### FUS4 - Sandbox Fork Rebase and Package Pin Alignment

Status: completed 2026-05-25

Bring `nimbus-crun` and `nimbus-libkrun` into the same fork contract:

1. Verify `nimbus-crun` still has the standard `upstream` remote to
   `containers/crun`.
2. Create active `nimbus/1.27.1` branch unless a temporary `main` exception is
   explicitly documented in `nimbus-crun` release notes.
3. Verify the crun patch-carrier applies cleanly to exact upstream source tag
   `1.27.1` and records the upstream source commit in release notes.
4. Base `nimbus-libkrun` on upstream `v1.18.1`.
5. Port Nimbus bind-address and release tooling deltas onto upstream's
   `v1.18.1` virtio queue, TSI, networking, and package dependency changes.
6. Tag `nimbus-libkrun` as `v1.18.1-nimbus.1`.
7. Update `nimbus-crun` README, build workflow, build helper, and release notes
   to consume `nimbus-libkrun` `v1.18.1-nimbus.1`.
8. If any `nimbus-crun` paired-libkrun pin, release text, workflow, or helper
   changes, tag `nimbus-crun` as `v1.27.1-nimbus.2`; keep
   `v1.27.1-nimbus.1` as immutable history.
9. Update Nimbus installers, Linux package workflows, helper tests, docs, and
   `scripts/verify-fork-upstream-standardization.sh` to use
   `v1.18.1-nimbus.1` and the selected crun tag.

Success criteria:

- `nimbus-crun` has a valid `upstream` remote and active release branch.
- `nimbus-libkrun` has branch `nimbus/v1.18.1` and tag
  `v1.18.1-nimbus.1`.
- `nimbus-crun` has branch `nimbus/1.27.1` and tag `v1.27.1-nimbus.2` if the
  paired libkrun pin or release/build contract changes.
- No active installer/package default still references `v1.17.4-nimbus.1`
  unless explicitly testing upgrade behavior. This includes
  `packaging/linux-distribution-contract.env`, install helper fixtures, Linux
  package helper fixtures, Fedora/SRPM helper fixtures, VMM validation helper
  fixtures, and active sandbox/distribution docs.
- No active `nimbus-crun` workflow, README, or build helper still requires
  `libkrun.so.1.17.4`.
- The libkrun port log records which upstream `v1.18.1` TSI fixes are reused
  and how the Nimbus bind-address map fits the new activation model.
- Tests prove bind-address port maps, legacy port maps, explicit unmapped-listen
  denial, and crun fail-closed symbol detection against the new libkrun tag.
- Linux package render tests and install verifier fixtures pass with the new
  libkrun tag.
- Required focused fork gates include:
  `cargo test -p libkrun port_map_tests -- --nocapture`,
  `bash scripts/verify-release-archive.sh` against the generated/extracted
  libkrun archive, `bash scripts/verify-patch.sh`,
  `bash scripts/verify-port-map-parser.sh`, and the paired crun build gate
  against the new libkrun root.

Evidence:

- `nimbus/nimbus-libkrun` branch `nimbus/v1.18.1` was based on upstream
  `v1.18.1` and tagged `v1.18.1-nimbus.1`.
- Ported the Nimbus bind-address `HostPortMap` onto upstream's `v1.18.1`
  queue activation model and reused upstream's TSI fixes for datagram
  `sendto_addr` binding, listen backlog handling, edge-triggered OUT handling,
  and the `cap-ng` to `caps` dependency shift.
- Added libkrun tests for IPv4 bind address, IPv6 bind address, explicit
  unmapped-listen denial, and legacy no-explicit-map behavior.
- Updated libkrun release tooling to emit `libkrun.so.1.18.1`, removed the
  obsolete `libcap-ng-dev` prerequisite, and followed the upstream v1.18 init
  build path rather than invoking a removed `make init/init` target.
- `nimbus/nimbus-crun` branch `nimbus/1.27.1` remains a patch-carrier over
  upstream crun `1.27.1` and was tagged `v1.27.1-nimbus.2` for the paired
  libkrun contract bump.
- Updated `nimbus-crun` README, build helper, and GitHub workflow to consume
  `nimbus-libkrun` `v1.18.1-nimbus.1`.
- Updated Nimbus root packaging/install contracts, helper fixtures, VMM bundle
  helper, active distribution/security docs, and
  `scripts/verify-fork-upstream-standardization.sh` to use
  `v1.18.1-nimbus.1` and `v1.27.1-nimbus.2`.
- macOS libkrun direct tests are not meaningful because upstream v1.18.1 still
  compiles Linux-only init sources for the libkrun test lane on Darwin; Linux
  verification ran on Debian 13 `minicloud`.

Verification:

- `cargo test -p krun-devices listen -- --nocapture` on `minicloud`: 4 passed,
  0 failed, 59 filtered out.
- `cargo test -p libkrun port_map_tests -- --nocapture` on `minicloud`: 4
  passed, 0 failed.
- `bash scripts/build-release-archive.sh --output-dir /tmp/nimbus-libkrun-v1.18.1-release --arch amd64 --version v1.18.1-nimbus.1` on `minicloud`: produced `nimbus-libkrun-linux-amd64.tar.gz`, verified `krun_set_port_map_with_bind_address`, and verified pkg-config relocation.
- `bash scripts/verify-release-archive.sh --archive /tmp/nimbus-libkrun-v1.18.1-release/nimbus-libkrun-linux-amd64.tar.gz --expected-libkrunfw-version 5.3.0` on `minicloud`: passed.
- `bash scripts/verify-port-map-parser.sh` in `nimbus-crun`: passed.
- `bash scripts/verify-patch.sh /Users/jack/src/github.com/containers/crun` in
  `nimbus-crun`: passed.
- Paired crun build on `minicloud` against the `v1.18.1-nimbus.1` libkrun
  archive root: passed with `build.libkrun.shared_object=.../libkrun.so.1.18.1`
  and `build.libkrun.linkage=dlopen`.
- `bash scripts/verify-install-helper.sh`: 35 tests passed.
- `bash scripts/verify-build-linux-release-packages-helper.sh`: rendered
  deterministic nimbus, nimbus-libkrun, nimbus-crun, and Bun/JSC adapter deb/rpm
  manifests with updated versions; local package build skipped because `nfpm`
  is not installed.
- `bash scripts/verify-build-apt-repository-helper.sh` on `minicloud`: signed
  apt metadata built and verified via local `apt-ftparchive`.
- `bash scripts/verify-build-fedora-release-srpms-helper.sh` on `minicloud`:
  Fedora 42 SRPM builder produced reusable source RPMs, installed x86_64 RPMs,
  and built/query-verified aarch64 RPM metadata from release artifacts.
- `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`: passed.
- `bash scripts/verify-fork-upstream-standardization.sh --offline`: passed.
- `bash scripts/verify-fork-upstream-standardization.sh`: passed.
- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.

### FUS5 - Active Documentation and Pin Cleanup

Status: completed 2026-05-25

Replace active `locker` naming in current docs and manifests with the new
Nimbus standard. Archived proof logs and completed plans may retain historical
evidence.

Success criteria:

- Active `Cargo.toml`, `Cargo.lock`, runtime docs, package manifests, and active
  plan docs use `-nimbus.N` tags.
- `rg -n "locker" Cargo.toml Cargo.lock docs scripts .github Makefile` only
  reports archived/proof history or intentional runtime implementation names,
  not active fork branch/tag standards.
- Docs explain that historical `locker` tags are immutable history, not the
  current release naming standard.

Evidence:

- `Cargo.toml` now pins Deno-family crates to `nimbus/deno`
  `v2.8.0-nimbus.2` and `nimbus/rusty_v8` `v149.0.0-nimbus.1`; `Cargo.lock`
  resolves to those `-nimbus.N` tags.
- Active Bun adapter package metadata now points at
  `nimbus-bun-jsc-proof-main-20260525` rather than the earlier
  release-looking proof tag.
- Active crun/libkrun package defaults now point at
  `v1.27.1-nimbus.2` and `v1.18.1-nimbus.1`.
- `rg -n "locker-v|v[0-9][^[:space:]]*-locker" Cargo.toml Cargo.lock scripts .github Makefile`
  returned no matches.
- Broader docs search still reports historical compatibility manifests,
  completed proof logs, `v8-locker-fork-plan.md` references, and the current
  runtime implementation concept named `Locker`; those are not active fork
  branch/tag standards.

### FUS6 - GitHub Release and Default Branch Alignment

Status: completed 2026-05-25

Push branches and tags, then align GitHub release pages and default branches.

Success criteria:

- GitHub releases exist for the new Nimbus tags where release artifacts are
  expected: `nimbus/nimbus-libkrun` `v1.18.1-nimbus.1`,
  `nimbus/nimbus-crun` `v1.27.1-nimbus.2` if created, and any runtime fork tag
  that owns prebuilt release assets.
- `git ls-remote --symref git@github.com:nimbus/<repo>.git HEAD` resolves to
  the active Nimbus maintenance branch or a documented exception.
- Any retained default-branch exception is recorded in the owning fork README or
  active plan evidence with the reason and the branch/tag operators should
  consume.
- Release notes include upstream source tag, upstream commit, Nimbus commit,
  verification commands, paired dependency tags, and artifact checksums where
  applicable.

Evidence:

- GitHub default branches were updated:
  - `nimbus/deno` -> `nimbus/v2.8.0`.
  - `nimbus/rusty_v8` -> `nimbus/v149.0.0`.
  - `nimbus/bun` -> `nimbus/bun-main-20260525`.
  - `nimbus/nimbus-crun` -> `nimbus/1.27.1`.
  - `nimbus/nimbus-libkrun` -> `nimbus/v1.18.1`.
- `gh repo view <repo> --json nameWithOwner,defaultBranchRef` verified those
  default branches after mutation.
- `nimbus/rusty_v8` release `v149.0.0-nimbus.1` exists with prebuilt V8
  archives and generated binding assets for macOS arm64, Linux x86_64, Linux
  arm64, and Windows x86_64. Release notes now record upstream tag/commit,
  Nimbus tag/commit, paired Deno tag, verification commands, and the GitHub
  asset-digest inspection command.
- `nimbus/nimbus-libkrun` release `v1.18.1-nimbus.1` exists with amd64/arm64
  archives and `checksums.txt`. Release notes now record upstream tag/commit,
  Nimbus tag/commit, paired libkrunfw tag, asset SHA-256 values, and
  verification commands.
- `nimbus/nimbus-crun` release `v1.27.1-nimbus.2` exists with amd64/arm64
  binaries and `checksums.txt`. Release notes now record upstream tag/commit,
  Nimbus tag/commit, paired libkrun tag, asset SHA-256 values, and verification
  commands.
- `nimbus/deno` `v2.8.0-nimbus.2` is source-only for Cargo consumption and has
  no GitHub release asset requirement.
- `nimbus/bun` `nimbus-bun-jsc-proof-main-20260525` remains a proof-main source
  checkpoint, not a release-standard Bun fork, and has no standalone GitHub
  release asset requirement.

### FUS7 - End-to-End Consumer Verification

Status: completed 2026-05-25

Run the consumer gates that prove the standardized forks work as product inputs:

- `bash scripts/verify-fork-upstream-standardization.sh`.
- Deno/V8 focused runtime tests in Nimbus.
- Bun adapter verification gate for the chosen Bun posture.
- Linux packaging render and install helper tests for crun/libkrun:
  `bash scripts/verify-install-helper.sh`,
  `bash scripts/verify-build-linux-release-packages-helper.sh`,
  `bash scripts/verify-build-fedora-release-srpms-helper.sh`, and
  `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`.
- `cargo fmt --all --check`.
- `make check`.
- `make clippy`.
- `npm run typecheck`.
- `npm run test`.
- `npm run build`.

Success criteria:

- Each command reports pass/fail with captured output.
- Any skipped command has a concrete blocker and a follow-up action.
- minicloud is used for Linux-only verification where macOS cannot prove the
  packaging/runtime behavior.

Evidence:

- `bash scripts/verify-fork-upstream-standardization.sh --offline`: passed.
- `bash scripts/verify-fork-upstream-standardization.sh`: passed after FUS6
  default-branch alignment; `origin_head` now reports the active Nimbus branch
  for all five fork repos.
- Deno/V8 focused runtime tests:
  - `cargo test -p nimbus-runtime locker -- --nocapture`: runtime library
    filters reported 4 passed, 0 failed, 4 ignored; `tests/locker_smoke.rs`
    reported 5 passed, 0 failed.
  - `cargo test -p nimbus-runtime warm_pooled_runtime -- --nocapture`: 1
    passed, 0 failed.
- Bun/JSC focused gates:
  - `cargo test -p nimbus-runtime backends::bun_jsc --lib -- --nocapture`: 10
    passed, 0 failed.
  - `bash scripts/verify-bun-jsc-adapter-package-helper.sh`: passed negative
    and positive package validation cases.
  - `bash scripts/verify-bun-jsc-release-assets-helper.sh`: passed absent,
    positive, and tamper/unknown-platform rejection cases.
  - `make verify-bun-jsc-runtime-contract`: first sandboxed run failed because
    server diagnostics tests could not bind a local listener; unsandboxed rerun
    passed all 7 steps with 39 Rust tests plus 5 UI tests.
- Linux package and install gates:
  - `bash scripts/verify-install-helper.sh`: 35 tests passed.
  - `bash scripts/verify-build-linux-release-packages-helper.sh`: rendered
    deterministic nimbus/nimbus-libkrun/nimbus-crun/nimbus-bun-jsc-adapter
    deb/rpm manifests; local package build skipped because `nfpm` is not
    installed.
  - `bash scripts/verify-build-apt-repository-helper.sh` on Debian 13
    `minicloud`: signed apt metadata built and verified via local
    `apt-ftparchive`.
  - `bash scripts/verify-build-fedora-release-srpms-helper.sh` on Debian 13
    `minicloud`: Fedora 42 SRPM builder produced reusable source RPMs,
    installed x86_64 RPMs, and built/query-verified aarch64 RPM metadata.
  - `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`: passed.
- Broad gates:
  - `cargo fmt --all --check`: passed.
  - `git diff --check`: passed.
  - `make check`: passed; workspace check finished in 45.67s.
  - `make clippy`: passed; workspace clippy finished in 29.98s with
    `-D warnings`.
  - `npm run typecheck`: passed. Existing TanStack route-generation warnings
    about non-route helper files remained warnings.
  - `npm run test`: passed, including `nimbus-ui` 42 files / 278 tests.
  - `npm run build`: passed. Existing route-generation/code-splitting and
    chunk-size warnings remained warnings.
- `npm run docs:validate-refs:strict`: unavailable; npm reported the script is
  missing. This is not a FUS7 required gate, but it remains useful future
  documentation tooling.

### FUS8 - Closeout

Status: completed 2026-05-25

Close the plan only after:

- all five fork repos are clean locally;
- all expected branches/tags are pushed;
- Nimbus root consumes the standardized tags;
- installers and package workflows consume the standardized crun/libkrun tags;
- active docs no longer teach `locker` as the naming standard;
- the verification script/inventory gate is committed so future agents can
  detect drift.

Evidence:

- All five fork repos were clean in the final networked
  `scripts/verify-fork-upstream-standardization.sh` run.
- Expected branches/tags were pushed and GitHub default branches now point at
  the active Nimbus branch for each fork.
- Nimbus root consumes standardized Deno/rusty_v8 tags in `Cargo.toml` and
  `Cargo.lock`.
- Nimbus installers, Linux package helpers, apt/COPR helpers, VMM helper, and
  active distribution/security docs consume `nimbus-libkrun`
  `v1.18.1-nimbus.1` and `nimbus-crun` `v1.27.1-nimbus.2`.
- Active docs describe `locker` branch/tag names as historical evidence only;
  no active code/package surface pins a `*-locker.*` tag.
- `scripts/verify-fork-upstream-standardization.sh` is part of this baseline so
  future fork drift is mechanically detectable.

## Goal Prompt

This goal is active for the current autonomous execution run:

```text
/goal Complete docs/plans/fork-upstream-standardization-plan.md autonomously.

Verifiable success criteria:
- Implement FUS0-FUS8 in order, updating each item status in the plan as work
  completes.
- Normalize remotes for nimbus/deno, nimbus/rusty_v8, nimbus/bun,
  nimbus/nimbus-crun, and nimbus/nimbus-libkrun so origin is the Nimbus fork
  and upstream is the official upstream.
- Replace active locker branch/tag standards with nimbus branch/tag standards
  while preserving historical tags and archived proof logs.
- Rebase/port the Deno family to Deno v2.8.0 and its declared rusty_v8
  v149.0.0 pair, including or equivalently proving upstream rusty_v8 teardown
  fix e5abf2b.
- Build/test a mostly-upstream Deno v2.8.0 candidate before reapplying old
  Nimbus Deno patches, and drop any patch now covered by upstream.
- Resolve Bun's release-baseline decision so Nimbus does not present a mainline
  proof commit as an upstream release-derived fork.
- Rebase Bun proof work only onto a clearly named proof-main branch when the
  needed Rust/embedder surfaces are not in an official upstream release.
- Rebase/port nimbus-libkrun to upstream v1.18.1 and keep nimbus-crun aligned
  with upstream 1.27.1 and the new libkrun release, including a new
  `v1.27.1-nimbus.N` tag when the crun paired-libkrun contract changes.
- Produce per-fork port logs that list dropped, retained, and upstream-replaced
  Nimbus deltas with verification evidence.
- Update Nimbus Cargo pins, installer/package defaults, active docs, manifests,
  and verification fixtures to consume the standardized Nimbus tags.
- Push required branches/tags/releases only after local verification passes.
- Run and report the required verification gates from FUS7, including the fork
  inventory script, install/package helper gates, and minicloud for Linux-only
  checks where needed.
- Leave unrelated dirty user files untouched.
```
