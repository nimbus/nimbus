# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/distribution-plan.md`
  - canonical plan for distributing nimbus across all channels: install
    script, apt repo (Debian/Ubuntu), COPR (Fedora), Homebrew + machine VM
    (macOS via krunkit/libkrun), binary tarballs, container images, cloud
    VM images (AWS AMI, GCP). Channel 4 covers the macOS machine VM
    architecture (krunkit, guest image, control channel, virtiofs, gvproxy).
    Activation gate met on 2026-04-13 (microVM service baseline `done`);
    binary release, Homebrew/cask, and Linux package mirror lanes are in
    flight under this plan.
## Current Reference Baselines

Completed execution plans live under `docs/plans/archive/` and are not
enumerated here. Use current architecture and operating docs first; open
archived plans only when you need historical execution detail.

- `docs/plans/archive/tenant-isolation-control-plane-plan.md`
  - completed execution record for making production tenant isolation explicit
    across compute, networking, storage, HostBridge/runtime grants, sandbox
    artifacts, volumes, images, secrets, cleanup, and resource quotas (closed
    2026-05-22). Added the server-owned `TenantIsolationContext`, tenant-owned
    sandbox roots and volumes, fail-closed network/image/secret admission,
    runtime grant/tier admission, storage/API tenant authorization, microVM
    resource quotas, and the two-tenant proof harness. Future tenant-isolation
    waves should start from current architecture docs plus this baseline and
    promote a new active plan before changing the contract.
- `docs/plans/archive/tenant-isolation-enterprise-hardening-plan.md`
  - completed follow-on execution record for turning the tenant-isolation
    control-plane baseline into an enterprise-grade trust surface (closed
    2026-05-22). Added typed admission decision records, PDP/PEP separation,
    reusable conformance, read-only drift/audit scanning, Linux hard-quota
    proof, stable workload identity, image provenance/signature policy,
    structured redacted audit events, and customer/operator readiness docs.
    Current posture starts from `docs/tenant-isolation.md`,
    `docs/operating/tenant-isolation.md`, and the architecture docs linked
    there.
- `docs/plans/archive/local-dev-canonicalization-plan.md`
  - completed execution record for the local-dev canonicalization wave
    (LD0-LD7, closed 2026-05-21). Made `nimbus-server`'s cross-toolchain
    dependency on the `nimbus-ui` JS build explicit and self-healing
    via the Makefile's UI dependency graph: `UI_DIST_INDEX` /
    `UI_CODEGEN_SENTINEL` are prereqs on every workspace lane that
    compiles `nimbus-server` (`check`, `test`, `clippy`, `ci-required`,
    `verify-desktop-ui`, `test-rust-workspace`, `test-rust-docs`).
    `crates/nimbus-server/build.rs` no longer emits a stub HTML and
    errors actionably when `packages/nimbus-ui/dist/index.html` is
    absent in any profile. `.github/workflows/ci.yml` dropped all seven
    inlined `npm run codegen` / `npm run build -w packages/nimbus-ui`
    steps and routes through the Make targets instead. Build contract
    documented at `docs/operating/local-dev.md`. Fresh-clone proof at
    `docs/plans/proof/local-dev-canonicalization/`. `/goal` control
    plane gated on `bash scripts/verify-local-dev-canonicalization.sh`
    (ten conditions). Future local-dev / build-graph waves must
    promote a new active plan.
- `docs/plans/archive/ci-caching-canonicalization-plan.md`
  - completed execution record for the CI caching canonicalization wave
    (CC0-CC8, closed 2026-05-22). Layered sccache (per-rustc-call
    content-addressed cache via `mozilla-actions/sccache-action`) on
    top of the existing Swatinem floor across every Rust job in
    `.github/workflows/ci.yml`, `.github/workflows/desktop-ui.yml`, and
    `.github/workflows/node-compat-nightly.yml`. Rotated all Swatinem
    `shared-key` values `-v1 → -v2` to force a clean dep tree, added
    `save-always: true` for rerun-safe saves, and gated `save-if` on
    `refs/heads/main` so PRs cannot poison the main-branch pool.
    Inserted two leader jobs: `ui-artifacts` (one canonical SPA build
    consumed by harness + coverage via `actions/download-artifact@v7`)
    and `warm-sccache` (serial cold-start that populates the sccache
    pool before harness/coverage fan out). Kept the `cargo llvm-cov
    -j 1` constraint from LD7 unchanged (sccache shortens compile
    time, not link time; rust-lld bus-error risk unchanged) and added
    `--no-doc-tests` to the Coverage `cargo llvm-cov` invocation
    (doc-tests still execute via `make test-rust-docs` in
    `rust-workspace-tests`; CC7 only drops coverage measurement).
    Canonical caching contract is `docs/operating/ci-caching.md`.
    `/goal` control plane gated on
    `bash scripts/verify-ci-caching-canonicalization.sh` (twelve
    conditions). Proof bundle at
    `docs/plans/proof/ci-caching-canonicalization/`. Future CI caching
    / sccache / Swatinem waves must promote a new active plan.
- `docs/plans/archive/sandbox-microvm-hardening-plan.md`
  - completed execution record for closing the microVM service exposure
    blockers from `docs/plans/security/sandbox-isolation-audit.md`: krun OCI
    seccomp, explicit capabilities, `noNewPrivileges`, TSI bind-address
    carry-through, patched-crun port-map parser robustness, and Debian 13
    Linux localhost-only proof. Covered SMH0-SMH4, closed 2026-05-21. Future
    distribution packaging must pin and ship the validated patched
    crun/libkrun stack or rerun the same smoke proof for a replacement stack.
- `docs/plans/archive/nimbus-libkrun-runtime-stack-plan.md`
  - completed execution record for shipping the patched Linux krun service
    stack from Nimbus-owned release artifacts (closed 2026-05-21). Created
    `nimbus/nimbus-libkrun`, published `v1.17.4-nimbus.1`, paired
    `nimbus-crun` `v1.27.1-nimbus.1`, updated direct install/uninstall/verify,
    Linux package builders, apt/COPR mirror workflows, VMM helpers, and docs,
    and closed with Debian 13 root VMM localhost-only smoke plus Fedora 42
    package proof. Future krun stack bumps should promote a new active plan.
- `docs/plans/archive/runtime-engine-seam-plan.md`
  - completed Step 0 runtime-extension baseline (RS0-RS6, closed
    2026-05-21). Defined `WorkerLoop` / `RuntimeBackend` as the execution seam,
    split the JavaScript Nimbus context contract from Deno-op transport, made
    engine/content/compatibility/package metadata explicit, added the new
    engine proof-harness contract, and recorded Bun/JSC proof gates 1-10
    through generated-program timeout/cancel. Future execution-boundary work
    starts from
    `docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`.
- `docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`
  - completed execution-boundary control plane (EIB0-EIB7, closed
    2026-05-21). Built the ownership map across runtime, sandbox, WASM/WASI,
    admission, and security-audit plans; defined shared trust tiers; kept
    Bun/JSC proof-only with no fork yet; routed sandbox audit findings; aligned
    wasmtime and WASI agent plans; recorded resource/admission promotion rules;
    and closed with the runtime/backend go/no-go matrix. Future implementation
    waves should promote focused active plans from this baseline.
- `docs/plans/archive/cli-daemon-canonicalization-plan.md`
  - completed execution record for the CLI daemon canonicalization wave
    (CD1-CD9, closed 2026-05-19). Removed source-tree walk-up from
    `nimbus start`; bounded `dev`/`deploy`/compose walkers at the
    nearest `.git/` (directory or worktree-shaped file); added a
    CockroachDB-shaped `operator console:` banner line on both daemon
    commands; added `--open` to `nimbus dev` (only); removed the
    legacy explicit-spawn flag from `nimbus ui`, leaving it as an
    unflagged discover-and-open command. Regression tests in
    `nimbus-bin` cover the rebrand-trap shape,
    the worktree boundary, the banner, and the `ServerDiscoveryRecord`
    serde golden contract shared with Electron and the Playwright
    fixture.
- `docs/plans/archive/install-script-plan.md`
  - completed execution record for the nimbus install script (Channel 1):
    POSIX `curl | sh` quick start for Linux (Debian/Ubuntu, Fedora/RHEL) and
    macOS (Apple Silicon). Covered I1-I5: platform detection, dependency
    installation, GitHub Releases binary download with SHA256 verification,
    macOS Homebrew cask install/upgrade with bundled `libexec/gvproxy`,
    uninstall, and the canonical hosted URL
    `https://github.com/nimbus/nimbus/releases/latest/download/install.sh`.
    Fresh-host proofs landed for Ubuntu 24.04, Debian 13, Fedora 42, and
    Apple Silicon macOS at `.install-script-proofs/`. Closed 2026-05-17;
    future install-script work must promote a new active plan.
- `docs/plans/archive/desktop-mission.md`
  - completed mission-completion record for the autonomous-mode control
    plane that bound Phase 1 + Phase 2 desktop work into a single mission.
    Closed 2026-05-16; durable authorizations, resume procedure, and stop
    condition preserved as historical reference.
- `docs/plans/archive/desktop-ui-plan.md`
  - completed execution record for Phase 1 of the operator console:
    embedded React SPA at `/ui/*` via `rust-embed`, dashboard/machines/
    services/functions/data/logs/runs/settings tabs, dark mode, a11y,
    DU0–DU10 + DU11 hardening. Consumed the `_nimbus` system-tenant
    surface and current architecture references below. Successor:
    `docs/plans/archive/desktop-ui-shell-overhaul-plan.md` (two-view
    shell, primary drawer, sub-drawer, tenant selector, active-tenant
    store).
- `docs/plans/archive/desktop-ui-shell-overhaul-plan.md`
  - completed execution record for the two-view operator console
    shell: Developer console at `/app/*` (Overview / Compute /
    Schedules / Storage / Files / Observability / Settings, tenant-
    scoped) and Operator console at `/admin/*` (System / Tenants /
    Machines / Network / Services / Observability / Settings, server-
    wide), with top-nav view switcher + collapsible primary drawer +
    contextual sub-drawer (static menu | dynamic list) + tenant
    selector + active-tenant Zustand store + `?as=` bootstrap. Covered
    O0–O8. Closed 2026-05-17; verification artifacts under
    `docs/plans/proof/desktop-ui-shell-overhaul/`. Promote new active
    plans before implementing real feature content for the placeholder
    Services / Files / Schedules surfaces.
- `docs/plans/archive/desktop-ui-compute-services-redesign-plan.md`
  - completed execution record for correcting two IA mistakes in the
    shell-overhaul plan: (1) Services moves from Operator-only to
    dual-persona (Developer `/app/services*` + Operator
    `/admin/services*`, parallel to Observability), sharing
    `ServicesTable`/`ServiceDoc` with a `showTenantColumn` toggle;
    (2) Compute stops being a kitchen sink — inner tabs deleted, the
    page becomes a Functions-only landing with a Convex-style
    hierarchical function tree in the sub-drawer, a per-function
    detail page at `/app/compute_/$function` with Statistics/Source/
    Logs/Runs tabs, and a docked Input/Output runner; the standalone
    `/app/compute/runner` route was deleted (pre-launch breaking
    change). Scheduled/Cron migrated out of Compute into
    `/app/schedules` with `?section=scheduled|cron`. Covered CS0–CS10.
    Closed 2026-05-18; proof bundle under
    `docs/plans/proof/desktop-ui-compute-services-redesign/`. Promote
    a new active plan before implementing real Code-refs (Services)
    or Drift (Operator) backends, or before wiring a Monaco/Shiki
    source viewer.
- `docs/plans/archive/desktop-ui-design-review-fixes-plan.md`
  - completed execution record for the 2026-05-18 operator-console
    design review cleanup. Closed 12 of 14 in-scope findings (16
    total; F8 already-fixed and F16 folded into F1; F15 theme-matrix
    smoke and F6-restore of Restarts/Density/Drift deferred to owning
    plans): copy hygiene + canonical `EmptyState`
    (F1, F13), lens gating ⌘\ Developer-only (F2), Observability +
    Schedules section truth (F3, F4), tenant default + ScopeChip
    cleanup (F5, F12), real shells on `/admin` index +
    `/admin/observability` (F10, F11), service-detail tab pruning to
    Placement-only (F6), and polish for breadcrumb / tab casing /
    sub-drawer grouping (F7, F9, F14). Covered DR0–DR8. Closed
    2026-05-18; proof bundle under
    `docs/plans/proof/desktop-ui-design-review-fixes/` (before vs
    after walks at 1440×900, plus DR2 lens-blocked evidence). Out of
    scope: theme-matrix smoke (F15) and re-adding service-detail
    Restarts/Density/Drift tabs — both return when their owning
    plans land.
- `docs/plans/archive/desktop-ui-followup-hardening-plan.md`
  - completed execution record for the post-design-review correctness
    wave on the operator console. Consumed a 2026-05-18 four-reviewer
    pass against the predecessor's after-state (~30 follow-up items: 1
    BLOCKER, 8 MAJORs, 13 MINORs, 7 NITs). Ratified the shipped dual-
    persona Services IA into `DESIGN.md` (H1, BLOCKER cleared);
    introduced `TenantScope` + `SubDrawerItem<TId>` discriminated
    unions and dropped the `extractTenantId` regex helper (H2);
    wired `LoadingValue<T>` envelopes through Overview / System /
    `/admin/tenants` 404 (H3); landed casing + breadcrumb +
    `EmptyState` + `/admin/network` default-section polish (H4); and
    closed the cleanup + spec-backfill items including the shared
    `fetchTenants` helper and the AbortController unmount-path test
    (H5). Covered H0–H7. Closed 2026-05-18; proof bundle under
    `docs/plans/proof/desktop-ui-followup-hardening/` (predecessor's
    sealed `after/` serves as this wave's `before/`; this wave's
    `after/` closes the predecessor's `/admin/services/$service`
    evidence gap and adds 10 new `h7-*` captures). Out of scope:
    F15 theme-matrix smoke and the Restarts/Density/Drift restore —
    both still return when their owning plans land.
- `docs/plans/archive/desktop-ui-architecture-hardening-plan.md`
  - completed execution record for the architecture-debt wave on the
    operator console. Consumed a 2026-05-18 post-closure
    critical-reflection review of the Followup-Hardening wave that
    surfaced six architecture items the H-wave intentionally did not
    touch. Covered A0–A8: codegen now emits per-export typed return
    surfaces (`{ default, _typed }`) so every `useQuery` returns the
    handler's declared shape directly and every `as never` / `as
    ServiceDoc[] | undefined` cast was deleted in the same wave (A1);
    `ServiceDoc` lifted to `lib/types/services.ts` with zero
    cross-persona route-to-route type imports (A2);
    `routes/admin/settings.tsx` decomposed from 1608 LOC to 93 LOC
    plus four concept-owned siblings under `routes/admin/settings/`
    (A3); five data routes hoisted to TanStack Router `loader` +
    `Route.useLoaderData()` (A4); Storybook component catalog with
    11 stories under `packages/nimbus-ui/src/stories/` (A5);
    `verify-desktop-ui` CI lane wired via playwright-cli with a
    10-step smoke walk plus zero-error console hygiene gate, and a
    long-standing CSP violation on the inline FOUC theme-resolution
    script fixed at the root by pinning its SHA-256 hash in `UI_CSP`
    with a compile-time drift test (A6); `routes/app/observability.tsx`
    split 978 → 180 LOC root + three siblings under
    `routes/app/observability/`, with `routes/app/storage_.$table.tsx`
    (1154 LOC) and `routes/admin/machines.tsx` (715 LOC) kept under
    the CLAUDE.md under-1500 acceptable band with explicit
    justification (A7). Closed 2026-05-18; proof bundle under
    `docs/plans/proof/desktop-ui-architecture-hardening/`. Four grep
    gates clean at close: `as never` 0 hits, `as ServiceDoc[] | undefined`
    0 hits, cross-persona `ServiceDoc` import under `routes/admin` 0
    hits, `routes/admin/settings.tsx` 93 LOC (well under the 900-LOC
    cap). Visual baseline points to the sealed `h7-*` bundle from
    the followup-hardening wave since A1–A7 are code/architecture-only
    changes. Out of scope continues: F15 theme-matrix smoke and the
    F6 Restarts/Density/Drift restore — both return when their owning
    plans land.
- `docs/plans/archive/desktop-ui-architecture-residue-plan.md`
  - completed execution record for the architecture-residue wave on the
    operator console — the second pass after the Architecture-Hardening
    wave. Promoted 2026-05-19 from a four-stream post-closure code review
    that surfaced three blockers the A2/A4 grep gates missed (vanity-gate
    `as unknown as CountQuery` residue in `shell/nav-entries.ts`, partial
    loader migration on `_.$service.tsx` siblings, un-specced codegen
    inference layer) plus ~12 cleanup items and four nits. Covered R0–R12:
    producer-side typed `QueryEntry<TArgs, TReturn>` wrapper retired the
    nine remaining `as unknown as` casts (R1); the three sibling-`useQuery`
    sites on `_.$service.tsx`-class routes and the two page-level
    `useQuery` calls on `compute_.runs_.$runId.tsx` folded into
    `Route.loader` (R2 + R4); codegen `isTrivialValidator` now unwraps
    `union(v.any(), v.null())`, `inferMutationResultType` throws on missing
    `plan.table`, `helperCall` audits convention-inferred fallbacks, and
    `JsonValue` collapsed to a single source in `dataModel.d.ts` (R3);
    loader-error envelope (`Route.errorComponent` + `EmptyState` retry) +
    spec coverage added to the four A4 routes (R5); shared `_filters.tsx`
    + `Th`/`Td` primitives consolidate the observability + tenants
    duplication (R6); `routes/app/services{,_.$service}.tsx` switched
    from `useEffect → router.invalidate()` to `Route.loaderDeps` keying
    on `useUiStore.getState().activeTenant` (R7); A3 dead `dialogRef`
    declarations removed from `danger-zone.tsx` and `settings/sub-drawer.ts`
    adopted the typed-const `as const satisfies StaticSubDrawerSpec<…>`
    pattern (R8); `inline_fouc_script_hash_matches_csp` now asserts
    exactly one inline script and tolerates attribute-bearing tags, and
    the `desktop-ui.yml` workflow path filter widened to cover
    `Cargo.{toml,lock}` + `rust-toolchain*` (R9); smoke spec seeds one
    tenant + one service via `/api/tenants` + raw
    `/convex/_nimbus/mutation` (`Mutation::Insert`) so ScopeChip,
    services-table, and operator service-detail assertions fire
    unconditionally (R10); polish + nit pass (R11) collapsed the parallel
    `ObservabilityTab` unions, dropped debug-residue aria-live + unused
    re-exports, and added five Storybook variants
    (`copy-chip → ClipboardDenied`, `breadcrumb → LongPathTruncation`,
    `time → RelativeFutureSkew / RelativeFarPast / RelativeFarFuture`,
    `upgrade-popover → NotAvailable / StaleCheck / ErrorCheck`,
    `appearance-section → snapshot/restore on unmount`). Closed
    2026-05-19; proof bundle under
    `docs/plans/proof/desktop-ui-architecture-residue/` (`before.md`
    records R0 HEAD counts; `README.md` records R0–R12 →
    after-evidence mapping plus close-time grep-gate output). Eight
    grep gates clean at close: `as never` 0 hits, non-spec
    `as unknown as ` 0 hits, `as ServiceDoc … | undefined` 0 hits,
    cross-persona `ServiceDoc` import under `routes/admin` 0 hits,
    `type ServiceDoc` 1 hit (single canonical), `useQuery` in the three
    loaderized routes 0 hits, `router.invalidate` in `routes/app/services*.tsx`
    0 hits, `export type JsonValue` 1 hit in `_generated/`. Visual
    baseline inherits the H wave's sealed `h7-*` bundle since R1–R12
    are code/architecture-only changes.
- `docs/plans/archive/update-lifecycle-plan.md`
  - completed execution record for the operator-facing update lifecycle:
    server-side `/api/system/version-info` with stale-while-revalidate
    (UL1), SPA staleness UX in `packages/nimbus-ui/` (UL2),
    `nimbus-desktop` first-run "CLI not found" setup card + background
    `brew upgrade` runner + OS staleness notification (UL3), and
    operator-facing `docs/operating/updates.md` (UL4). Closed 2026-05-16.
    Current operator reference is `docs/operating/updates.md`; decision
    anchor is `docs/decisions/001-update-staleness-detection.md`.
- `docs/plans/archive/desktop-shell-plan.md`
  - completed execution record for Phase 2 of the operator console:
    signed, notarized, auto-updating Electron 42.x desktop shell in
    `nimbus/desktop` wrapping the embedded `/ui/*` SPA. Covered DS0A
    through DS10: external credentials, repo scaffold, server discovery/
    lifecycle, Electron Fuses + IPC security baseline, native chrome
    (tray/menu/window), auto-update, per-platform packaging, packaged
    E2E, code signing, release CI, and operator/security docs. DS7 / DS8
    / DS9 macOS re-verification deferred to first real `v0.x` release
    per the in-tree §"External feedback loops" disposition.
- `docs/plans/archive/brand-system-plan.md`
  - completed execution record for the two-tier brand system rollout
    across `nimbus/nimbus` and `nimbus/desktop`: 9-variant brand palette
    (Brand tier, gradients permitted, marketing surfaces) layered cleanly
    over the operator console's Industrial Precision Product tier (single
    teal accent, OKLCH 240° neutrals, no gradients). Covered L0–L9:
    canonical logo SVG + tight mark + 9 variants, DESIGN.md brand section,
    favicon, sidebar mark, desktop app icon, tray refresh,
    `cli-not-found.html` token migration, and idempotent
    `gen-variants.sh`. Closed 2026-05-16.
- `docs/plans/archive/bootc-machine-default-plan.md`
  - completed execution record for BMD0-BMD7: direct Fedora bootc machine-os
    recipe ownership, build artifact proof, bootc-native machine-config,
    macOS parity, bootc lifecycle, default promotion to
    `ghcr.io/nimbus/machine-os:v0.1.30@sha256:f565...`, and legacy
    Podman/FCOS demotion to explicit diagnostic/repair overrides. Closed
    2026-05-16; promote a new active plan for future machine OS work.
- `docs/plans/archive/system-tenant-api-plan.md`
  - completed execution record for ST1-ST4: `_nimbus` system tenant bootstrap,
    server-owned machine/service/network projections, local-admin lifecycle
    endpoints, packaged `_nimbus` Convex query bundle, read/write split, and
    verification evidence for the desktop UI prerequisite gate
- `docs/architecture/sandbox/microvm-service-baseline.md`
  - concise current baseline for the landed krun-backed microVM runtime,
    service activation, Compose-backed `nimbus compose ...` surface, and the
    Linux-versus-macOS platform model
- `docs/architecture/sandbox/macos-machine-flow.md`
  - concise current reference for the settled macOS developer-machine contract:
    pinned Nimbus bootc machine image digest, bootc-native machine-config,
    forwarded machine API, host-resident `nimbus start`, explicit legacy
    Podman/FCOS diagnostic overrides, and proof-helper entrypoints
- `docs/architecture/runtime/adapter-boundary.md`
  - current runtime and adapter ownership boundary
- `docs/architecture/runtime/permission-model.md`
  - current runtime permission-mode, grant, language, compatibility-target,
    and preset baseline
- `docs/architecture/server/auth-runtime-trust.md`
  - current server-owned auth and runtime trust boundary
- `docs/architecture/runtime/node-compat-surface-matrix.md`
  - current Node compatibility support matrix and evidence pointers
- `docs/adapters/convex/compatibility.md`
  - current Convex adapter compatibility contract
- `docs/plans/archive/machine-os-adoption-plan.md`
  - superseded evidence plan for MOS0-MOS2 and the abandoned MOS3A
    FCOS-derived candidate. Do not resume MOS3A from this plan; the
    bootc-native default lives in
    `docs/plans/archive/bootc-machine-default-plan.md` as completed
    implementation baseline. Future machine OS work must promote a new
    active plan.

## Deferred plans with defined scope

- `docs/plans/windows-machine-support-plan.md`
  - canonical execution plan for the Podman-aligned Windows developer-machine
    architecture, source-backed against the Podman WSL2 provider: Windows-native
    `nimbus.exe` with WSL2 machine provider, win-sshproxy named-pipe API
    forwarding, shell-script bootstrap (not ignition), WSL2-native networking
    (not gvproxy); activation gate is macOS MAC5+ stabilization

## Deferred design and experiment plans

- `docs/plans/layered-admission-control-plan.md`
  - current owner of future layered admission-control and `EO8` promotion work;
    use it before promoting any new admission-control boundary
- `docs/plans/wasmtime-backend-plan.md`
  - canonical plan for adding a wasmtime-based WASM backend alongside the
    existing V8 backend (currently implemented via `deno_core`); covers
    backend abstraction refactor, WIT interface definitions, cooperative
    fuel-based scheduling, module caching, and bundle format extension;
    activation gate met (Locker fork Phase 5 completed 2026-04-06), but
    runtime-engine seam hardening now owns the next Step 0 before promotion
- `docs/plans/wasi-agent-capabilities-plan.md`
  - canonical plan for adding agent OS primitives (virtual filesystem, sandboxed
    process execution, HTTP client) via WASI Component Model interfaces; covers
    `nimbus:agent` WIT package, `AgentOsProvider` trait, capability-based tenant
    admission, and agent-os sidecar integration; activates after the wasmtime
    backend plan W3 completes
- `docs/plans/native-transport-evolution-plan.md`
  - proposed follow-on plan for Nimbus-native transport evolution: shared
    session and codec seams, benchmark-driven optional binary codec work, and
    optional WebTransport evaluation without re-owning the established
    WebSocket protocol or Firebase transport work.
- `docs/plans/agent-browser-service-plan.md`
  - canonical deferred plan for adding a first-class `ctx.browser`
    session resource to Nimbus: `BrowserProvider` trait, single-Chrome-
    many-BrowserContexts shape, sandboxed-Chrome production provider
    behind `nimbus-sandbox`, durable Playwright-compatible storage state
    in `nimbus-storage`, warm pool, extraction cache, mutation-journal
    integration, and per-tenant capability admission paired with
    `wasi-agent-capabilities-plan.md`. North-star research at
    `docs/plans/research/agent-browser-service-prior-art.md`.
- `docs/plans/secret-management-plan.md`
  - canonical deferred plan for a tenant-scoped secret-management
    surface in Nimbus: `SecretProvider` trait + URI-shaped references
    + capability-gated `ctx.secret.*` host bridge + `_nimbus.secrets`
    Nimbus-native storage with KMS-DEK-wrapped values + multi-backend
    routing via `_nimbus.secret_stores` + cache invalidation via
    iroh-gossip. MVP provider set: NimbusNative, File (SOPS-compatible),
    AWS Secrets Manager, GCP Secret Manager, Azure Key Vault, HashiCorp
    Vault, Kubernetes Secrets. Activates when any of the browser plan
    Phase B5, the wasi-agent Phase A2, or a first paying tenant needs
    stronger-than-env-var credentials. Prior-art research at
    `docs/plans/research/secret-management-prior-art.md`. Supersedes
    the gap note `docs/plans/research/secret-management-shape.md`.
- `docs/plans/service-identity-provider-auth-plan.md`
  - canonical deferred plan for promoting stable workload identity into
    short-lived provider-auth credentials: enforced `identity` grants,
    OIDC/JWT minting, SPIFFE/SVID shape, mTLS/service-account tokens,
    node/machine identity binding, Vault/Kubernetes/AWS/GCP/Azure
    provider-auth adapters, lifecycle/revocation, and audit. Consumes
    `TenantWorkloadStableIdentity`; does not own secret values. Dependency
    review at
    `docs/plans/research/service-identity-provenance-dependency-audit.md`.
- `docs/plans/artifact-provenance-verification-plan.md`
  - canonical deferred plan for production artifact verification behind
    the landed `TenantImageVerificationProvider` seam: maintained OCI
    reference parsing, Cosign verification, SLSA provenance verification,
    SBOM evidence, offline/private-root verification, and extension from
    service images to runtime/function bundles and machine images. Nimbus
    owns policy evaluation and evidence normalization, not cryptographic
    verification. Dependency review at
    `docs/plans/research/service-identity-provenance-dependency-audit.md`.
- `docs/plans/enterprise-policy-and-sandbox-egress-plan.md`
  - canonical deferred plan for OpenShell-informed enterprise trust work:
    typed operator policy artifacts, built-in Rust policy evaluation,
    optional external policy backends such as OPA/Rego or Cedar, sandbox-local
    egress enforcement for process-capable microVM/browser/agent guests,
    dynamic versus recreate-required policy lifecycle, OCSF/OpenTelemetry
    security-event export, and future policy advisor/prover lanes. Research at
    `docs/plans/research/openshell-competitor-analysis.md`.

## Archive Policy

Completed plans are stored in `docs/plans/archive/` for historical review, but
this README intentionally does not catalog them. Use `rg` or `find` when you
need a specific historical record, and do not resume an archived plan unless
the work is explicitly a historical review.

## How To Use This Folder

- Start with the plan that owns your workstream.
- For broad maintainability, refactor, modularity, readability, canonical
  naming, idiomatic-Rust, or god-file cleanup work, start with
  `docs/architecture/testing/reliability-posture.md` and
  `docs/architecture/testing/ci-failure-investigation.md`, then promote a new
  active plan unless another active plan already owns the slice.
- For the landed krun-backed microVM and service-control architecture, start
  with `docs/architecture/sandbox/microvm-service-baseline.md` rather than
  opening the archived plans first.
- For current macOS developer-machine behavior, start with
  `docs/architecture/sandbox/microvm-service-baseline.md` and
  `docs/architecture/sandbox/macos-machine-flow.md`.
- Promote a new active plan before landing another machine/service CLI UX
  wave.
- Do not resume a plan from `docs/plans/archive/` unless you were explicitly
  asked to review historical work.
- If no active plan owns the work, promote or author a new active plan instead
  of reviving a completed archived one.
- For install-script follow-on work, start with the completed baseline at
  `docs/plans/archive/install-script-plan.md` and the active parent context
  in `docs/plans/distribution-plan.md`. Promote a new active plan before
  another install-script wave unless one already owns the slice.
- For Linux krun service stack packaging, `nimbus-libkrun`, or paired
  `nimbus-crun` release work, start with
  `docs/plans/archive/nimbus-libkrun-runtime-stack-plan.md`, then cross-check
  `docs/plans/distribution-plan.md`.
- For production tenant isolation across microVM services, in-process runtime
  grants, HostBridge access, storage namespaces, service networking, volumes,
  images, secrets, quotas, cleanup, enterprise auditability, admission
  decisions, conformance, or workload identity, start with
  `docs/tenant-isolation.md` and `docs/operating/tenant-isolation.md`, then
  cross-check
  `docs/plans/archive/tenant-isolation-enterprise-hardening-plan.md`,
  `docs/plans/archive/tenant-isolation-control-plane-plan.md`,
  `docs/plans/security/sandbox-isolation-audit.md`,
  `docs/architecture/sandbox/microvm-service-baseline.md`,
  `docs/architecture/runtime/permission-model.md`, and
  `docs/architecture/storage/provider-topologies.md`.
- For image signatures, SLSA provenance, SBOMs, OCI reference/referrer parsing,
  runtime bundle provenance, machine-image provenance, or any artifact
  verification code, start with
  `docs/plans/artifact-provenance-verification-plan.md`. Do not hand-roll
  cryptographic verification in Nimbus.
- For service identity, provider-auth exchange, OIDC/JWT minting,
  SPIFFE/SVIDs, mTLS certificates, service-account tokens, or promotion of the
  `identity` grant, start with
  `docs/plans/service-identity-provider-auth-plan.md`.
- For Convex or Nimbus CLI/codegen workflow work (`packages/codegen/`,
  `packages/convex/`, `demos/convex/`, or the `nimbus start --app-dir`
  contract), start with `docs/adapters/convex/ai-guidelines.md`,
  `docs/operating/cli.md`, and `docs/adapters/convex/compatibility.md`.
  Promote a new active plan before another CLI/codegen/facade architecture wave
  unless one already owns the slice.
- For encryption at rest work, start with
  `docs/architecture/storage/encryption.md` and `docs/operating/encryption.md`.
  Use the archived execution plan only for historical closeout detail.
- For Compose-backed service lifecycle follow-on work, start with
  `docs/architecture/sandbox/microvm-service-baseline.md`, then promote or author a new
  active plan if the task is larger than a small focused change.
- For microVM service security hardening, start with
  `docs/plans/archive/sandbox-microvm-hardening-plan.md` and
  `docs/plans/security/sandbox-isolation-audit.md`.
- For repo-wide reliability-proof posture or CI flake investigation, start
  with `docs/architecture/testing/reliability-posture.md` and
  `docs/architecture/testing/ci-failure-investigation.md`.
- For future cleanup or verification-hardening work that is not already owned
  by another active plan, author or promote a new active plan instead of
  reviving an archived one.
- For future execution-boundary work, including runtime engines, Bun/JSC,
  wasmtime, sandbox isolation, WASI agent capabilities, or admission/resource
  boundaries, start with
  `docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`; then
  promote a focused active plan for the implementation slice or use an existing
  backend-specific deferred plan such as `wasmtime-backend-plan.md` when its
  activation gate is met.
- For future agent OS capabilities via WASI Component Model, start with
  `wasi-agent-capabilities-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/plans/research/` for north-star architecture and background research,
  not execution sequencing.
