# Plan: Machine Lifecycle Hardening — Podman-Aligned Robustness

Canonical execution plan for hardening the neovex machine lifecycle to match
Podman's source-backed robustness patterns. These items apply to the existing
macOS krunkit path and the future Windows WSL2 path, and were identified
through source-backed review of Podman's machine infrastructure.

Reviewed against:

- `crates/neovex-bin/src/machine/manager.rs` — current krunkit+gvproxy
  orchestration
- `crates/neovex-bin/src/machine/mod.rs` — machine config/state types,
  `write_json_file`
- `crates/neovex-bin/src/machine/protocol.rs` — machine API protocol types
- `crates/neovex-bin/src/machine/client.rs` — machine API client
- `crates/neovex-bin/src/machine/backend.rs` — forwarded sandbox backend
- `crates/neovex-bin/src/service/mod.rs` — `ServiceHostPlatform` and backend
  routing
- `docs/plans/macos-machine-support-plan.md` — active macOS execution plan
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

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** macOS `MAC6` complete — the transparent developer UX
  should be working before we harden the lifecycle underneath it. These items
  are not blockers for basic macOS functionality but are blockers for
  production-quality machine management.
- **Related plans:**
  - `docs/plans/macos-machine-support-plan.md` — active macOS plan; this
    hardening plan should be executed between `MAC6` and `MAC7` or as a
    follow-up after `MAC7`
  - `docs/plans/windows-machine-support-plan.md` — deferred Windows plan;
    `WIN2` should build on the hardened infrastructure from this plan

## Current Assessed State

- The machine manager (`crates/neovex-bin/src/machine/manager.rs`) implements
  krunkit+gvproxy orchestration for macOS with basic lifecycle management.
- SSH readiness uses a 2-layer check (port listening + SSH exec) that mirrors
  Podman's approach. The code explicitly comments: "Mirror Podman's macOS
  machine layering."
- Stale state detection works: `refresh_machine_state()` checks PID liveness
  and transitions dead machines to `Failed`/`Stale`.
- However, several Podman robustness patterns are missing. Each was identified
  by reading Podman source and comparing against the neovex implementation.

## Current Review Findings

### 1. Provider-specific graceful stop sequencing — under-hardened

**Podman source:** the WSL provider's `StopVM()` in
`pkg/machine/wsl/stubber.go` runs `enterns systemctl exit 0` with a 60-second
timeout before calling `wsl --terminate`. The Apple/libkrun provider does
*not* SSH into the guest; it stays on the provider seam and calls
`mc.LibKrunHypervisor.KRun.Stop(hardStop, true)`, which lowers to vfkit's
state-change API and a bounded wait/hard-stop fallback in
`pkg/machine/apple/vfkit/helper.go`.

**Neovex current:** `stop_machine()` in `manager.rs` sends HTTP
`POST /vm/state {"state":"Stop"}` to krunkit's REST API. If that fails,
SIGTERM → SIGKILL the krunkit process. This is the correct control seam for
the macOS krunkit path, but the plan text currently over-generalizes WSL's
guest-systemd stop flow onto macOS.

**Risk:** Adding an ad hoc SSH shutdown path on macOS would diverge from
Podman's battle-tested provider-owned libkrun lifecycle. At the same time, the
current Neovex stop path still lacks an explicit Podman-shaped bounded wait and
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

**Neovex current:** `start_machine()` in `manager.rs` spawns gvproxy and
krunkit as child processes but does not register signal handlers. If the user
presses Ctrl-C during startup, the state file may be left as
`lifecycle: Starting` with stale PID references.

**Risk:** After a Ctrl-C during startup, `neovex machine status` shows
`starting` forever. The user must `neovex machine rm` and recreate.
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

**Neovex current:** `allocate_local_port()` binds to `127.0.0.1:0` and reads
the OS-assigned ephemeral port. Reuses the previous port on restart (stored in
`MachineRuntimeState.ssh_port`). No file locking, no conflict detection, no
reassignment.

**Risk:**
- Two concurrent `neovex machine init` calls could theoretically allocate the
  same ephemeral port (unlikely but possible)
- A port allocated in a previous session could be taken by another application
  by the time the machine restarts, causing a hard startup failure with no
  automatic recovery
- When Windows support lands, multiple machines can run concurrently
  (`RequireExclusiveActive = false` for WSL2), making port collisions more
  likely

**Fix:** Implement Podman's file-locked port allocation:
- Lock file at the shared machine state root
  (for example `${XDG_STATE_HOME:-~/.local/state}/neovex/machine/port-alloc.lck`)
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

**Neovex current:** `write_json_file()` in `mod.rs` uses direct `fs::write()`
— no locking, no atomic write pattern.

**Risk:** If `neovex machine status` reads the config while
`neovex machine start` is writing it, the reader could see partial or corrupt
JSON. This is unlikely in a single-user CLI today but becomes more likely when
`neovex serve` auto-manages the machine lifecycle (concurrent reads and writes
from the server and CLI). Podman's single-config-file lock is not sufficient
guidance on its own because Neovex persists two coupled records:
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

**Neovex current:** `MachineConfigRecord` and `MachineStateRecord` have no
version field.

**Risk:** If the config schema changes in a future neovex release, existing
machine configs will fail to deserialize with an opaque serde error. The user
must `rm` and `init` a new machine, losing their configuration. Adding a
version field only to `MachineConfigRecord` would still leave
`MachineStateRecord` as an opaque failure mode, even though Neovex loads both
records on ordinary machine commands.

**Fix:** Add `version: u32` to `MachineConfigRecord` and an explicit version or
schema tag to `MachineStateRecord`. Set `CURRENT_MACHINE_CONFIG_VERSION = 1`
and `CURRENT_MACHINE_STATE_VERSION = 1`. On load:
- If version matches: proceed normally
- If version is older: migrate or return a clear error with upgrade
  instructions
- If version is newer: return a clear error ("this machine was created with a
  newer version of neovex")
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

**Neovex current:** `MachineProvider` enum has only `Krunkit`. All startup
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

**Neovex current:** `start_machine()` runs everything sequentially in one
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

| Concern | Podman | Neovex current | Target |
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
3. `docs/plans/macos-machine-support-plan.md` (macOS execution context)
4. `docs/plans/windows-machine-support-plan.md` (Windows execution context)
5. the reviewed Podman source files listed at the top of this document

General rules:

- These items harden existing infrastructure. They do not change the macOS or
  Windows architecture — those are owned by their respective plans.
- Do not block macOS `MAC4`-`MAC6` on these items. They are robustness
  improvements, not functional requirements for the first working macOS flow.
- Items that are shared between macOS and Windows (port allocation, config
  persistence, config versioning, provider flags, networking phases) should
  land before `WIN2` so the Windows provider builds on hardened infrastructure.
- Items that are macOS-specific (provider-driven krunkit stop hardening and
  signal handling) should land before `MAC7` closeout.
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

This plan does not cover:

- macOS machine architecture (owned by macOS plan)
- Windows machine architecture (owned by Windows plan)
- new providers or backends
- machine image artifacts or guest bootstrap content

## Verification Contract

### Minimum verification for every code item

- `cargo fmt --all --check`
- focused `cargo check -p neovex-bin`
- focused `cargo test -p neovex-bin` for the machine module
- plan ledger and execution-log update in the same change set

### Required verification lanes

- **Graceful stop lane**
  - macOS krunkit stop uses the provider control surface first and waits
    through the bounded grace period before hard-stop escalation
  - future WSL stop exits nested systemd before `wsl --terminate`
  - timeout behavior is explicit and testable

- **Signal handling lane**
  - Ctrl-C during `neovex machine start` transitions to `Stopped`
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
| MLH1 | todo | Provider-specific graceful stop sequencing: macOS krunkit stop stays on the control seam with bounded wait/hard-stop; future WSL exits nested systemd before terminate | none |
| MLH2 | todo | Signal handling during startup: SIGINT/SIGTERM handler kills children, transitions to Stopped | none |
| MLH3 | todo | Global SSH port allocation: file-locked pool, conflict reassignment on startup, release on removal | none |
| MLH4 | todo | Atomic config/state writes: temp file + rename under a per-machine lock spanning both records | none |
| MLH5 | todo | Machine record versioning: versioned config plus explicit state compatibility/rebuild policy | none |
| MLH6 | todo | Provider capability flags: `uses_provider_networking`, `requires_exclusive_active`, `image_format` on `MachineProvider` | none |
| MLH7 | todo | Explicit pre/post-start networking phases: factor `start_machine` into phased orchestration | MLH6 |

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

- `neovex machine stop` on macOS uses the provider control surface first, not
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

- Ctrl-C during `neovex machine start` does not leave state as `Starting`
- gvproxy and krunkit processes are cleaned up on signal
- `neovex machine status` after Ctrl-C shows `stopped`, not `starting`

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
- concurrent `neovex machine init` calls do not allocate the same port
- `neovex machine rm` releases the port back to the pool

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
  - older version: migrate (or clear error with upgrade instructions)
  - newer version: clear error ("created with newer neovex version")
- unsupported or unreadable state: reset/rebuild state explicitly instead of
  surfacing a raw parse failure
- migration function placeholder for future version bumps

Acceptance criteria:

- existing (versionless) configs are treated as version 0 and either
  auto-migrated or rejected with a clear message
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

- 2026-04-15: Created this plan based on source-backed review of Podman's
  machine infrastructure against the existing neovex machine module. Identified
  7 robustness gaps by comparing `manager.rs` against Podman's `shim/host.go`,
  `ports/ports.go`, `vmconfigs/machine.go`, `wsl/stubber.go`, and
  `vmconfigs/config.go`. Each gap has a concrete Podman source reference and a
  specific fix. Plan is deferred until macOS MAC6 is complete.
- 2026-04-15: Corrected the plan after a deeper source-backed review against
  the current Neovex machine module and Podman's Apple/WSL providers. Reframed
  `MLH1` around provider-specific stop sequencing instead of incorrectly
  copying WSL's guest-systemd shutdown flow onto macOS libkrun; moved SSH port
  allocation artifacts to the shared machine-state root; upgraded `MLH4` to
  require a per-machine lock spanning both `config.json` and `status.json`;
  and upgraded `MLH5` to cover Neovex's split config/state persistence model
  with an explicit state rebuild policy. This keeps Neovex aligned with
  Podman's battle-tested machine-management seams without introducing
  unnecessary divergence on the krunkit path.
