# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans
- `docs/plans/system-tenant-api-plan.md`
  - canonical execution plan for the `_nimbus` system tenant and management
    API: machine/service state persistence as documents, HTTP lifecycle
    endpoints, Convex function bundle with typed query surface, read/write
    path split; prerequisite for the desktop UI plan
- `docs/plans/desktop-ui-plan.md`
  - canonical execution plan for a Docker Desktop / Podman Desktop-style
    graphical interface: embedded React SPA at `/ui/*` via `rust-embed`,
    dashboard/machines/services/functions/data/logs/runs/settings tabs,
    dark mode, a11y, optional Electron shell (Phase 2); depends on the system
    tenant API plan and current architecture references below
- `docs/plans/install-script-plan.md`
  - canonical execution plan for the nimbus install script (Channel 1):
    `curl | sh` quick start for Linux (Debian/Ubuntu, Fedora/RHEL) and
    macOS (Apple Silicon). Covers platform detection, dependency
    installation, binary download, checksum verification, post-install
    verification helper, and the libkrun gap on Debian/Ubuntu.
- `docs/plans/bootc-machine-default-plan.md`
  - canonical execution plan for keeping the pinned Podman machine image as
    the current macOS default while proving a direct Fedora bootc-derived
    Nimbus machine image, promoting it only after macOS parity and bootc
    lifecycle evidence, then removing legacy FCOS/Podman-image reliance.

## Current Reference Baselines

Completed execution plans live under `docs/plans/archive/` and are not
enumerated here. Use current architecture and operating docs first; open
archived plans only when you need historical execution detail.

- `docs/architecture/sandbox/microvm-service-baseline.md`
  - concise current baseline for the landed krun-backed microVM runtime,
    service activation, Compose-backed `nimbus compose ...` surface, and the
    Linux-versus-macOS platform model
- `docs/architecture/sandbox/macos-machine-flow.md`
  - concise current reference for the settled macOS developer-machine contract:
    pinned Podman image digest, host-managed guest binary sync, forwarded
    machine API, host-resident `nimbus start`, and proof-helper entrypoints
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
    FCOS-derived candidate. Do not resume MOS3A from this plan; use
    `docs/plans/bootc-machine-default-plan.md` for current machine OS work.

## Pending plans

- `docs/plans/nimbus-rename-satellite-repos-plan.md`
  - prerequisite plan for renaming internals of satellite repositories
    (`machine-os`, `nimbus-crun`) and creating a new `nimbus/homebrew-tap`
    before the main repo rename: guest image paths, systemd units, OCI media
    types, OCI annotations, build scripts, workflow inputs, Homebrew cask, and
    cross-repo interface coordination
- `docs/plans/nimbus-rename-plan.md`
  - canonical execution plan for renaming the project from "nimbus" to "nimbus"
    and relocating all repositories from the `nimbus` GitHub organization
    to `nimbus`: GitHub repo transfers, Rust crate renames, JS package renames,
    CI/CD workflow updates, script renames, Makefile updates, config/doc bulk
    replacement, and verification; depends on the satellite repos plan above

## Deferred plans with defined scope

- `docs/plans/windows-machine-support-plan.md`
  - canonical execution plan for the Podman-aligned Windows developer-machine
    architecture, source-backed against the Podman WSL2 provider: Windows-native
    `nimbus.exe` with WSL2 machine provider, win-sshproxy named-pipe API
    forwarding, shell-script bootstrap (not ignition), WSL2-native networking
    (not gvproxy); activation gate is macOS MAC5+ stabilization

## Deferred design and experiment plans

- `docs/plans/distribution-plan.md`
  - canonical plan for distributing nimbus across all channels: install
    script, apt repo (Debian/Ubuntu), COPR (Fedora), Homebrew + machine VM
    (macOS via krunkit/libkrun), binary tarballs, container images, cloud
    VM images (AWS AMI, GCP). Channel 4 covers the macOS machine VM
    architecture (krunkit, guest image, control channel, virtiofs, gvproxy)
- `docs/plans/layered-admission-control-plan.md`
  - current owner of future layered admission-control and `EO8` promotion work;
    use it before promoting any new admission-control boundary
- `docs/plans/wasmtime-backend-plan.md`
  - canonical plan for adding a wasmtime-based WASM backend alongside the
    existing V8 backend (currently implemented via `deno_core`); covers
    backend abstraction refactor, WIT interface definitions, cooperative
    fuel-based scheduling, module caching, and bundle format extension;
    activation gate met (Locker fork Phase 5 completed 2026-04-06)
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
- For repo-wide reliability-proof posture or CI flake investigation, start
  with `docs/architecture/testing/reliability-posture.md` and
  `docs/architecture/testing/ci-failure-investigation.md`.
- For future cleanup or verification-hardening work that is not already owned
  by another active plan, author or promote a new active plan instead of
  reviving an archived one.
- For future wasmtime WASM backend work, start with
  `wasmtime-backend-plan.md`.
- For future agent OS capabilities via WASI Component Model, start with
  `wasi-agent-capabilities-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/plans/research/` for north-star architecture and background research,
  not execution sequencing.
