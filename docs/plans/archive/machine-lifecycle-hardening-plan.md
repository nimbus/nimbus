# Plan: Machine Lifecycle Hardening — Podman-Aligned Robustness

Canonical execution plan for hardening the nimbus machine lifecycle to match
Podman's source-backed robustness patterns. These items apply to the existing
macOS krunkit path and the future Windows WSL2 path, and were identified
through source-backed review of Podman's machine infrastructure.

Reviewed against:

- `docs/reference/microvm-service-baseline.md` — landed Linux production and
  service-control baseline that the developer-machine paths must preserve
- `crates/nimbus-bin/src/machine/manager.rs` — current krunkit+gvproxy
  orchestration
- `crates/nimbus-bin/src/machine/mod.rs` — machine config/state types,
  `write_json_file`
- `crates/nimbus-bin/src/machine/protocol.rs` — machine API protocol types
- `crates/nimbus-bin/src/machine/client.rs` — machine API client
- `crates/nimbus-bin/src/machine/backend.rs` — forwarded sandbox backend
- `crates/nimbus-bin/src/service/mod.rs` — `ServiceHostPlatform` and backend
  routing
- `docs/reference/macos-machine-flow.md` — current macOS developer-machine
  contract reference
- `docs/plans/archive/macos-machine-support-plan.md` — completed macOS
  execution record and closeout evidence
- `docs/plans/windows-machine-support-plan.md` — deferred Windows execution
  plan
- Podman source (source-backed review, not documentation):
  - `pkg/machine/shim/host.go` — `Start()` signal handling (lines 541-574),
    `stopLocked()` provider-owned stop sequence (lines 419-460)
  - `pkg/machine/shim/networking.go` — `conductVMReadinessCheck()` 3-layer
    readiness, `startNetworking()`/`PostStartNetworking` phase separation,
    SSH port reassignment
  - `pkg/machine/ports/ports.go` — `AllocateMachinePort`,
    `ReleaseMachinePort`, global file-locked port allocation (10000-65535),
    conflict reassignment
  - `pkg/machine/vmconfigs/machine.go` — `MachineConfig.Write()` atomic writes
    via `ioutils.AtomicWriteFile`, `lockfile.LockFile` for exclusive access,
    and config compatibility checks on load
  - `pkg/machine/vmconfigs/config.go` — `MachineConfigVersion`,
    `VMProvider` interface, `UseProviderNetworkSetup()`,
    `RequireExclusiveActive()` capability flags
  - `pkg/machine/wsl/stubber.go` — `StopVM()` graceful systemd exit before
    distribution termination (lines 236-274, 60-second timeout)
  - `pkg/machine/apple/vfkit/helper.go` — Apple/libkrun stop wait and
    hard-stop fallback (`Helper.Stop`)
  - `pkg/machine/apple/apple.go` — `StartGenericAppleVM()` return signature
    `(releaseFunc, readyWaitFunc, error)`

---

## Status

- **Status:** `archived-complete`
- **Primary owner:** archived historical record
- **Activation gate:** promoted to the active shared machine-hardening control
  plane on 2026-04-16 by explicit enterprise-readiness direction, then
  administratively closed and archived on 2026-04-18 after `MLH1` through
  `MLH7` all landed. Use this document for historical review of the shared
  Podman-aligned hardening rollout and for Windows/macOS prerequisite context;
  current behavior now lives in the stable references plus the owning active
  platform or follow-on plans.
- **Related plans:**
  - `docs/reference/macos-machine-flow.md` — current macOS delivery contract;
    that reference owns the developer-machine topology, guest-image contract,
    and user-facing workflow, while this plan owns the shared reliability and
    lifecycle hardening seams underneath it
  - `docs/plans/archive/macos-machine-support-plan.md` — completed macOS
    execution record with the proof and sequencing that informed this plan
  - `docs/plans/windows-machine-support-plan.md` — deferred Windows platform
    plan; `WIN2` should build on the hardened infrastructure from this plan

## Current Assessed State

- The machine manager (`crates/nimbus-bin/src/machine/manager.rs`) implements
  krunkit+gvproxy orchestration for macOS with basic lifecycle management.
- SSH readiness uses a 2-layer check (port listening + SSH exec) that mirrors
  Podman's approach. The code explicitly comments: "Mirror Podman's macOS
  machine layering."
- Stale state detection works: `refresh_machine_state()` checks PID liveness
  and transitions dead machines to `Failed`/`Stale`.
- Host storage roots now follow a Nimbus-owned XDG split: config under
  `XDG_CONFIG_HOME`, lifecycle state and locks under `XDG_STATE_HOME`,
  durable per-machine VM artifacts under `XDG_DATA_HOME`, and shared
  redownloadable machine-image / guest-binary artifacts under
  `XDG_CACHE_HOME`.
- However, several Podman robustness patterns are missing. Each was identified
  by reading Podman source and comparing against the nimbus implementation.

## Current Review Findings

### 1. Provider-specific graceful stop sequencing — under-hardened

**Podman source:** the WSL provider's `StopVM()` in
`pkg/machine/wsl/stubber.go` runs `enterns systemctl exit 0` with a 60-second
timeout before calling `wsl --terminate`. The Apple/libkrun provider does
*not* SSH into the guest; it stays on the provider seam and calls
`mc.LibKrunHypervisor.KRun.Stop(hardStop, true)`, which lowers to vfkit's
state-change API and a bounded wait/hard-stop fallback in
`pkg/machine/apple/vfkit/helper.go`.

**Nimbus current:** `stop_machine()` in `manager.rs` sends HTTP
`POST /vm/state {"state":"Stop"}` to krunkit's REST API. If that fails,
SIGTERM → SIGKILL the krunkit process. This is the correct control seam for
the macOS krunkit path, but the plan text currently over-generalizes WSL's
guest-systemd stop flow onto macOS.

**Risk:** Adding an ad hoc SSH shutdown path on macOS would diverge from
Podman's battle-tested provider-owned libkrun lifecycle. At the same time, the
current Nimbus stop path still lacks an explicit Podman-shaped bounded wait and
hard-stop policy on the provider seam, and the future WSL provider still needs
its own nested-systemd exit sequence.

**Fix:** Keep macOS on provider-driven stop semantics: stop through the
krunkit/vfkit control surface, wait with a Podman-like bounded grace period,
and only hard-stop on timeout. For the future WSL2 provider, implement
Podman's `enterns systemctl exit 0` → `wsl --terminate` sequence. Do **not**
add SSH guest shutdown to the krunkit path.

### 2. Signal handling during machine startup — missing

**Podman source:** `pkg/machine/shim/host.go` (lines 541-574) registers a
goroutine that catches SIGINT/SIGTERM/SIGPIPE during `Start()`. On signal, it
sets `mc.Starting = false`, writes the config, and lets the signal propagate.
This prevents machines from getting stuck in a `Starting` state.

**Nimbus current:** `start_machine()` in `manager.rs` spawns gvproxy and
krunkit as child processes but does not register signal handlers. If the user
presses Ctrl-C during startup, the state file may be left as
`lifecycle: Starting` with stale PID references.

**Risk:** After a Ctrl-C during startup, `nimbus machine status` shows
`starting` forever. The user must `nimbus machine rm` and recreate.
`refresh_machine_state()` does detect stale PIDs if the processes died, but if
gvproxy is still alive (it was spawned before krunkit), the state may appear
partially valid.

**Fix:** Register a signal handler (via `tokio::signal` or `ctrlc` crate) at
the start of `start_machine()`. On signal:
1. Kill any spawned child processes (gvproxy, krunkit)
2. Transition state to `Stopped` (not `Failed` — the user chose to stop)
3. Persist the updated state
4. Re-raise the signal for normal exit behavior

### 3. Global SSH port allocation with file locking — missing

**Podman source:** `pkg/machine/ports/ports.go` implements:
- File-based lock at `{globalDataDir}/port-alloc.lck`
- Allocation state at `{globalDataDir}/port-alloc.dat` (JSON)
- Port range: 10000-65535
- `AllocateMachinePort()` / `ReleaseMachinePort()`
- On startup, `reassignSSHPort()` releases old port, allocates new one,
  updates connection config if the previously allocated port is now in use

**Nimbus current:** `allocate_local_port()` binds to `127.0.0.1:0` and reads
the OS-assigned ephemeral port. Reuses the previous port on restart (stored in
`MachineRuntimeState.ssh_port`). No file locking, no conflict detection, no
reassignment.

**Risk:**
- Two concurrent `nimbus machine init` calls could theoretically allocate the
  same ephemeral port (unlikely but possible)
- A port allocated in a previous session could be taken by another application
  by the time the machine restarts, causing a hard startup failure with no
  automatic recovery
- When Windows support lands, multiple machines can run concurrently
  (`RequireExclusiveActive = false` for WSL2), making port collisions more
  likely

**Fix:** Implement Podman's file-locked port allocation:
- Lock file at the shared machine state root
  (for example `${XDG_STATE_HOME:-~/.local/state}/nimbus/machine/port-alloc.lck`)
- Allocation state at that same shared machine state root
  (`port-alloc.dat`), with the global reservation set living there and the
  current machine's assigned port continuing to live in its machine record
- Port range: 10000-65535 (or configurable)
- On startup, check if allocated port is in use. If so, release and reallocate.
- On machine removal, release the port back to the pool.

### 4. Atomic config/state writes with file locking — missing

**Podman source:** `pkg/machine/vmconfigs/machine.go`:
- `lockfile.LockFile` for exclusive access during config operations
- `ioutils.AtomicWriteFile()` for writes (write to temp file, then rename)
- `mc.Lock()` / `mc.Unlock()` called around all state mutations

**Nimbus current:** `write_json_file()` in `mod.rs` uses direct `fs::write()`
— no locking, no atomic write pattern.

**Risk:** If `nimbus machine status` reads the config while
`nimbus machine start` is writing it, the reader could see partial or corrupt
JSON. This is unlikely in a single-user CLI today but becomes more likely when
`nimbus serve` auto-manages the machine lifecycle (concurrent reads and writes
from the server and CLI). Podman's single-config-file lock is not sufficient
guidance on its own because Nimbus persists two coupled records:
`config.json` and `status.json`.

**Fix:**
- Atomic writes: write to `{path}.tmp`, then `fs::rename()` to `{path}`.
  Rename is atomic on both Unix and Windows (same file system).
- File locking: add a **per-machine** lockfile that covers both
  `config.json`, `status.json`, and related lifecycle bookkeeping, instead of
  independent per-file locks that still allow mixed-generation reads across the
  two records. Use `fs2::FileExt::lock_exclusive()` (or platform-appropriate
  equivalent) around mutating operations; read paths should participate in the
  same coherence contract.

### 5. Machine record versioning and state rebuild policy — missing

**Podman source:** `pkg/machine/vmconfigs/config.go` defines
`MachineConfigVersion = 1`, and `pkg/machine/vmconfigs/machine.go` checks the
stored version during config load so incompatible machine configs fail
explicitly instead of surfacing as opaque parse errors.

**Nimbus current:** `MachineConfigRecord` and `MachineStateRecord` have no
version field.

**Risk:** If the config schema changes in a future nimbus release, existing
machine configs will fail to deserialize with an opaque serde error. The user
must `rm` and `init` a new machine, losing their configuration. Adding a
version field only to `MachineConfigRecord` would still leave
`MachineStateRecord` as an opaque failure mode, even though Nimbus loads both
records on ordinary machine commands.

**Fix:** Add `version: u32` to `MachineConfigRecord` and an explicit version or
schema tag to `MachineStateRecord`. Set `CURRENT_MACHINE_CONFIG_VERSION = 1`
and `CURRENT_MACHINE_STATE_VERSION = 1`. On load:
- If version matches: proceed normally
- If version is older: migrate or return a clear error with upgrade
  instructions
- If version is newer: return a clear error ("this machine was created with a
  newer version of nimbus")
- If state is unsupported or unreadable: reset/rebuild the state record
  explicitly instead of surfacing a raw serde parse failure

This is cheapest to add pre-launch (no existing configs to migrate).

### 6. Provider capability flags — missing

**Podman source:** `pkg/machine/vmconfigs/config.go` defines capability
methods on `VMProvider`:
```go
UseProviderNetworkSetup() bool     // true=provider owns networking, false=shim runs gvproxy
RequireExclusiveActive() bool      // true=only one VM at a time, false=parallel
MountType() VolumeMountType        // VirtIOFS, QEMU9pfs, Sshfs, Unknown
VMType() define.VMType             // WSLVirt, AppleHvVirt, HyperVVirt, etc.
```
The shim orchestrator uses these flags to generically decide behavior without
matching on provider type.

**Nimbus current:** `MachineProvider` enum has only `Krunkit`. All startup
code is hardcoded for krunkit+gvproxy. No capability abstraction.

**Risk:** When the WSL2 provider lands (`WIN2`), the startup code will need
`match provider { Krunkit => ..., Wsl2 => ... }` in every function. Without
capability flags, every new provider requires touching every orchestration
function.

**Fix:** Add capability methods alongside the provider enum:
```rust
impl MachineProvider {
    fn uses_provider_networking(&self) -> bool {
        match self {
            Self::Krunkit => false,  // shim runs gvproxy
            Self::Wsl2 => true,     // WSL2 owns networking
        }
    }
    fn requires_exclusive_active(&self) -> bool {
        match self {
            Self::Krunkit => true,   // one krunkit VM at a time
            Self::Wsl2 => false,     // multiple WSL2 distros allowed
        }
    }
    fn image_format(&self) -> ImageFormat {
        match self {
            Self::Krunkit => ImageFormat::Raw,
            Self::Wsl2 => ImageFormat::Tar,
        }
    }
}
```

Then use `config.provider.uses_provider_networking()` instead of
`match config.provider { Krunkit => ... }` in the orchestrator.

### 7. Explicit pre/post-start networking phases — missing

**Podman source:** `pkg/machine/shim/host.go` and
`pkg/machine/shim/networking.go` separate networking into three explicit
phases:
1. `startNetworking()` — before VM launch: SSH port check, gvproxy start
   (non-WSL), provider-specific pre-networking
2. `StartVM()` → `WaitForReady()` — VM launch and readiness
3. `PostStartNetworking()` — after VM is running: win-sshproxy launch (WSL),
   socket forwarding verification, readiness check

**Nimbus current:** `start_machine()` runs everything sequentially in one
function: spawn gvproxy → spawn krunkit → wait for ready → wait for SSH. No
explicit phase separation.

**Risk:** When the WSL2 provider lands, the startup flow diverges
significantly:
- macOS: gvproxy BEFORE VM, socket forwarding via gvproxy
- Windows: NO gvproxy before VM, win-sshproxy AFTER VM
Without explicit phases, this becomes a tangled `match provider` block.

**Fix:** Factor `start_machine()` into explicit phases:
```rust
fn start_machine(...) {
    pre_start_networking(provider, config, state)?;   // gvproxy (krunkit) or no-op (WSL2)
    start_vm(provider, config, state)?;                // krunkit or wsl --import+bootstrap
    wait_for_ready(provider, config, state)?;          // vsock ready (krunkit) or no-op (WSL2)
    post_start_networking(provider, config, state)?;   // socket verify (krunkit) or win-sshproxy (WSL2)
    conduct_readiness_check(provider, config, state)?; // SSH port + SSH exec (both)
}
```

Each phase dispatches on provider capability flags rather than matching on
provider type directly.

## Podman Alignment Matrix

| Concern | Podman | Nimbus current | Target |
| --- | --- | --- | --- |
| Graceful stop sequencing | WSL exits nested systemd before terminate; Apple/libkrun stays on provider stop + bounded wait/hard-stop | krunkit REST stop, then 20s SIGTERM/SIGKILL helper teardown | Provider-specific stop sequencing: macOS stays provider-driven; future WSL exits nested systemd before terminate |
| Signal handling during startup | SIGINT/SIGTERM handler marks Starting=false | No signal handling | Signal handler kills children, transitions to Stopped |
| SSH port allocation | Global file-locked pool (10000-65535), conflict reassignment | OS-assigned ephemeral port, no locking | File-locked pool, conflict reassignment |
| Config persistence | Atomic writes + lockfile around machine config | Direct `fs::write()` on separate config/state files | Atomic temp+rename under a per-machine lock covering config and state |
| Record versioning | `MachineConfigVersion` + compatibility checks on load | No version field | Versioned config plus explicit state version/rebuild policy |
| Provider capability flags | `UseProviderNetworkSetup`, `RequireExclusiveActive`, etc. | Hardcoded for krunkit | Capability methods on `MachineProvider` |
| Networking phases | `StartNetworking` → `StartVM` → `PostStartNetworking` | Sequential in one function | Explicit pre/post phases |

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Roadmap Status Ledger` and `Execution Log`
3. `docs/reference/microvm-service-baseline.md` (landed Linux/service baseline)
4. `docs/reference/macos-machine-flow.md` (current macOS contract)
5. `docs/plans/windows-machine-support-plan.md` (Windows execution context)
6. the reviewed Podman source files listed at the top of this document

General rules:

- This is the archived historical control record for shared
  machine-lifecycle hardening. Treat it as durable background context for
  handoff, review, and prerequisite analysis when work touches machine
  robustness or shared provider orchestration, but promote a new active plan
  before starting additional shared hardening scope.
- These items harden existing infrastructure. They do not change the macOS or
  Windows architecture — those are owned by their respective platform plans.
- Before changing code for this workstream, reread `Current Assessed State`,
  `Current Review Findings`, `Podman Alignment Matrix`,
  `Control Plan Rules`, `Verification Contract`, `Roadmap Status Ledger`, and
  `Execution Log`.
- Resume the earliest `MLH*` item that is not `done`, or continue any item
  already marked `in_progress`, before starting new machine-lifecycle scope
  unless the user explicitly reprioritizes the ledger.
- Shared hardening items (port allocation, config persistence, record
  versioning, provider flags, networking phases) should land before `WIN2` so
  the Windows provider builds on hardened infrastructure instead of inventing
  its own side path.
- macOS-facing hardening items (provider-driven krunkit stop sequencing and
  startup signal handling) should land before machine-workstream closeout so
  the developer UX carries enterprise-grade reliability, not just basic
  functionality.
- When work materially advances both this plan and a platform-specific plan,
  update both ledgers in the same change set. The platform plan owns topology;
  this plan owns reliability hardening.
- Every code change must pass `cargo fmt --all --check`, focused `cargo check`,
  and focused tests for the touched machine manager seams.
- Every substantive work burst must update this plan's ledger and execution log
  in the same change set.

## Scope

This plan covers:

- provider-specific graceful stop sequencing
- signal handling during machine startup
- global SSH port allocation with file locking
- atomic config/state writes with file locking
- machine record versioning and state rebuild policy
- provider capability flags on `MachineProvider`
- explicit pre/post-start networking phases
- the shared host-storage contract for machine config, state, data, and caches

This plan does not cover:

- macOS machine architecture (owned by macOS plan)
- Windows machine architecture (owned by Windows plan)
- new providers or backends
- machine image artifacts or guest bootstrap content

## Shared Host Storage Contract

This control decision exists to keep Nimbus Podman-aligned at the published
artifact and runtime-seam layer without coupling Nimbus to Podman's or
Docker's mutable host state.

### Build Now

- Keep machine control roots Nimbus-owned and separate from
  `~/.config/containers`, `~/.local/share/containers`, Docker Desktop state,
  or any other tool-owned runtime directories.
- Share redownloadable artifacts across **Nimbus machines** where it reduces
  user overhead, but keep that sharing inside Nimbus-owned cache roots instead
  of reusing Podman or Docker stores.
- Reuse standards-boundary inputs where they are intentionally stable and
  user-beneficial, such as published OCI references/digests, system CA trust,
  and explicitly chosen SSH keys.

### Do Not Build

- Do not share machine definitions, machine status, default-machine pointers,
  forwarded sockets, PID files, or lock files with Podman or Docker.
- Do not share VM disks, EFI variable stores, or any other mutable guest
  machine data across tools.
- Do not point Nimbus at Podman's local image store, Docker's image store,
  libpod metadata, containerd metadata, or any other mutable runtime store as a
  shortcut.
- Do not make Podman or Docker host state part of Nimbus's correctness
  contract for machine start, stop, rebuild, or recovery.

### What Is Reasonable To Share

- **Within Nimbus:** machine image blob caches, downloaded guest Linux
  `nimbus` assets, and similar redownloadable artifacts should be shared across
  Nimbus machines so users do not pay repeated download/decompression costs.
- **Across tools by stable convention:** system CA trust, registry credentials
  or credential-helper conventions if Nimbus explicitly adopts them, and
  user-selected SSH identities.
- **At the logical contract layer only:** immutable OCI references/digests and
  other published artifact identifiers. The identifier may be shared across
  tools; the local cache/store implementation should remain Nimbus-owned.

### Target XDG Split

- config: `${XDG_CONFIG_HOME:-~/.config}/nimbus/machine`
- state: `${XDG_STATE_HOME:-~/.local/state}/nimbus/machine`
- data: `${XDG_DATA_HOME:-~/.local/share}/nimbus/machine`
- cache: `${XDG_CACHE_HOME:-~/.cache}/nimbus/machine`

### Planned Follow-On Refactor

- Keep `config.json`, generated ignition, machine records, port-allocation
  state, and machine lock files in the config/state roots where they already
  fit the control-plane contract.
- Move durable per-machine VM artifacts such as the materialized raw disk and
  EFI variable store from the state root into the data root.
- Move redownloadable machine image layers, decompression staging, and the
  guest Linux `nimbus` asset cache out of the per-machine state tree and into
  a Nimbus-owned shared cache root.
- If future sharing is needed to reduce user overhead further, prefer adding a
  standalone Nimbus-owned content-addressed cache contract rather than
  coupling Nimbus directly to Podman's or Docker's internal stores.

## Verification Contract

### Minimum verification for every code item

- `cargo fmt --all --check`
- focused `cargo check -p nimbus-bin`
- focused `cargo test -p nimbus-bin` for the machine module
- plan ledger and execution-log update in the same change set

### Required verification lanes

- **Graceful stop lane**
  - macOS krunkit stop uses the provider control surface first and waits
    through the bounded grace period before hard-stop escalation
  - future WSL stop exits nested systemd before `wsl --terminate`
  - timeout behavior is explicit and testable

- **Signal handling lane**
  - Ctrl-C during `nimbus machine start` transitions to `Stopped`
  - child processes (gvproxy, krunkit) are cleaned up on signal
  - state file is not left as `Starting` after signal

- **Port allocation lane**
  - allocated ports persist across restarts
  - port conflicts are detected and reassigned automatically
  - concurrent machine inits do not collide (file lock prevents race)
  - machine removal releases the port

- **Config persistence lane**
  - concurrent read during write does not produce corrupt JSON
  - crash during write does not leave corrupt state (atomic rename)
  - concurrent CLI/server operations do not observe mixed-generation
    `config.json` and `status.json`

- **Record versioning lane**
  - older config version produces clear upgrade message
  - newer config version produces clear downgrade warning
  - incompatible or corrupt state triggers an explicit reset/rebuild path
    instead of a raw parse failure
  - current versions load normally

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| MLH1 | done | `stop_machine()` now routes through provider-specific stop sequencing; the krunkit path issues `/vm/state` `Stop`, waits on the provider process, escalates with `/vm/state` `HardStop` only when needed, and uses a Podman-aligned configurable grace budget before gvproxy cleanup | none |
| MLH2 | done | Startup now installs scoped SIGINT/SIGTERM monitoring; interrupted startup kills spawned children, clears runtime artifacts, persists `Stopped`, and avoids leaving machine state stuck in `Starting` | none |
| MLH3 | done | SSH ports now allocate from a shared file-locked pool under the machine state root, reuse the recorded machine port when it is still valid, reassign on startup when the old port is busy or outside the managed range, and release by machine name on removal | none |
| MLH4 | done | Machine config/state writes now use atomic temp-file replacement, and machine command entrypoints plus default machine-API client loading hold a per-machine lock under the shared state root so config/status reads and writes stay coherent | none |
| MLH5 | done | Machine config/state records now carry explicit schema versions; unsupported config versions now fail clearly with recreate guidance, and unreadable or incompatible state rebuilds with an explicit stale error instead of a raw parse failure | none |
| MLH6 | done | `MachineProvider` now carries explicit capability values for networking ownership, exclusivity, image format, and bootstrap mode, with Podman-aligned `Krunkit` values plus a staged `Wsl2` capability contract for future Windows work | none |
| MLH7 | done | `start_machine()` now runs through explicit bootstrap, pre-start networking, VM start, machine-ready, post-start networking, and readiness-check phases dispatched from provider capabilities while preserving the current macOS flow | MLH6 |

## Implementation Checkpoints

### MLH1 — Provider-specific graceful stop sequencing

Repo outputs:

- provider-specific graceful stop helper in machine manager
- macOS krunkit path keeps the provider control seam (`/vm/state`) and gains a
  bounded wait/hard-stop policy that matches Podman's libkrun/vfkit shape
- future WSL path runs `enterns systemctl exit 0` before `wsl --terminate`
- configurable stop timeout with a Podman-aligned default grace budget
- updated `stop_machine()` to delegate to provider-specific stop sequencing

Acceptance criteria:

- `nimbus machine stop` on macOS uses the provider control surface first, not
  ad hoc guest SSH
- the helper wait budget prevents premature helper kill during ordinary
  graceful stop on macOS
- the future WSL provider exits nested systemd before distribution terminate
- timeout/fallback behavior is explicit and testable

### MLH2 — Signal handling during startup

Repo outputs:

- signal handler registration at the start of `start_machine()`
- on SIGINT/SIGTERM:
  1. kill spawned child processes (gvproxy PID, krunkit PID)
  2. transition lifecycle to `Stopped`
  3. persist updated state
- handler deregistered after startup completes

Acceptance criteria:

- Ctrl-C during `nimbus machine start` does not leave state as `Starting`
- gvproxy and krunkit processes are cleaned up on signal
- `nimbus machine status` after Ctrl-C shows `stopped`, not `starting`

### MLH3 — Global SSH port allocation

Repo outputs:

- port allocation module with file-based locking
- lock file at the shared machine state root (`MachineRootLayout.state_root`)
- allocation state at that same shared machine state root
- `allocate_machine_port()` / `release_machine_port()` functions
- port range: 10000-65535
- conflict detection on startup: if allocated port is in use, release and
  reallocate
- machine removal releases the port

Acceptance criteria:

- allocated ports are stable across restarts (same machine gets same port)
- port conflicts from external applications are detected and recovered
- concurrent `nimbus machine init` calls do not allocate the same port
- `nimbus machine rm` releases the port back to the pool

### MLH4 — Atomic config/state writes

Repo outputs:

- `write_json_file_atomic()` function (or update existing `write_json_file()`)
  using write-to-temp-then-rename pattern
- per-machine lockfile that spans both `config.json` and `status.json`
- file locking via `fs2::FileExt::lock_exclusive()` or platform-appropriate
  equivalent around lifecycle mutations
- read paths participate in the same machine-level coherence contract

Acceptance criteria:

- crash during write does not leave corrupt config/state files
- concurrent read during write sees either old or new state, never partial
- file locking prevents concurrent writes from interleaving across config and
  state
- concurrent server/CLI operations do not observe mixed-generation machine
  records

### MLH5 — Machine record versioning and state rebuild policy

Repo outputs:

- `version: u32` field added to `MachineConfigRecord`
- explicit version/schema tag added to `MachineStateRecord`
- `CURRENT_MACHINE_CONFIG_VERSION: u32 = 1`
- `CURRENT_MACHINE_STATE_VERSION: u32 = 1`
- load policy:
  - current version: proceed normally
- older version: clear recreate guidance instead of silent migration
- newer version: clear error ("created with newer nimbus version")
- unsupported or unreadable state: reset/rebuild state explicitly instead of
  surfacing a raw parse failure

Acceptance criteria:

- unsupported config versions are rejected with a clear recreate message
- version mismatch produces a helpful compatibility error, not a serde parse
  failure
- corrupt or incompatible state does not brick ordinary machine commands
- new machines are created with current config/state versions

### MLH6 — Provider capability flags

Repo outputs:

- capability methods on `MachineProvider`:
  ```rust
  fn uses_provider_networking(&self) -> bool
  fn requires_exclusive_active(&self) -> bool
  fn image_format(&self) -> ImageFormat
  fn bootstrap_mode(&self) -> BootstrapMode  // Ignition or ShellScript
  ```
- existing krunkit orchestration code updated to use capability checks where
  appropriate (no behavior change, just indirection)

Acceptance criteria:

- existing macOS krunkit path works identically after refactor
- capability methods return correct values for `Krunkit`
- `Wsl2` variant can be added later with correct capability values without
  touching the orchestrator

### MLH7 — Explicit pre/post-start networking phases

Repo outputs:

- `start_machine()` refactored into explicit phases:
  1. `pre_start_networking()` — SSH port check, gvproxy start (if
     `!uses_provider_networking()`)
  2. `start_vm()` — krunkit launch (or WSL distro start)
  3. `wait_for_ready()` — vsock ready signal (or no-op for WSL)
  4. `post_start_networking()` — socket forwarding verification (or
     win-sshproxy launch for WSL)
  5. `conduct_readiness_check()` — SSH port + SSH exec
- each phase dispatches on provider capability flags
- existing macOS flow is functionally identical after refactor

Acceptance criteria:

- existing macOS startup works identically after refactor
- each phase is a separate testable function
- adding a new provider requires implementing each phase, not modifying the
  orchestrator
- the orchestration sequence matches Podman's `Start()` ordering

## Dependency Graph

- `MLH1` through `MLH5` have no dependencies on each other and can proceed in
  any order.
- `MLH6` has no dependencies.
- `MLH7` depends on `MLH6` (uses capability flags to dispatch phases).
- The macOS plan's `MAC7` should ideally follow `MLH1` and `MLH2`.
- The Windows plan's `WIN2` should ideally follow `MLH3`, `MLH4`, `MLH5`,
  `MLH6`, and `MLH7`.

## Recommended Delivery Order

1. `MLH5` — config versioning (cheapest to add now, most expensive to add
   later; no behavior change, just a new field)
2. `MLH4` — atomic writes (small, low-risk improvement)
3. `MLH3` — port allocation (shared macOS+Windows concern)
4. `MLH2` — signal handling (fixes real user-facing bug)
5. `MLH1` — provider-specific graceful stop sequencing (most impactful for
   service reliability)
6. `MLH6` — provider capability flags (architectural prep for Windows)
7. `MLH7` — networking phases (architectural prep for Windows)

Items 1-5 improve the macOS path today. Items 6-7 are primarily architectural
prep for Windows but also clean up the macOS code.

## Execution Log

- 2026-04-18: Administratively closed and archived this plan after confirming
  the shared `MLH1`-`MLH7` ledger is fully complete and the follow-on machine
  work has returned to the owning platform plans and stable reference docs.
  Updated the repo entrypoints and plan index so new agents treat this
  document as historical execution context rather than an active control
  plane.
- 2026-04-15: Created this plan based on source-backed review of Podman's
  machine infrastructure against the existing nimbus machine module. Identified
  7 robustness gaps by comparing `manager.rs` against Podman's `shim/host.go`,
  `ports/ports.go`, `vmconfigs/machine.go`, `wsl/stubber.go`, and
  `vmconfigs/config.go`. Each gap has a concrete Podman source reference and a
  specific fix. Plan is deferred until macOS MAC6 is complete.
- 2026-04-15: Corrected the plan after a deeper source-backed review against
  the current Nimbus machine module and Podman's Apple/WSL providers. Reframed
  `MLH1` around provider-specific stop sequencing instead of incorrectly
  copying WSL's guest-systemd shutdown flow onto macOS libkrun; moved SSH port
  allocation artifacts to the shared machine-state root; upgraded `MLH4` to
  require a per-machine lock spanning both `config.json` and `status.json`;
  and upgraded `MLH5` to cover Nimbus's split config/state persistence model
  with an explicit state rebuild policy. This keeps Nimbus aligned with
  Podman's battle-tested machine-management seams without introducing
  unnecessary divergence on the krunkit path.
- 2026-04-16: Promoted this document from a deferred follow-on plan to the
  active shared machine-lifecycle hardening control plane. Clarified that new
  agents should treat this ledger plus the git worktree as the durable progress
  state for enterprise-readiness reliability work, while the macOS and Windows
  plans remain the architecture owners for their respective platform flows.
- 2026-04-16: Completed `MLH5` in `crates/nimbus-bin/src/machine/`. Added
  `CURRENT_MACHINE_CONFIG_VERSION` / `CURRENT_MACHINE_STATE_VERSION` plus
  explicit `version` fields on machine config/state records; unsupported or
  newer config versions now fail with an operator-friendly compatibility error;
  and unreadable or
  unsupported state now rebuilds into a `Stopped` + `Stale` record with the
  rebuild reason persisted in `last_error`. Focused regression coverage now
  exercises older-version rejection, newer-version rejection, and corrupt-state
  rebuild behavior. Verification: `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`, `cargo test -p nimbus-bin machine::`. Recommended
  next item: `MLH4` so the new record contract lands on top of atomic,
  machine-level coherent writes instead of direct `fs::write()`.
- 2026-04-16: Completed `MLH4` in `crates/nimbus-bin/src/machine/`. Replaced
  direct record writes with atomic `NamedTempFile` write + flush + sync +
  persist semantics, added a per-machine advisory lock file under the shared
  machine state root (`<state-root>/<name>.lock`), and wrapped the default
  machine command entrypoints plus `require_default_machine_api_client()` in
  that lock so config/state reads and writes participate in one machine-level
  coherence contract. Focused regression coverage now checks the state-root
  lock path and atomic replacement behavior in addition to the existing
  machine tests. Verification: `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`, `cargo test -p nimbus-bin machine::`. Recommended
  next item: `MLH3` so the same shared state root now used for locking becomes
  the durable home for file-locked SSH port allocation.
- 2026-04-16: Completed `MLH3` in `crates/nimbus-bin/src/machine/`. Added a
  shared SSH port allocator under the machine state root using
  `port-alloc.dat` plus `port-alloc.lck`, reserved by machine name under a
  global advisory lock. `MachineLaunchPlan::build()` now reuses the recorded
  machine port when it is still within the managed range and actually
  available, but automatically reassigns when the recorded port is busy or
  outside the managed pool; `nimbus machine rm` now releases the reservation.
  Focused regression coverage now exercises recorded-port reuse, busy-port
  reassignment, direct release, and removal-triggered release in addition to
  the existing machine tests. Verification: `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`, `cargo test -p nimbus-bin machine::`. Recommended
  next item: `MLH2` so interrupted startup no longer leaves partially launched
  helper state even though record writes and port allocation are now coherent.
- 2026-04-16: Completed `MLH2` in `crates/nimbus-bin/src/machine/`. Startup now
  installs a scoped SIGINT/SIGTERM monitor using `signal-hook-registry`,
  checks that monitor inside the gvproxy socket wait loop, ready-signal wait
  loop, and SSH-readiness loop, and treats interruption as an explicit
  cancelled-start path instead of a generic failure. On cancellation, Nimbus
  now kills spawned children, removes runtime artifacts, restores state to
  `Stopped`, and persists the repaired record instead of leaving the machine in
  `Starting`. Focused regression coverage now exercises cancelled wait-path
  behavior plus interrupted-start cleanup and state repair. Verification:
  `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine::`. Recommended next item: `MLH1` so the
  same enterprise-grade lifecycle handling applies on shutdown, not just
  startup interruption.
- 2026-04-16: Completed `MLH1` in `crates/nimbus-bin/src/machine/`. The
  macOS krunkit path now follows Podman's libkrun/vfkit stop shape: issue the
  provider control-plane `Stop` request first, wait on the provider process for
  a configurable grace budget (`NIMBUS_MACHINE_STOP_TIMEOUT_SECS`, default
  90s), escalate with provider-level `HardStop` only when the graceful wait
  expires, and only then clean up gvproxy. Focused regression coverage now
  exercises graceful control-seam shutdown, hard-stop request payloads, and the
  timeout/force-stop helpers without relying on flaky integration races.
  Verification: `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine::`. Recommended next item: `MLH6` so the
  provider-specific decisions move into explicit capabilities before the
  Windows provider lands.
- 2026-04-16: Completed `MLH6` and `MLH7` in
  `crates/nimbus-bin/src/machine/`. `MachineProvider` now owns explicit
  Podman-aligned capability values for networking ownership, exclusivity, image
  format, and bootstrap mode; `Krunkit` uses host-launched networking plus raw
  disk + ignition, and a staged `Wsl2` capability contract now exists without
  changing the current macOS behavior. `start_machine()` is refactored into
  explicit bootstrap, pre-start networking, VM start, machine-ready,
  post-start networking, and readiness-check phases that dispatch from those
  capability values. The current macOS flow is preserved, but the orchestrator
  no longer needs platform-specific branching smeared through one block of
  startup code. Focused regression coverage now exercises the provider
  capability contract alongside the existing launch, stop, and readiness
  seams. Verification: `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine::`. Recommended next item: none; the shared
  `MLH1`-`MLH7` roadmap is complete, so follow-on work should resume in the
  owning platform plans (`MAC4`-`MAC7`, `WIN1`-`WIN2`) rather than opening new
  lifecycle-hardening items here.
- 2026-04-17: Follow-on shared hardening in `crates/nimbus-bin/src/machine/`
  tightened the guest machine-API capability contract after the Podman-aligned
  macOS guest proved image-start-ready without `buildah`, but the flat
  `required_binaries` payload still made that look like a runtime defect.
  Replaced that payload with explicit `binary_statuses` plus
  `operation_statuses`, bumped the guest machine-API protocol to `v1alpha2`,
  and taught the host client to fail version skew with an operator-friendly
  "host expects X / guest reported Y" error that points operators at an
  explicit `NIMBUS_MACHINE_GUEST_BINARY` override when they intentionally need
  to stage a non-release guest binary. Verification:
  `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine::api:: -- --nocapture`,
  `cargo test -p nimbus-bin machine::client:: -- --nocapture`,
  `cargo test -p nimbus-bin service:: -- --nocapture`. Live macOS proof on the
  isolated temp root `/tmp/nimbus-bootstrap-smoke.s7lYjJ` confirmed the new
  mismatch error during `target/debug/nimbus machine start`:
  `guest machine API protocol mismatch ... host expects v1alpha2, guest reported v1alpha1`.
  The intended shipping contract remains release-asset first rather than
  implicit workspace-binary discovery. Full live proof of the new `v1alpha2`
  capability payload remained blocked at that point on
  rebuilding the aarch64 Linux guest binary locally; this host currently lacks
  `aarch64-linux-gnu-gcc`, so `cargo build --release --target aarch64-unknown-linux-gnu -p nimbus-bin`
  fails before producing a matching guest override binary.
- 2026-04-17: Closed that proof gap on the same isolated macOS temp root
  `/tmp/nimbus-bootstrap-smoke.s7lYjJ` without touching the Homebrew-installed
  Nimbus state. Installed `cargo-zigbuild` into `/tmp/cargo-zigbuild`,
  redirected Zig and cargo-zigbuild caches into `/tmp`, and built a matching
  Linux guest artifact with
  `cargo zigbuild --release --target aarch64-unknown-linux-gnu -p nimbus-bin`
  while overriding the archiver to `/usr/bin/ar` and `/usr/bin/ranlib` plus
  `LIBZ_SYS_STATIC=1` for the bundled zlib cross build. Rebuilt the host debug
  CLI with `cargo build -p nimbus-bin`, then restarted the isolated macOS
  machine with `NIMBUS_MACHINE_GUEST_BINARY=/Users/jack/src/github.com/nimbus/nimbus/target/aarch64-unknown-linux-gnu/release/nimbus`
  to remove any ambiguity about the staged guest binary. The successful run
  returned `lifecycle: running`, `manager: ready`, and a forwarded machine API
  capability payload with `protocol_version: v1alpha2`,
  `service_execution_ready: true`, `service-sandboxes.image-start:
  available=true`, and `service-sandboxes.build-start: available=false` with
  the expected `buildah` blocker. Direct forwarded-socket proof on
  `/tmp/nimbus-bootstrap-smoke.s7lYjJ/runtime/default-api.sock` returned
  `200` on `/healthz` with
  `{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha2",...}`.
  Guest-side proof via `target/debug/nimbus machine ssh` reported
  `nimbus 0.1.10` and current boot ID
  `f46c2819-bb64-49fa-aa4e-d506f4f96590`; no extra bootstrap reboot was needed
  for the successful convergence run.
- 2026-04-18: Added an explicit shared host-storage contract to this plan so
  future machine-lifecycle work does not accidentally couple Nimbus to Podman
  or Docker host state. Locked the contract to Nimbus-owned XDG roots:
  config under `XDG_CONFIG_HOME`, lifecycle state under `XDG_STATE_HOME`,
  durable VM artifacts under `XDG_DATA_HOME`, and redownloadable machine-image
  / guest-binary artifacts under `XDG_CACHE_HOME`. Explicitly recorded that
  sharing should happen inside Nimbus-owned caches across Nimbus machines, not
  by reusing Podman or Docker machine metadata, VM disks, or local image
  stores.
- 2026-04-18: Implemented that storage split in
  `crates/nimbus-bin/src/machine/`. Added `resolve_data_root()` and
  `resolve_cache_root()`, expanded `MachineRootLayout` to persist those roots,
  and bumped machine config schema to v2 so the new split is an explicit
  contract rather than a silent migration path. Durable per-machine boot artifacts now live under
  `data/<machine>/...`, while machine-image blobs and guest Linux `nimbus`
  assets now share a Nimbus-owned cache root across machines. `machine rm`
  now removes the machine data tree without purging shared caches, status
  output surfaces the new roots, and machine commands now expect the new
  config/data/cache layout directly instead of carrying state-root or
  versionless-config compatibility shims. Verification:
  `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine:: -- --nocapture`. Recommended next item:
  none in this control plan; the split is landed, so follow-on work should use
  these roots instead of reintroducing machine artifacts under `state`.
- 2026-04-18: Revalidated the landed storage split on a real macOS host using
  isolated roots under `/tmp/nimbus-mac-validation.KRe8ea`. Proof commands
  used the current worktree's `target/debug/nimbus` with
  `XDG_CONFIG_HOME=/tmp/nimbus-mac-validation.KRe8ea/xdg-config`,
  `XDG_STATE_HOME=/tmp/nimbus-mac-validation.KRe8ea/xdg-state`,
  `XDG_DATA_HOME=/tmp/nimbus-mac-validation.KRe8ea/xdg-data`,
  `XDG_CACHE_HOME=/tmp/nimbus-mac-validation.KRe8ea/xdg-cache`, and
  `NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-validation.KRe8ea/runtime`.
  `machine init` recorded the pinned Podman digest and the split roots;
  `machine start` pulled the compressed Podman image into
  `xdg-cache/nimbus/machine/images/1ca36ee640f03bf7ca59d45cadfdfa2dd2497b79e5c2030871d5798f336b96b4.raw.zst`,
  materialized the boot disk at
  `xdg-data/nimbus/machine/default/images/default.raw`, and reached
  `machine_api.reachable: true` with recorded helper paths
  `/opt/homebrew/bin/krunkit` and
  `/opt/homebrew/opt/podman/libexec/podman/gvproxy`. Guest SSH proof first
  exposed a stale local Linux artifact (`/usr/local/bin/nimbus --version`
  returned `0.1.10` when forced to use the previously built local override),
  so the validation switched to the released `v0.1.11`
  `nimbus_linux_arm64.tar.gz` asset under the same isolated root, restarted
  the machine without re-pulling the Podman image, and then confirmed
  `/usr/local/bin/nimbus --version` returned `nimbus 0.1.11`. The only host
  blocker encountered was local guest cross-build: `cargo build --release
  --target aarch64-unknown-linux-gnu -p nimbus-bin` failed because
  `aarch64-linux-gnu-gcc` is not installed on this macOS host. A follow-up
  validation pass also caught a stop-path residue bug for
  `default-gvproxy.sock-krun.sock`; after wiring that derived path into the
  active runtime cleanup helper, a freshly rebuilt `target/debug/nimbus`
  plus one more cached start/stop cycle left
  `/tmp/nimbus-mac-validation.KRe8ea/runtime` with only truncated log files.
- 2026-04-17: Follow-on hardening in the same shared machine seam closed two
  fresh-root macOS reliability gaps. First, the new
  `scripts/verify-build-nimbus-machine-guest-binary-helper.sh` test harness
  was corrected so it no longer writes fake `cargo` outputs into the real
  workspace `target/`; the helper now supports a test-only
  `NIMBUS_MACHINE_GUEST_BUILD_REPO_ROOT` override, and the verify lane writes
  only into temp fake repos. Second, `sync_guest_nimbus_binary()` now always
  repairs guest socket activation after binary staging by running
  `systemctl daemon-reload`, `stop`, `reset-failed`, removing the stale guest
  Unix socket path, and `start`ing `nimbus.socket` before host API readiness
  checks continue. That closes the first-boot race on stock Podman FCOS images
  where `nimbus.socket`/`nimbus.service` could hit `start-limit-hit` before the
  host had finished staging the guest binary. Verification:
  `bash scripts/verify-build-nimbus-machine-guest-binary-helper.sh`,
  `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin client_reports_guest_protocol_mismatch_cleanly -- --nocapture`,
  `cargo test -p nimbus-bin guest_binary_lookup -- --nocapture`,
  `cargo test -p nimbus-bin podman_machine_os_ -- --nocapture`,
  `cargo test -p nimbus-bin ensure_guest_nimbus_socket_shell_repairs_first_boot_failures -- --nocapture`,
  `cargo build -p nimbus-bin`, and
  `bash scripts/build-nimbus-machine-guest-binary.sh --cache-root /tmp/nimbus-machine-guest-build-real`.
  Fresh-root live proof on
  `/tmp/nimbus-bootstrap-auto-proof.FkWnBv` then succeeded without
  `NIMBUS_MACHINE_GUEST_BINARY`: `machine init --ssh-identity ...` followed by
  `machine start` returned `lifecycle: running`, `manager: ready`,
  recorded the pinned Podman digest as the current machine-image identity, and
  exposed a reachable forwarded machine API at
  `/tmp/nimbus-bootstrap-auto-proof.FkWnBv/runtime/default-api.sock` with
  `/healthz` returning
  `{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha2",...}`.
  Guest SSH proof on the same root reported `nimbus 0.1.10`,
  boot ID `ee6e4fd4-3a1e-4341-8657-7257ca2ef181`, and
  `nimbus.socket` plus `nimbus.service` both `ActiveState=active`; no extra
  bootstrap reboot was needed. The remaining macOS functional gap is still the
  intentionally unavailable `service-sandboxes.build-start` lane until the
  Podman-aligned build path lands without introducing a direct Podman product
  dependency. The broader UX gap that `machine start` still requires a prior
  `machine init` remains in the owning macOS plan rather than this shared
  lifecycle plan.
- 2026-04-17: Began the follow-on runtime cleanup for that remaining macOS
  `service-sandboxes.build-start` gap by removing `buildah`-specific launch
  artifact terminology from the shared sandbox backends without changing
  behavior. `crates/nimbus-sandbox/src/backends/oci/buildah.rs` now returns a
  generic mounted-rootfs session (`MountedRootfsSession` /
  `PreparedMountedImageLaunch`) instead of leaking `BuildahContainer` /
  `PreparedImageLaunch` into the rest of the runtime. Both
  `backends/container/runtime.rs` and `backends/krun/vm.rs` now persist
  `MountedRootfs` launch artifacts and talk about mount-session reuse instead
  of `buildah` containers, while `backends/oci/conmon.rs` now threads that
  same mount-session seam through the create/state/start/delete launch plan.
  This is intentionally a behavior-preserving refactor: the build-backed lane
  still shells through `BuildahCli` today, but the builder-specific shape is
  now localized to the implementation boundary so the next slice can replace
  the actual Dockerfile/build execution path without another cross-cutting
  rename across manifest, krun, container, and conmon plumbing. Verification:
  `cargo fmt --all`, `cargo fmt --all --check`,
  `cargo check -p nimbus-sandbox -p nimbus-bin`,
  `cargo test -p nimbus-sandbox pull_mount_inspect_and_cleanup_execute_expected_commands -- --nocapture`,
  `cargo test -p nimbus-sandbox prepare_built_image_launch_uses_built_image_reference -- --nocapture`,
  `cargo test -p nimbus-sandbox plan_only_backend_lowers_build_launch_through_generic_trait_surface -- --nocapture`,
  and
  `cargo test -p nimbus-sandbox conmon_launch_plan_injects_mount_prelude_for_image_backed_sandboxes -- --nocapture`.
  The remaining work is the real Podman-aligned builder replacement itself:
  `image-start` already uses the in-tree OCI materializer, but `build-start`
  still needs a first-class Nimbus build pipeline that turns a Dockerfile and
  context into that same mounted-rootfs contract without requiring a standalone
  guest `buildah` product dependency.
- 2026-04-17: Closed that `service-sandboxes.build-start` dependency gap in
  the shared sandbox/runtime seam. `crates/nimbus-sandbox/src/backends/oci/`
  now includes an internal Dockerfile builder that converges onto the same
  `PreparedMaterializedImageLaunch` contract already used by `image-start`.
  The current supported subset is intentionally narrow and operator-friendly:
  single-stage Dockerfiles with `FROM scratch` or a registry base image plus
  runtime metadata and local-context file operations (`COPY`/`ADD`, `CMD`,
  `ENTRYPOINT`, `ENV`, `WORKDIR`, `USER`, `EXPOSE`, `LABEL`, `STOPSIGNAL`,
  `HEALTHCHECK`, `VOLUME`). Unsupported instructions such as `RUN`,
  multi-stage `FROM`, and flag-heavy `COPY --from=...` now fail explicitly
  instead of silently depending on a missing guest toolchain. Both
  `backends/container/runtime.rs` and `backends/krun/vm.rs` now route
  `start_from_build()` through that materialized-rootfs path, so build-backed
  launches no longer shell through guest `buildah` on the active macOS lane.
  The guest machine-API capability contract in
  `crates/nimbus-bin/src/machine/api.rs` was tightened to match: `build-start`
  now advertises the same runtime prerequisites as `image-start`
  (`conmon`, `crun`, `netavark`, `aardvark-dns`) and no longer reports
  `buildah` or `fuse-overlayfs` as blockers. Verification:
  `cargo fmt --all --check`,
  `cargo check -p nimbus-sandbox -p nimbus-bin`,
  `cargo test -p nimbus-sandbox builder_ -- --nocapture`,
  `cargo test -p nimbus-sandbox plan_only_backend_lowers_build_launch_through_generic_trait_surface -- --nocapture`,
  `cargo test -p nimbus-bin capability_response_ -- --nocapture`,
  and
  `cargo test -p nimbus-bin macos_service_commands_use_forwarded_machine_api_for_container_projects -- --nocapture`.
  Remaining work now moves back up to the owning macOS functional proof path:
  exercise a real build-backed service flow on macOS against the pinned Podman
  image, then decide whether broader Dockerfile coverage (`RUN`,
  multi-stage builds, or remote/flagged context handling) belongs in the
  Nimbus builder or should stay out of the v1 macOS contract.
- 2026-04-17: Follow-on shared reliability proof on the isolated macOS root
  `/tmp/nimbus-mac-buildproof.nDZ0P4` closed the stale-state seams that were
  still making the forwarded service proof flaky even after the Podman-aligned
  guest contract and local guest-binary sync were in place. In
  `crates/nimbus-bin/src/machine/api.rs`, machine-API `list` and
  `inspect-current` now refresh only sandboxes that may still be live
  (`starting`, `ready`, `not_ready`, `stopping`) before reading persisted
  state; that avoids re-inspecting historical stopped sandboxes that reuse the
  same published host port and would otherwise let old cleanup paths withdraw
  the active gvproxy forward. In
  `crates/nimbus-sandbox/src/backends/container/runtime.rs` and
  `crates/nimbus-sandbox/src/backends/krun/vm.rs`, stale pidfiles without a
  live process now collapse to `failed` unless shutdown was explicitly
  requested, which stops post-restart `service up` from reporting
  `already_running` for dead sandboxes. The container cleanup path now also
  treats gvproxy `unexpose` failures as best-effort teardown instead of a hard
  stop failure, matching Podman's operator-facing behavior. Finally,
  `scripts/collect-nimbus-machine-service-proof.sh` now waits for
  `service inspect` to reach `status: ready`, retries the localhost
  published-port probe, and normalizes HTTP CRLF headers before matching
  `200 OK`; `scripts/verify-nimbus-machine-service-proof-helper.sh` was
  updated to exercise those retry paths deterministically. Verification:
  `bash scripts/verify-nimbus-machine-service-proof-helper.sh`,
  `cargo test -p nimbus-bin machine_api_list_and_current_refresh_persisted_service_state_before_reply -- --nocapture`,
  `cargo test -p nimbus-sandbox detect_runtime_status_marks_stale_pidfiles_as_failed -- --nocapture`,
  `cargo test -p nimbus-sandbox release_execution_artifacts_ignores_machine_forwarder_unexpose_failures -- --nocapture`,
  `cargo fmt --all --check`, and `cargo check -p nimbus-bin`. Real host proof
  then succeeded after rebuilding both binaries with
  `cargo build -p nimbus-bin` and
  `bash scripts/build-nimbus-machine-guest-binary.sh --cache-root /tmp/nimbus-machine-guest-build-real`,
  restarting the isolated machine with
  `HOME=/tmp/nimbus-mac-buildproof.nDZ0P4/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-buildproof.nDZ0P4/runtime NIMBUS_MACHINE_GUEST_BINARY=/Users/jack/src/github.com/nimbus/nimbus/target/aarch64-unknown-linux-gnu/release/nimbus NIMBUS_MACHINE_API_READY_TIMEOUT_SECS=180 /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine start`,
  and capturing
  `bash scripts/collect-nimbus-machine-service-proof.sh --home /tmp/nimbus-mac-buildproof.nDZ0P4/home --runtime-root /tmp/nimbus-mac-buildproof.nDZ0P4/runtime --output-dir /tmp/nimbus-mac-buildproof.nDZ0P4/service-proof-buildstart-pathfix-teardownfix-refreshfix-stalepidfix-crlffix --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --compose-file /tmp/nimbus-mac-buildproof.nDZ0P4/project/compose.yaml --service demo --published-url http://127.0.0.1:18080/healthz`.
  That bundle recorded a clean run across `machine status`, machine-API
  `/healthz` and `/capabilities`, `service up`, `service inspect`,
  `service list`, `service ps`, `service logs`, localhost `HTTP/1.1 200 OK`
  on `127.0.0.1:18080/healthz`, `service down`, and `service list` after
  teardown, all without an extra bootstrap reboot. Shared `MLH1`-`MLH7`
  remain complete; the next work returns to the owning macOS plan for the
  remaining build-backed service-flow and packaging closeout.
- 2026-04-17: Tightened that same macOS proof contract so the repo records the
  correct build-backed reality instead of leaving it implicit. The isolated
  project under `/tmp/nimbus-mac-buildproof.nDZ0P4/project/compose.yaml` is
  `build:`-backed, and the successful bundle already proved it: captured
  `service-config.txt` shows `source.kind: build` with the resolved
  Dockerfile/context paths, while `machine-api-capabilities.txt` shows
  `service-sandboxes.build-start` available on the forwarded guest API. To keep
  future regressions obvious, `scripts/verify-nimbus-machine-service-proof-helper.sh`
  now mirrors the live contract by rendering a build-backed compose service and
  a `v1alpha2` capability payload with `build-start` available; the collector
  usage text in `scripts/collect-nimbus-machine-service-proof.sh` now also says
  explicitly that the proof lane covers both image-backed and build-backed
  projects. Verification: `bash scripts/verify-nimbus-machine-service-proof-helper.sh`.
  Durable conclusion: the build-backed macOS service flow is already covered by
  the checked-in proof lane and the recorded real-host bundle. The remaining
  follow-on work is no longer "prove build-start works on macOS"; it is the
  packaging/distribution closeout and any future decision about expanding the
  supported Dockerfile subset beyond the current v1 contract.
- 2026-04-17: Reused the completed shared hardening seams in a new host
  entrypoint instead of letting macOS `serve` invent its own side path.
  `crates/nimbus-bin/src/machine/mod.rs` now exports
  `ensure_default_machine_api_client_started()`, which holds the existing
  per-machine lock, loads the initialized default machine, and routes any
  stopped-machine recovery back through the same `start_machine()` convergence
  path before checking forwarded machine-API health. The macOS host-backed
  `serve` loader in `crates/nimbus-bin/src/service/mod.rs` now uses that
  helper only for container-backed Compose projects, so the host-resident
  server no longer fails with a manual "run `nimbus machine start` first"
  prerequisite. Focused verification:
  `cargo fmt --all --check`,
  `cargo test -p nimbus-bin macos_host_loader_auto_starts_default_machine_only_for_container_projects -- --nocapture`,
  `cargo test -p nimbus-bin host_loader_accepts_default_projects_with_ready_forwarded_machine_api_on_macos -- --nocapture`,
  `cargo test -p nimbus-bin macos_service_commands_use_forwarded_machine_api_for_container_projects -- --nocapture`,
  and `cargo check -p nimbus-bin`. Real-host proof on the existing isolated
  root `/tmp/nimbus-mac-closeout.FNcv0I/serve-proof-d4c-autostart` then started
  from `machine status = stopped`, launched `target/debug/nimbus serve ...`
  directly, and captured `/health = 200`, `machine_api.reachable = true`,
  `services:activate = 18080`, localhost service `200 ok`, native `/ws`
  initial plus pushed `subscription_result`, and tenant teardown withdrawing
  the forwarded localhost service again. Durable conclusion: the shared MLH
  lock/convergence contract now covers the host `serve` entrypoint too, and
  follow-on work returns to packaging/distribution rather than more machine
  lifecycle hardening.
- 2026-04-18: Tightened the checked-in macOS guest-binary convergence
  contract so the default path never depends on a locally built Linux guest
  artifact. `resolve_guest_nimbus_binary()` in
  `crates/nimbus-bin/src/machine/manager.rs` now has only two inputs:
  `NIMBUS_MACHINE_GUEST_BINARY` for an intentional local override, or the
  matching tagged GitHub release asset cached under Nimbus's machine cache.
  Deleted the old workspace `target/<triple>/{release,debug}/nimbus`
  auto-discovery path, updated the machine client/operator wording, and
  updated `docs/reference/macos-machine-flow.md` to describe the same
  release-asset-first contract. Added focused regression coverage proving the
  resolver reuses the cached release binary directly. Verification:
  `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine:: -- --nocapture`.
- 2026-04-18: Captured a fresh real-host macOS proof of that release-asset
  contract on isolated roots under `/tmp/nimbus-release-proof.aYbYTo` with no
  `NIMBUS_MACHINE_GUEST_BINARY` override and no preexisting Nimbus cache. The
  exact host flow was: `machine init --ssh-identity /tmp/nimbus-release-proof.keytest`,
  `machine start`, `machine status`, `machine ssh -- /usr/local/bin/nimbus --version`,
  and `scripts/collect-nimbus-machine-guest-proof.sh`, all using
  `HOME=/tmp/nimbus-release-proof.aYbYTo/home`,
  `NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-release-proof.aYbYTo/runtime`, and
  `/Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus`.
  First boot pulled the pinned Podman image into
  `home/.cache/nimbus/machine/images/1ca36ee640f03bf7ca59d45cadfdfa2dd2497b79e5c2030871d5798f336b96b4.raw.zst`,
  cached the matching guest asset at
  `home/.cache/nimbus/machine/guest-nimbus/v0.1.11-nimbus_linux_arm64-nimbus`,
  materialized the raw disk at
  `home/.local/share/nimbus/machine/default/images/default.raw`, and recorded
  the desired/recorded Podman digest match in `machine status`. Elevated host
  status then confirmed `machine_api.reachable: true`,
  `protocol_version: v1alpha2`, and the expected stock guest runtime binaries
  (`conmon`, `crun`, `netavark`, `aardvark-dns`). Guest SSH returned
  `nimbus 0.1.11` from `/usr/local/bin/nimbus`, and the proof bundle at
  `/tmp/nimbus-release-proof.aYbYTo/guest-proof-release-path` captured guest
  version, SHA-256, socket/service state, virtiofs `/Users`, and guest
  machine-API `/healthz` plus `/v1/machine-api/capabilities` over the booted
  VM's own `/run/nimbus/nimbus.sock`. A warm stop/start on the same isolated
  root then returned to `running`/`ready` on the same cached Podman digest and
  guest asset without any override, and the stop-path runtime cleanup
  converged back to log files only after a brief teardown settle, with no
  surviving `krunkit` or `gvproxy` processes. Durable conclusion: the
  enterprise macOS happy path is now proved against the real release-asset
  contract instead of a locally built guest binary.
- 2026-04-18: Closed the remaining operator-visibility gap on that contract by
  adding a `guest_binary_contract` section to `nimbus machine status` for the
  host-managed macOS path. Status now reports whether the desired guest binary
  comes from the tagged GitHub release asset or an explicit
  `NIMBUS_MACHINE_GUEST_BINARY` override, the desired cache/install paths, the
  desired version/hash when available locally, and the observed guest
  `/usr/local/bin/nimbus` version/hash when the machine is running. This keeps
  the convergence contract inspectable without reintroducing local-worktree
  fallback or hidden operator state. Focused regression coverage now exercises
  both release-asset and explicit-override status rendering. Verification:
  `cargo fmt --all`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine_status_ -- --nocapture`.
