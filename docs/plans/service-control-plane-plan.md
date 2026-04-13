# Plan: Service Control Plane

Canonical plan for Neovex's developer and operator service control plane for
Compose-declared sandbox-backed services.

Builds on `microvm-runtime-plan.md` M1 through M4 and takes ownership of the
remaining M5 architectural work: project identity, durable state ownership,
control-root layout, and lifecycle command semantics for
`neovex service ...`.

---

## Status

- **Status:** `in_progress`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-13 after:
  - `microvm-runtime-plan.md` M4 reached `done`
  - local source review completed for:
    - `containers/podman`
    - `containers/podman-compose`
- **Related plans:**
  - `docs/plans/microvm-runtime-plan.md` — owns the krun microVM backend,
    lifecycle probes, runtime integration, and the already-landed first M5
    Compose translation slice
  - `docs/plans/vmm-infrastructure-plan.md` — owns the patched-crun,
    conmon, and host-integration baseline
  - `docs/plans/distribution-plan.md` — owns macOS machine-VM delivery and
    packaging shape

---

## Why This Needs Its Own Plan

The remaining M5 work is no longer just "add more CLI commands." It now has
its own architectural questions:

- what is the durable source of truth for service lifecycle state?
- how do `neovex service ...` commands identify a project and its services?
- how do `neovex --compose-file ...` and `neovex service ...` share one control
  plane instead of becoming parallel systems?
- which Docker/Podman behaviors should Neovex copy, and which should it
  explicitly reject?

Those questions are large enough that they should not remain buried as one
phase inside the broader microVM runtime plan.

---

## Current Assessed State

- `neovex-bin` already owns the first Compose translation seam in
  `crates/neovex-bin/src/service/compose.rs`.
- `neovex service config` already parses and validates a supported Compose
  subset and lowers it into a typed `SandboxServiceCatalog`.
- `crates/neovex-bin/src/service/project.rs` now owns the first explicit
  project/control-root seam. It derives a deterministic Compose project key
  from the validated project name plus the canonical compose file path, derives
  a local service tenant id from that key, and materializes a project-scoped
  krun backend root under
  `<control_data_dir>/services/projects/<project_key>/backends/krun/`.
- `crates/neovex-sandbox/src/backends/krun/state.rs` now owns the first
  backend-owned persisted-state lookup seam. `KrunSandboxStateView` reads krun
  manifests from the project-scoped backend root and exposes typed surfaces for
  project listing, inspect-by-service identity, and log-path lookup without
  introducing a separate CLI lifecycle database.
- `neovex --compose-file ...` now reuses that same control-plane derivation for
  the server path instead of constructing the krun backend with the generic
  `/tmp/neovex-sandbox` default.
- `neovex service config`, `up`, `down`, `list`, `inspect`, `logs`, and `ps`
  now all exist locally in `crates/neovex-bin/src/service/`.
- `neovex service up` reparses Compose through the same
  `ComposeProjectContext`, derives the same deterministic project key / local
  tenant / project-scoped krun backend root as `--compose-file`, and starts
  services through the same `SandboxServiceCatalog` launch bridge used by the
  server path. If the current persisted service identity is still active,
  `up` returns `already_running` instead of launching a duplicate sandbox.
- `neovex service down` resolves current service identity from backend-owned
  persisted state, dedupes historical manifest history per service name, and
  stops the active sandbox through the generic backend `stop()` seam. If the
  resolved service identity is already terminal, `down` returns
  `already_stopped`.
- The remaining gap is now end-to-end proof and operator evidence, not control
  plane ownership. Linux-host compose-backed verification and recovery drills
  still need to be recorded back into this plan and
  `microvm-runtime-plan.md`.

---

## Source-Backed Findings

### Compose format

- Docker and Podman both target the Compose Spec.
- `compose.yaml` remains the right config input format for Neovex.

### Podman

- `podman compose` is not Podman's native lifecycle engine. It is a thin
  wrapper around an external compose provider:
  - `/Users/jack/src/github.com/containers/podman/cmd/podman/compose.go`
- Podman's native lifecycle control plane is `libpod` runtime state plus
  runtime-owned persistent files:
  - `/Users/jack/src/github.com/containers/podman/libpod/runtime.go`
  - `/Users/jack/src/github.com/containers/podman/libpod/sqlite_state.go`
  - `/Users/jack/src/github.com/containers/podman/pkg/domain/infra/abi/containers.go`
  - `/Users/jack/src/github.com/containers/podman/libpod/container_config.go`
  - `/Users/jack/src/github.com/containers/podman/libpod/runtime_ctr.go`
  - `/Users/jack/src/github.com/containers/podman/libpod/container_log.go`
  - `/Users/jack/src/github.com/containers/podman/libpod/container_inspect.go`
- Podman's native declarative Linux service layer is Quadlet/systemd, not
  Compose:
  - `/Users/jack/src/github.com/containers/podman/docs/source/markdown/podman-systemd.unit.5.md`
  - `/Users/jack/src/github.com/containers/podman/docs/source/markdown/podman-quadlet.1.md`

### podman-compose

- `podman-compose` is a Python Compose implementation on top of the Podman CLI:
  - `/Users/jack/src/github.com/containers/podman-compose/README.md`
  - `/Users/jack/src/github.com/containers/podman-compose/podman_compose.py`
- It is useful as a **behavioral reference** for:
  - compose-file discovery rules
  - project-name precedence
  - `.env` / `COMPOSE_*` precedence
  - multi-file merge/include behavior
  - project/service labels and config-hash ideas
- It is **not** a good runtime dependency for Neovex because:
  - it is a separate Python process and CLI wrapper
  - it orchestrates Podman containers and networks, not Neovex sandboxes
  - its durable lookup model is label-and-`podman ps` based, while Neovex
    already has backend-owned manifests and logs

---

## Architectural Decisions

1. **Compose stays the input format.**
   Neovex continues to accept `compose.yaml` as the user-facing service
   definition format.

2. **Neovex does not depend on `podman-compose` at runtime.**
   We may use it as a source reference, but not as an execution dependency or
   subprocess wrapper.

3. **Backend-owned persisted sandbox state is the lifecycle source of truth.**
   `neovex service up/down/list/logs/inspect` must resolve against backend-owned
   manifests, logs, and state roots. Do not add a second CLI-owned lifecycle
   state file or project database.

4. **`neovex service ...` and `neovex --compose-file ...` must share one
   lowering pipeline and one project identity model.**
   The CLI and server path may differ in execution mode, but not in how they
   derive service definitions, project identity, backend roots, or service
   names.

5. **Project identity must be deterministic and collision-resistant.**
   Neovex should follow Compose-style project-name precedence, but it must also
   disambiguate same-named projects from different directories. The working
   design target is:
   - human-facing project name: normalized Compose project name
   - durable project key: project name plus stable hash of the canonical
     compose root / config path set
   - deterministic local tenant identity derived from that project key

6. **Control roots belong under the Neovex control-plane directory, not `/tmp`.**
   For local developer workflows, backend state roots should live under a
   deterministic project-scoped subtree of `control_data_dir`, then fan out
   into backend-specific directories such as krun manifests, bundles, and logs.

7. **Logs and inspect output must be backend-first.**
   `neovex service logs` should tail backend-owned persistent logs.
   `neovex service inspect` should surface a typed, backend-owned summary built
   from manifest/runtime state, not raw YAML or ad hoc CLI caches.

8. **Podman alignment is selective.**
   Copy:
   - Compose input compatibility
   - deterministic project identity rules where they fit
   - backend-owned lifecycle truth
   - typed inspect/log/list surfaces

   Do not copy:
   - `podman compose` as a wrapper model
   - Podman network/DNS assumptions that do not apply to TSI
   - Podman pods as the primary ownership primitive

9. **OS-native service managers are future adapters, not the source of truth.**
   If Neovex later emits systemd, Quadlet, or launchd artifacts, they should be
   generated from the same service control plane, not replace it.

---

## Project Identity Contract

Current implemented precedence in `crates/neovex-bin/src/service/compose.rs`
and `project.rs`:

1. top-level Compose `name:` after Neovex sanitization
2. otherwise the compose file's parent directory name
3. otherwise the compose file stem
4. otherwise the literal fallback `neovex`

Current implemented derivation rules:

- project name sanitization lowercases ASCII alphanumerics and rewrites all
  other characters to `-`, then trims leading/trailing `-`
- project key = `<project-name-slug>-<12 hex chars of sha256(canonical compose file path)>`
- local tenant id = `svc-<project-key>`
- project root =
  `<control_data_dir>/services/projects/<project_key>/`
- krun backend root =
  `<control_data_dir>/services/projects/<project_key>/backends/krun/`

Current non-goals:

- no separate CLI-owned lifecycle registry file
- no path-agnostic project key that could collide across different compose
  roots with the same project name
- no hidden alternate control root under `/tmp`

---

## Proposed Control-Plane Shape

### Stable inputs

- `compose.yaml` and related included files
- CLI flags / environment
- current backend manifests and logs

### Deterministic derived identifiers

- project name
- project key
- local tenant id
- per-service sandbox identity

### Durable backend roots

Target direction:

```text
<control_data_dir>/
  services/
    projects/
      <project_key>/
        backends/
          krun/
            containers/
            bundles/
            logs/
            helpers/
```

The backend may own additional files under that root, but lifecycle truth lives
in backend-owned state, not in a separate CLI-owned registry file.

### CLI resolution model

- `neovex service config` parses and validates Compose
- `neovex service up/down/...` reparses Compose, derives the same project key,
  and resolves service identities deterministically
- lifecycle commands then discover current state from the backend root

That keeps the Compose file authoritative for desired configuration while the
backend remains authoritative for current runtime state.

---

## Roadmap

## Roadmap Status Ledger

| Slice | Status | Notes |
|---|---|---|
| SCP1: project identity + control-root contract | `done` | project-name precedence, project-key / local-tenant derivation, and the project-scoped backend-root contract are now documented here and implemented in `crates/neovex-bin/src/service/project.rs` |
| SCP2: backend-owned summary/lookup seams | `done` | `KrunSandboxStateView` landed in `crates/neovex-sandbox/src/backends/krun/state.rs` with project listing, inspect-by-service identity, and log-path lookup over manifest-backed krun state |
| SCP3: lifecycle commands | `done` | `config`, `up`, `down`, `list`, `inspect`, `logs`, and `ps` now exist locally; `down` resolves one current target per service identity instead of fanning out across raw manifest history |
| SCP4: CLI/server ownership unification | `done` | main server path plus explicit lifecycle commands now share `ComposeProjectContext`, project-scoped backend roots, and the `SandboxServiceCatalog` lowering bridge without a duplicate lifecycle database |
| SCP5: end-to-end proof and operator docs | `in_progress` | Initial Debian 13 proof was recorded on 2026-04-13, but post-review hardening found the checked-in recovery helper could validate stale manifests/logs from the shared workdir and could miss orphaned conmon/crun processes by grepping the host port instead of sandbox identity. The helper now targets the exact current-run project root and sandbox ids; rerun on Linux is required before calling SCP5 closed again |

---

### SCP1: Ratify project identity and control-root contract

Deliverables:

- documented precedence for project naming
- documented algorithm for project key / local tenant id derivation
- documented backend root layout under `control_data_dir`
- explicit rejection of a CLI-owned lifecycle state file

### SCP2: Add backend-owned summary and lookup seams

Deliverables:

- typed krun/backend summary surface for:
  - list by project
  - inspect by service identity
  - log path / log streaming lookup
- tests for manifest-backed discovery and missing/orphan cases

### SCP3: Wire lifecycle commands onto the backend state model

Deliverables:

- `neovex service up`
- `neovex service down`
- `neovex service list`
- `neovex service logs`
- `neovex service inspect`
- `neovex service ps`

### SCP4: Unify server-path and CLI-path ownership

Deliverables:

- `neovex --compose-file ...` uses the same project/control-root derivation as
  `neovex service ...`
- no duplicate lifecycle bookkeeping between CLI and server path
- documented behavior for lazy activation vs explicit pre-start

### SCP5: End-to-end proof and operator docs

Deliverables:

- Linux-host compose-backed end-to-end evidence recorded back into
  `microvm-runtime-plan.md`
- CLI docs updated with the ratified control-plane semantics
- recovery drills for restart, orphan discovery, and log persistence

---

## Verification Contract

For this plan, do not call the architecture closed until all of the following
are true:

- the project identity and control-root rules are documented in-repo
- no CLI-owned lifecycle state file was introduced
- backend-owned list/log/inspect seams exist and are tested
- `neovex service ...` lifecycle commands use the same lowering and identity
  rules as `--compose-file`
- Linux-host end-to-end proof exists for the compose-backed runtime path

---

## Initial Recommendation

- Use `podman-compose` as a **reference implementation** for Compose parsing and
  project-shape edge cases.
- Do **not** use `podman-compose` as Neovex's runtime dependency or wrapper.
- Align Neovex's ownership model with Podman's native `libpod` approach:
  backend-owned state, typed inspect/log/list surfaces, and deterministic
  project identity.

---

## Execution Log

- 2026-04-13: Landed the first code slice for `SCP1` and part of `SCP4`.
  Added `crates/neovex-bin/src/service/project.rs` with a typed
  `ComposeProjectContext` / `ComposeProjectControlPlane` helper that:
  - loads the validated Compose project plan
  - canonicalizes the compose file path
  - derives a deterministic project key from the project name plus path hash
  - derives a local service tenant id from that project key
  - materializes a project-scoped krun backend root under
    `<control_data_dir>/services/projects/<project_key>/backends/krun/`
  - produces a krun backend config rooted there
  The main `--compose-file` server path now uses that helper instead of
  `KrunSandboxBackendConfig::default()`, so Compose-backed server startup no
  longer falls back to `/tmp/neovex-sandbox`. The Linux ignored smoke in
  `crates/neovex-bin/src/main.rs` was updated to exercise the same helper.
  Verification:
  - `cargo fmt --all --check`
  - `cargo check -p neovex-bin -p neovex-sandbox -p neovex`
  - `cargo test -p neovex-bin service::`
- 2026-04-13: Landed `SCP2`, the first backend-owned persisted-state lookup
  seam for krun. Added
  `crates/neovex-sandbox/src/backends/krun/state.rs` with a public
  `KrunSandboxStateView` that:
  - reads krun manifests under the project-scoped `state/containers/` root
  - lists typed sandbox summaries for the project
  - resolves inspect-by-service identity without assuming one manifest per
    service name, preferring live sandboxes before newer terminal manifests
  - returns backend-owned log paths from persisted conmon layout data
  - skips missing manifest directories without inventing a CLI-side registry
  Added focused tests for manifest-backed discovery, missing roots, and
  service-identity preference across multiple manifests for the same
  tenant/service pair. Verification:
  - `cargo fmt --all --check`
  - `cargo check -p neovex-sandbox -p neovex-bin -p neovex`
  - `cargo test -p neovex-sandbox krun::state -- --nocapture`
  - `cargo test -p neovex-bin service:: -- --nocapture`
- 2026-04-13: Started `SCP3` with the first non-mutating lifecycle command
  slice in `crates/neovex-bin/src/service/mod.rs`. Added:
  - `neovex service list [--all-tenants]`
  - `neovex service inspect <service> [--tenant <tenant-id>]`
  Both commands now derive the same project-scoped backend root as
  `--compose-file`, resolve persisted state through `KrunSandboxStateView`, and
  render typed YAML from backend-owned manifests instead of inventing a CLI
  registry. Current scoping rule:
  - default lifecycle lookups use the deterministic local project tenant id
    derived from the Compose project key
  - `list --all-tenants` shows all persisted sandboxes under the project root
  - `inspect --tenant ...` overrides the default local tenant
  Added focused tests that exercise the real compose-derived control root plus
  persisted manifest discovery for both the default local-tenant view and the
  explicit tenant override path. Verification:
  - `cargo fmt --all --check`
  - `cargo check -p neovex-sandbox -p neovex-bin -p neovex`
  - `cargo test -p neovex-bin service:: -- --nocapture`
  - `cargo test -p neovex-sandbox krun::state -- --nocapture`
- 2026-04-13: Extended `SCP3` with the first persisted log reader:
  - `neovex service logs <service> [--tenant <tenant-id>] [--follow]`
  The command resolves the same project-scoped backend root as the other
  lifecycle reads, selects the persisted sandbox for the requested
  tenant/service identity, and reads `ctr.log` directly from backend-owned
  conmon layout data. `--follow` uses a simple append-only polling loop over
  the persisted file instead of inventing a new log transport. Added focused
  tests for command parsing, tenant-aware log-path resolution, and incremental
  appended-byte reads. Verification:
  - `cargo fmt --all --check`
  - `cargo check -p neovex-sandbox -p neovex-bin -p neovex`
  - `cargo test -p neovex-bin service:: -- --nocapture`
- 2026-04-13: Extended `SCP3` with the first persisted process snapshot:
  - `neovex service ps <service> [--tenant <tenant-id>]`
  The command resolves the same project-scoped backend root as the other
  lifecycle reads, loads the selected sandbox from backend-owned persisted
  state, reads the persisted `pidfile` / `conmon.pid`, and layers a host `ps`
  snapshot over those PIDs when available. Added focused tests for command
  parsing, pidfile parsing, `ps` output filtering, and project-root-based PID
  snapshot rendering. Verification:
  - `cargo fmt --all --check`
  - `cargo check -p neovex-sandbox -p neovex-bin -p neovex`
  - `cargo test -p neovex-bin service:: -- --nocapture`
- 2026-04-13: Completed the remaining local `SCP3` lifecycle wiring and closed
  the `SCP4` ownership gap in `crates/neovex-bin/src/service/mod.rs`. Added:
  - `neovex service up [service] [--tenant <tenant-id>]`
  - `neovex service down [service] [--tenant <tenant-id>]`
  Both commands now reparse Compose through the same
  `ComposeProjectContext`, derive the same deterministic project key / local
  tenant / project-scoped krun backend root as `--compose-file`, and consume
  the same `SandboxServiceCatalog` lowering bridge as the server path.
  `service up` inspects backend-owned persisted state first and returns
  `already_running` when the current service identity is still active.
  `service down` resolves one current target per service identity from
  backend-owned manifests, deduping historical manifest history before stop or
  `already_stopped` reporting. Added focused tests for CLI parsing,
  manifest-history dedupe, and helper start/stop behavior. Verification:
  - `cargo fmt --all --check`
  - `cargo check -p neovex-sandbox -p neovex-bin -p neovex`
  - `cargo test -p neovex-bin service:: -- --nocapture`
- 2026-04-13: Ran initial SCP5 Linux-host verification on Debian 13 x86_64.
  Compose-serve verification via
  `bash scripts/verify-microvm-m5-compose-serve-helper.sh` passed (~9.5s):
  - V8 function `services:activate` returned `ctx.services.db.port = 18091`
  - BusyBox httpd responded on TSI host port 18091 (guest 8091)
  - tenant deletion stopped service and released port
  - state at `/tmp/neovex-sandbox-smoke/m5-compose-control/services/projects/smoke-app-444d8e7fafaa/`
  Recovery drill via `bash scripts/verify-microvm-m5-recovery-drill-helper.sh`
  also reported success and wrote
  `/tmp/neovex-sandbox-smoke/m5-recovery-drill/summary.txt`.
- 2026-04-13: Post-review hardening found two durability gaps in the initial
  SCP5 closeout:
  - the bin smoke had widened public API surface by making
    `RuntimeServiceRegistry` public even though the test could assert through
    the already-public `SandboxCatalog` surface on `SandboxServiceManager`
  - the recovery helper could validate stale manifests/logs from the shared
    `${NEOVEX_KRUN_SMOKE_WORKDIR}` and could miss orphaned conmon/crun
    processes by grepping the host port rather than the sandbox identity
  The current code now keeps `RuntimeServiceRegistry` internal again, records
  the exact current-run project root/key in the compose-serve helper, clears
  the M5 control root before the smoke run, and makes the recovery helper
  validate exact manifest/log/sandbox-id paths from that run instead of using
  `find ... | head -1`. Because the hardened recovery helper has not been
  rerun on Linux yet, SCP5 remains `in_progress` pending one more Linux-host
  verification pass.
