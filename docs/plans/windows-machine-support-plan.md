# Plan: Windows Machine Support — Podman-Aligned Developer Machines

Canonical execution plan for finishing Nimbus Windows support for engineers who
develop on Windows and deploy to Linux production hosts.

Reviewed against:

- `docs/architecture/sandbox/microvm-service-baseline.md`
- `docs/architecture/sandbox/macos-machine-flow.md`
- `docs/plans/archive/macos-machine-support-plan.md`
- `docs/plans/distribution-plan.md`
- `crates/nimbus-bin/src/machine/mod.rs`
- `crates/nimbus-bin/src/machine/stub/`
- `crates/nimbus-bin/src/compose/mod.rs`
- `crates/nimbus-sandbox/src/backends/`
- `.github/workflows/release.yml` — confirms `nimbus.exe` already builds on
  `windows-latest` with `x86_64-pc-windows-msvc` and ships in every release
- Podman Windows provider source (source-backed review, not documentation):
  - `pkg/machine/provider/platform_windows.go` — provider selection, VMType
    enum, permission checks
  - `pkg/machine/wsl/stubber.go` — `WSLStubber` type, `CreateVM`, `StartVM`,
    `UseProviderNetworkSetup() = true`, `RequireExclusiveActive() = false`
  - `pkg/machine/wsl/machine.go` — WSL distro config: systemd namespace
    bootstrap, SSH setup, user creation, containers.conf, bind mounts at
    `/mnt/wsl/podman-sockets/{dist}/`
  - `pkg/machine/wsl/declares.go` — bootstrap script:
    `nohup unshare --kill-child --fork --pid --mount --mount-proc /lib/systemd/systemd`
  - `pkg/machine/wsl/usermodenet.go` — optional user-mode networking via
    separate `podman-net-usermode` WSL distro running gvproxy/gvforwarder
  - `pkg/machine/wsl/wutil/wutil.go` — WSL command wrappers:
    `wsl --import`, `wsl -u root -d`, `wsl --terminate`, `wsl --unregister`,
    `CREATE_NO_WINDOW` creation flags
  - `pkg/machine/hyperv/stubber.go` — `HyperVStubber` type, `CreateVM`,
    `StartVM`, vsock registry entries for network and ready signals,
    `UseProviderNetworkSetup() = false`, ignition via Windows Registry chunks
  - `pkg/machine/hyperv/vsock/vsock.go` — `HVSockRegistryEntry` type,
    registry path at
    `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Virtualization\GuestCommunicationServices`,
    GUID-based port allocation (49152-65535)
  - `pkg/machine/hyperv/volumes.go` — 9P file shares via hvsock, per-mount
    registry entries, `podman machine client9p`/`server9p` commands
  - `pkg/machine/vmconfigs/config.go` — `VMProvider` interface,
    `MachineConfig` struct with provider-specific config fields
  - `pkg/machine/shim/host.go` — shim orchestrator: `Init`, `Start`, `Stop`,
    `Remove`, `Reset`, signal handlers, exclusive-active VM enforcement
  - `pkg/machine/shim/networking.go` — `startHostForwarder` (gvproxy launch),
    `conductVMReadinessCheck` (state + SSH port + SSH exec layered check,
    exponential backoff 500ms×6), two-path networking split
    (`UseProviderNetworkSetup` vs gvproxy)
  - `pkg/machine/shim/networking_windows.go` — `setupMachineSockets` returns
    named pipe paths (`npipe:////./pipe/{name}`), optional
    `docker_engine` global pipe claim
  - `pkg/machine/machine_windows.go` — `LaunchWinProxy`, `StopWinProxy`,
    `PipeNameAvailable`, `WaitPipeExists`, `DialNamedPipe`, named pipe
    constants, TID-file lifecycle, `PostThreadMessageW` shutdown
  - `pkg/machine/shim/diskpull/diskpull.go` — `Disker` interface, OCI/URL/local
    pull paths, `VMType.ImageFormat()` returns `Tar` for WSL, `Vhdx` for HyperV
  - `pkg/machine/ports/ports.go` — global SSH port allocation with file-based
    locking, `AllocateMachinePort`, `ReleaseMachinePort`, collision detection
  - `pkg/machine/ignition/ready.go` — per-provider ready unit generation:
    virtio serial (QEMU), vsock (Apple/HyperV/LibKrun), no ready unit for WSL
  - `pkg/machine/gvproxy.go` — `CleanupGVProxy`, PID file management
  - `pkg/machine/gvproxy_windows.go` — `winquit.QuitProcess` with 30s grace
  - `vendor/github.com/containers/gvisor-tap-vsock/pkg/types/gvproxy_command.go`
    — `GvproxyCommand` builder: `AddEndpoint`, `AddForwardSock`,
    `AddForwardDest`, `AddForwardUser`, `AddForwardIdentity`
  - `Makefile` (lines 844-854) — gvproxy/win-sshproxy download from
    `containers/gvisor-tap-vsock` releases, arch-specific:
    `gvproxy-windowsgui.exe`, `win-sshproxy.exe`, `win-sshproxy-arm64.exe`

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** not met — requires macOS plan (`MAC5`+) to stabilize the
  hybrid host-resident control-plane pattern and the forwarded machine-API seam,
  since Windows reuses that same architecture with a different VM provider and
  transport layer. That macOS gate is now satisfied by the archived MAC1-MAC7
  closeout record; Windows remains deferred only because this plan itself has
  not been promoted.
- **Related plans:**
  - `docs/architecture/sandbox/macos-machine-flow.md` — current macOS developer-machine
    contract reference; Windows shares the hybrid control-plane architecture
    and the forwarded machine-API contract
  - `docs/plans/archive/macos-machine-support-plan.md` — completed macOS
    execution record with the exact proof and sequencing Windows can reuse
  - `docs/architecture/sandbox/microvm-service-baseline.md` — current landed Linux
    microVM and service-control baseline
  - `docs/plans/distribution-plan.md` — packaging/distribution umbrella; this
    plan owns the detailed execution of the Windows row
  - `docs/plans/archive/machine-lifecycle-hardening-plan.md` — completed
    shared machine lifecycle hardening record (port allocation, config
    persistence, provider capability flags, networking phases); `WIN2` should
    build on that landed hardened infrastructure

## Current Assessed State

- Linux production support is complete and stable in the landed baseline.
- macOS support is in progress and owns the hybrid control-plane architecture
  that Windows will reuse: host-resident Nimbus server, guest-resident narrow
  machine API, forwarded socket transport, standard guest containers.
- Windows support does not exist as a developer platform. However, the repo
  **already builds and ships `nimbus.exe`** for `x86_64-pc-windows-msvc` in
  every release (`.github/workflows/release.yml`). V8/deno_core compiles on
  Windows today via GitHub Actions `windows-latest` runners. The remaining work
  is machine lifecycle, transport, and developer workflow — not compilation.
- Shared machine-image policy is now more explicit too: the active macOS/MAC4
  decision is FCOS-first for raw-disk VM providers, but that does not change
  WSL2's provider-specific tarball and shell-bootstrap contract. The shared
  `nimbus/nimbus-machine-os` repo may carry separate artifact families
  for FCOS raw/vhdx providers, WSL tar roots, and future `fedora-bootc`
  experiments without collapsing them into one runtime contract.
- The machine module stubs all lifecycle operations with explicit errors on
  non-Unix hosts. The sandbox backends gate on Linux at runtime.
- The distribution plan lists Windows as `Future` with `WSL2` as the execution
  model and `TBD` for service isolation strategy.
- The current `ServiceHostPlatform` enum has an `Other` variant that Windows
  falls into, but it is not wired to any sandbox backend initialization.
- Windows ARM64 is commented out in the release workflow, waiting only on a
  free GitHub `windows-11-arm` runner. V8 prebuilts are already available.

## Current Review Findings

- Podman remains the canonical implementation reference for Nimbus's Windows
  machine architecture, just as it is for macOS.
- Podman on Windows supports **two VM provider backends**: WSL2 (default) and
  Hyper-V. The key distinction for Nimbus is provider behavior and transport:
  WSL2 owns its networking/bootstrap path, while Hyper-V is closer to the
  Apple/libkrun family and remains deferred.

### Podman's VMProvider interface

All providers implement a common `VMProvider` interface defined in
`pkg/machine/vmconfigs/config.go`. Key methods:

```go
type VMProvider interface {
    CreateVM(opts define.CreateVMOpts, mc *MachineConfig, builder *ignition.IgnitionBuilder) error
    StartVM(mc *MachineConfig) (func() error, func() error, error)  // (releaseCmd, waitForReady, err)
    StopVM(mc *MachineConfig, hardStop bool) error
    State(mc *MachineConfig, bypass bool) (define.Status, error)
    StartNetworking(mc *MachineConfig, cmd *gvproxy.GvproxyCommand) error
    PostStartNetworking(mc *MachineConfig, noInfo bool) error
    UseProviderNetworkSetup() bool     // true=provider owns networking, false=gvproxy
    RequireExclusiveActive() bool      // true=only one VM at a time
    MountType() VolumeMountType
    MountVolumesToVM(mc *MachineConfig, quiet bool) error
    // ... Remove, Set, PrepareIgnition, etc.
}
```

This maps to Nimbus's `SandboxBackend` trait pattern. The
`UseProviderNetworkSetup()` and `RequireExclusiveActive()` capability flags are
the most architecturally important: they determine whether the shim layer runs
gvproxy or delegates networking to the provider, and whether multiple machines
can coexist.

### WSL2 provider — source-backed architecture

The WSL2 provider (`pkg/machine/wsl/`) is architecturally distinct from the
Apple and Hyper-V providers in several critical ways:

**Bootstrap is shell-script-based, not ignition-based.**
WSL does not use Fedora CoreOS or ignition. Instead:
1. `podman machine init` downloads a rootfs tarball and imports it as a WSL2
   distribution via `wsl --import {name} {path} {tarball} --version 2`.
2. Configuration happens via direct `wsl -u root -d {dist}` shell commands:
   SSH port setup, user creation with wheel group, sudoers `NOPASSWD`, systemd
   override, containers.conf generation, registry config, linger enablement.
3. The distribution name is prefixed: `podman-{machineName}`.

**Systemd runs in a nested namespace.**
WSL2's init is not systemd. Podman bootstraps systemd inside the WSL2 distro
via:
```bash
nohup unshare --kill-child --fork --pid --mount --mount-proc \
  --propagation shared /lib/systemd/systemd
```
Users enter the namespace via an `enterns` script. This means the WSL2 distro
has a double-nested process tree and users must exit twice to logout.

**WSL owns its own networking (`UseProviderNetworkSetup() = true`).**
Unlike Apple and Hyper-V providers, WSL does **not** use gvproxy for basic
networking. WSL2 provides its own virtual networking (NAT or mirrored mode).
The gvproxy role splits into two separate concerns:

1. **API forwarding** — handled by **win-sshproxy** (launched in
   `PostStartNetworking()`), which creates named pipes and tunnels to the guest
   podman socket via SSH. This is always used, regardless of networking mode.

2. **Container port forwarding** — handled by WSL2's own networking (ports
   bound inside WSL2 are accessible from Windows localhost in most
   configurations). An optional **user-mode networking** path exists for
   environments where WSL2's native networking is insufficient: this creates a
   separate `podman-net-usermode` WSL2 distro running gvproxy/gvforwarder with
   a static gateway IP (192.168.127.1).

This is a critical architectural difference from macOS: on macOS, gvproxy is
the universal networking and API-forwarding component. On Windows with WSL2,
gvproxy is **optional** (user-mode networking only), and win-sshproxy handles
API forwarding separately.

**WSL allows multiple concurrent machines (`RequireExclusiveActive() = false`).**
Unlike Apple/Hyper-V providers, WSL2 does not enforce single-active-VM
semantics. Multiple WSL2 distributions can run simultaneously.

**WSL image format is Tar, not Raw or VHDX.**
`VMType.ImageFormat()` returns `Tar` for WSL vs `Raw` for Apple/LibKrun vs
`Vhdx` for Hyper-V. The disk image pipeline diverges at this point.

**No ready signal unit for WSL.**
`ignition/ready.go` returns an empty ready unit for WSL (`case define.WSLVirt:
return "", nil`). WSL bootstrap is synchronous — the `StartVM` return value
includes a trivial ready function (`nil, readyFunc, nil`) because the distro
is considered ready once the bootstrap shell commands complete.

**Nimbus-specific implication: WSL auto-mounts do not eliminate path work.**
WSL makes Windows files visible inside the guest at `/mnt/<drive>/...`, but
because Nimbus keeps compose parsing and service planning on the Windows host,
Windows support still needs an explicit host-path translation seam for build
contexts, Dockerfiles, env files, bind mounts, and working directories. Reuse
WSL's mounts; do not invent a second file-sharing layer.

### Hyper-V provider — source-backed architecture (deferred)

The Hyper-V provider (`pkg/machine/hyperv/`) is architecturally closer to
the Apple/LibKrun providers. Deferred for Nimbus, documented here for future
reference only:

- Uses ignition, delivered via Windows Registry chunks (not vsock)
- Uses vsock for networking (`UseProviderNetworkSetup() = false`), gvproxy
  launched on the Windows host
- Uses 9P for volume mounts via hvsock (not virtiofs)
- Requires admin for first machine (registry entries)
- Enforces single active VM (`RequireExclusiveActive() = true`)
- Uses VHDX image format

### win-sshproxy — source-backed architecture

win-sshproxy is sourced from `containers/gvisor-tap-vsock` (same repo as
gvproxy). It is the universal Windows API-forwarding component, used by **both**
WSL2 and Hyper-V providers.

**Launch flow** (`pkg/machine/machine_windows.go`, `launchWinProxy()`):
1. Checks pipe availability with timeouts:
   - Machine pipe: 5s (`MachineNameWait`)
   - Global docker pipe: 250ms (`GlobalNameWait`)
2. Constructs command line:
   ```
   win-sshproxy.exe {name} {stateDir} \
     npipe:////./pipe/podman-{name} ssh://{user}@localhost:{port}{socket} {identity} \
     [npipe:////./pipe/docker_engine ssh://{user}@localhost:{port}{socket} {identity}] \
     [{host_url} ssh://{user}@localhost:{port}{socket} {identity}]
   ```
3. Socket mapping:
   - Rootful: `/run/podman/podman.sock` with `root` user
   - Rootless: `/run/user/1000/podman/podman.sock` with remote username

**Named pipe naming convention:**
- Machine-specific: `\\.\pipe\podman-{machineName}` (always created)
- Global Docker: `\\.\pipe\docker_engine` (optional, claimed if available)
- URI format: `npipe:////./pipe/{name}`

**Lifecycle management:**
- PID and thread ID stored in `{stateDir}/win-sshproxy.tid` as `{pid}:{tid}`
- Clean shutdown via `PostThreadMessageW` with `WM_QUIT` (0x12)
- Fallback to `TerminateProcess()` on graceful shutdown failure
- Startup verification: polls for pipe existence (80 retries × 250ms = 20s)

**Dependencies:**
- `github.com/Microsoft/go-winio` for named pipe I/O
- `github.com/containers/winquit` for graceful Windows process termination

### gvproxy on Windows — source-backed architecture

gvproxy's role on Windows is provider-dependent:

**With WSL2:** gvproxy is **not launched by default**. Only used in optional
user-mode networking mode, where it runs inside a separate
`podman-net-usermode` WSL2 distro. The guest-side component is `gvforwarder`
(installed at `/usr/libexec/podman/gvforwarder`).

**With Hyper-V (deferred):** gvproxy is launched on the Windows host by the
shim layer's `startHostForwarder()`, same as Apple/LibKrun providers. It
connects to the VM via a vsock endpoint registered in the Windows Registry.

**Binary sourcing:**
- Downloaded from `containers/gvisor-tap-vsock` releases
- AMD64: `gvproxy-windowsgui.exe`, `win-sshproxy.exe`
- ARM64: `gvproxy-windows-arm64.exe`, `win-sshproxy-arm64.exe`
- Found at runtime via `FindHelperBinary("gvproxy", false)` which searches
  `CONTAINERS_HELPER_BINARY_DIR`, `containers.conf` helper dirs, and the
  directory containing the executable

### Readiness check — source-backed architecture

`conductVMReadinessCheck()` in `pkg/machine/shim/networking.go` implements a
three-layer readiness check with exponential backoff:

1. **VM state check** — calls `provider.State()`, requires `define.Running`
2. **SSH port listening** — checks if the SSH port is accepting TCP connections
3. **SSH command execution** — runs `ssh {user}@localhost:{port} true`

Parameters: `maxBackoffs = 6`, initial `backoff = 500ms`, doubles each
iteration. Total potential wait: ~30 seconds.

**WSL divergence:** WSL's `StartVM` returns a trivial ready function because
bootstrap is synchronous. The full readiness check still runs afterward
(in `PostStartNetworking`) to verify the podman socket is answering, but the
"ready signal" concept from ignition/vsock does not apply.

### SSH port allocation — source-backed architecture

`pkg/machine/ports/ports.go` implements global SSH port allocation:
- File-based lock at `{globalDataDir}/port-alloc.lck`
- Allocation state at `{globalDataDir}/port-alloc.dat` (JSON)
- Port range: 10000-65535
- On startup, if allocated port is in use: `reassignSSHPort()` releases old,
  allocates new, updates connection config via
  `connection.UpdateConnectionPairPort()`

### Machine config persistence — source-backed architecture

`pkg/machine/vmconfigs/machine.go`:
- Stored as JSON at `{configDir}/{machineName}.json`
- Locked during operations via `lockfile.LockFile`
- Atomic writes via `ioutils.AtomicWriteFile()`
- Provider-specific config stored as nullable fields:
  ```go
  AppleHypervisor   *AppleHVConfig
  HyperVHypervisor  *HyperVConfig
  WSLHypervisor     *WSLConfig
  // etc.
  ```

### Windows elevation handling — source-backed architecture

`pkg/machine/windows/util_windows.go`:
- `HasAdminRights()` — checks if current process is elevated
- `RelaunchElevatedWait()` — uses `ShellExecuteExW` with `runas` verb
- Adds `--reexec` flag to prevent infinite elevation loops
- Output from elevated child captured to
  `{dataHome}/podman-elevated-output.log`

Not needed for the WSL2 provider (no admin required), but documented for
future Hyper-V support.

### Podman WSL2 exact startup sequence — source-backed

The exact ordering of events during `podman machine start` for WSL is
critical. Nimbus must mirror this sequence:

```text
1. Lock acquisition (mc.Lock())
2. State validation (machine not already running)
3. Signal handler setup (SIGINT/SIGTERM → set Starting=false)
4. startNetworking():
   ├── SSH port availability check (reassign if conflict)
   ├── provider.UseProviderNetworkSetup() → true (WSL branch)
   └── provider.StartNetworking(mc, nil) → no-op unless user-mode networking
5. StartVM():
   └── wslInvoke(dist, "/root/bootstrap")
       └── bootstrap script checks if systemd already running (idempotent)
       └── if not: unshare --kill-child --fork --pid --mount --mount-proc ...
6. WaitForReady():
   └── immediate no-op for WSL (bootstrap is synchronous)
7. PostStartNetworking():               ← win-sshproxy launches HERE
   ├── Get APISocket path
   ├── Build WinProxyOpts (name, identity, port, rootful, socket)
   └── LaunchWinProxy():
       ├── Check machinePipe available (5s timeout)
       ├── Try claim docker_engine pipe (250ms timeout)
       ├── Find win-sshproxy.exe binary
       ├── Build args: name, stateDir, pipe, ssh://user@localhost:port/sock, identity
       ├── cmd.Start() → launch win-sshproxy.exe process
       └── WaitPipeExists() → poll 80 retries × 250ms = 20s
8. conductVMReadinessCheck():           ← AFTER win-sshproxy
   ├── provider.State() == Running?
   ├── isListening(mc.SSH.Port)?
   └── ssh user@localhost:port true
   └── exponential backoff: 500ms × 6 iterations (~30s max)
9. Volume mounts (MountVolumesToVM)
10. Signal handler cleanup
```

### Podman WSL2 exact stop sequence — source-backed

```text
1. Graceful systemd shutdown:
   └── wslInvoke(dist, "/usr/local/bin/enterns", "systemctl", "exit", "0")
   └── 60-second timeout waiting for systemd to exit
2. Terminate WSL distribution:
   └── wsl --terminate nimbus-{name}
3. Stop win-sshproxy:
   └── Read TID from {stateDir}/win-sshproxy.tid
   └── PostThreadMessageW(tid, WM_QUIT)
   └── Fallback: TerminateProcess() on graceful failure
```

### Podman WSL2 exact bootstrap content — source-backed

**`/root/bootstrap`** (installed during CreateVM):
```bash
#!/bin/bash
ps -ef | grep -v grep | grep -q systemd && exit 0
nohup unshare --kill-child --fork --pid --mount --mount-proc \
  --propagation shared /lib/systemd/systemd >/dev/null 2>&1 &
sleep 0.1
```
Idempotent: exits immediately if systemd is already running.

**`/usr/local/bin/enterns`** (installed during CreateVM):
- Gets systemd PID via `ps -eo cmd,pid | grep -m 1 ^/lib/systemd/systemd`
- Uses `nsenter -m -p -t <PID>` to enter systemd namespace
- Elevates to proper user

**`containers.conf`** (installed during configureSystem):
```ini
[containers]

[engine]
cgroup_manager = "cgroupfs"
```

### Podman WSL2 command wrapping — source-backed

All WSL commands must follow these patterns from `wutil/wutil.go`:

```go
// Every WSL invocation:
cmd.Env = append(os.Environ(), "WSL_UTF8=1")

// Silent/background commands:
cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: 0x08000000}  // CREATE_NO_WINDOW
```

Nimbus must set `WSL_UTF8=1` on every `wsl.exe` invocation and use
`CREATE_NO_WINDOW` for background WSL commands to suppress console windows.

### Nimbus machine module — current patterns to extend

The existing nimbus machine module uses these patterns that Windows support
must follow:

**Cfg gating**: `#[cfg(unix)]` for macOS/Linux, `#[cfg(not(unix))]` for stubs.
Windows WSL2 support needs `#[cfg(target_os = "windows")]` modules alongside
the existing stubs, so the module structure becomes:

```rust
// mod.rs
#[cfg(unix)]
mod manager;                        // macOS krunkit+gvproxy
#[cfg(target_os = "windows")]
#[path = "wsl/manager.rs"]
mod manager;                        // Windows WSL2
#[cfg(not(any(unix, target_os = "windows")))]
#[path = "stub/manager.rs"]
mod manager;                        // Unsupported platforms
```

**MachineProvider enum**: Currently has only `Krunkit`. Add `Wsl2`:
```rust
enum MachineProvider {
    Krunkit,   // macOS: krunkit + gvproxy
    Wsl2,      // Windows: WSL2 distro + win-sshproxy
}
```

**MachineGuestConfig divergence**: macOS uses `ignition_file_path` and
`efi_variable_store_path`. WSL2 uses neither. Make these `Option` (already
are) and document that WSL2 leaves them `None`.

**MachineApiClient transport**: Currently uses `std::os::unix::net::UnixStream`
for HTTP-over-Unix-socket. Windows needs HTTP-over-named-pipe using
`tokio::net::windows::named_pipe` or equivalent. The HTTP protocol and
request/response format stays identical — only the underlying transport
changes.

**Directory layout on Windows**: macOS uses `/tmp/nimbus` (short runtime root)
and `~/.config/nimbus/machine`. Windows should use:
- Config: `%APPDATA%\nimbus\machine\` (or `%LOCALAPPDATA%`)
- State: `%LOCALAPPDATA%\nimbus\machine\`
- Runtime: `%LOCALAPPDATA%\nimbus\machine\run\` (no Unix socket path length
  concern on Windows since named pipes have their own namespace)
- WSL distro data: `%LOCALAPPDATA%\nimbus\machine\wsldist\{name}\`

**ServiceHostPlatform**: Currently `Macos | Linux | Other`. Replace `Other`
with `Windows`:
```rust
enum ServiceHostPlatform {
    Macos,
    Linux,
    Windows,   // NEW: routes to named-pipe forwarded machine API
}
```

**Default volumes**: macOS defaults to `[/Users -> /Users]` via virtiofs.
WSL2 auto-mounts Windows drives at `/mnt/c/`, `/mnt/d/`, etc. via Plan9/9P.
That means Nimbus does **not** need an extra VM file-sharing mechanism for the
default Windows path story. It *does* still need explicit Windows-host-path →
WSL-guest-path translation for compose-backed guest operations because the
host-resident Nimbus control plane currently keeps those paths as host
`PathBuf`s.

**Guest image artifact**: nimbus-machine-os may ship multiple provider-specific
artifact families. macOS currently targets a Raw FCOS-based artifact; WSL2
still needs a Tar rootfs artifact. The cross-repo build contract therefore
needs a WSL-specific image variant
that includes:
- the `nimbus` Linux binary (for guest machine API)
- `nimbus.socket` and `nimbus.service` systemd units
- standard container runtime: `buildah`, `conmon`, `crun`, `netavark`,
  `aardvark-dns`, `fuse-overlayfs`
- SSH server (`sshd`)
- base Fedora userland

## Podman Alignment Matrix

| Concern | Podman on Windows | Nimbus target on Windows | Alignment decision |
| --- | --- | --- | --- |
| Host topology | `podman.exe` manages one or more Linux machine VMs | `nimbus.exe` plus `nimbus machine ...` manage one Linux machine VM | match machine topology, deliberate DX divergence on control-plane placement |
| Host application/runtime | no host-resident app/runtime analogue | authoritative Nimbus API, V8 runtime, and storage stay on the Windows host | deliberate divergence for local DX (same rationale as macOS) |
| Provider interface | common `VMProvider` interface with capability flags | extend the existing machine provider abstraction with `Wsl2` capability values | match the provider-abstraction pattern without inventing a second provider model |
| VM provider | WSL2 (default, `UseProviderNetworkSetup=true`) or Hyper-V (`=false`) | WSL2 only; Hyper-V deferred | match default provider, defer secondary |
| Guest bootstrap | WSL: shell commands (no ignition); Hyper-V: ignition via registry | WSL: shell commands matching Podman's pattern | match WSL bootstrap model, not macOS ignition model |
| Guest systemd | nested namespace via `unshare --kill-child --fork --pid ...` | same nested namespace pattern for `nimbus.service` | match WSL systemd pattern |
| Guest control plane | guest `podman.socket` / Podman API | guest `nimbus.socket` exposing `/run/nimbus/nimbus.sock` | match socket-naming pattern, narrower API (same as macOS) |
| Guest workload implementation | standard guest containers | standard guest containers | match |
| API forwarding | win-sshproxy creates named pipes, tunnels via SSH (both providers) | win-sshproxy (from `containers/gvisor-tap-vsock`) or Nimbus-owned equivalent | match transport pattern |
| Named pipe naming | `\\.\pipe\podman-{machineName}` (machine) + optional `docker_engine` (global) | `\\.\pipe\nimbus-machine-{machineName}` (machine only, no Docker compat) | match naming convention, narrower scope |
| Container port forwarding | WSL: native WSL2 networking; Hyper-V: gvproxy via vsock | WSL: native WSL2 networking (no gvproxy needed) | match per-provider networking split |
| Optional user-mode networking | separate `podman-net-usermode` WSL2 distro with gvproxy/gvforwarder | defer — only add if WSL2 native networking proves insufficient | match architecture, defer implementation |
| Machine concurrency | WSL: multiple allowed; Hyper-V: single active | WSL: multiple allowed | match `RequireExclusiveActive` per-provider behavior |
| Machine image format | WSL: Tar; Hyper-V: VHDX | WSL: Tar (derived from nimbus-machine-os) | match per-provider image format |
| Filesystem sharing | WSL: Plan9/9P auto-mount at `/mnt/c/`; Hyper-V: 9P via hvsock per-mount | WSL: reuse Plan9/9P auto-mounts; do not add a second sharing layer | match default provider |
| Host path translation | WSL guest sees Windows drives under `/mnt/<drive>/...` | explicit Windows-host-path → WSL-guest-path translation for compose build contexts, env files, working dirs, and bind mounts | Nimbus-specific seam required by the host-resident control plane |
| SSH port management | global file-locked port allocation (10000-65535), conflict reassignment | same global allocation pattern | match |
| Readiness model | 3-layer check: VM state → SSH port → SSH exec, exponential backoff | same layered readiness pattern | match |
| Machine config persistence | JSON + lockfile atomic writes, provider-specific nullable fields | same pattern as existing nimbus machine config | match |
| Elevation/permissions | Hyper-V: admin for first machine; WSL: no special permissions | WSL: no special permissions needed | match WSL simplicity |
| Docker compatibility | optional `\\.\pipe\docker_engine` claim | not targeted | intentionally narrower |
| Linux production model | standard containers | krun-backed per-service microVMs | intentionally different (same as macOS) |

Durable rules:

- copy Podman's Windows machine topology, WSL2 provider shape, and the
  win-sshproxy API-forwarding pattern where they are battle-tested and
  platform-driven
- mirror the `UseProviderNetworkSetup` split: WSL2 owns its own networking,
  Hyper-V (future) would use gvproxy. Do not force gvproxy onto the WSL2
  path where Podman does not use it
- reuse WSL's built-in `/mnt/<drive>` filesystem visibility instead of adding
  a second sharing mechanism, but add a narrow Windows-host-path →
  WSL-guest-path translation seam for Nimbus's host-resident compose/build
  flow
- mirror the WSL2 shell-script bootstrap model. Do not attempt to use
  ignition/FCOS for the WSL2 provider — that is not how Podman does it
- keep Nimbus's guest API, Linux production runtime, and user-facing service
  abstraction product-specific
- prefer WSL2 as the default and initially only provider; defer Hyper-V until
  WSL2 is proven and there is a concrete need for stronger isolation

## Target Architecture

### Why Windows-native (mirrors Podman's approach)

Nimbus already builds and ships `nimbus.exe` for `x86_64-pc-windows-msvc` in
every release. V8/deno_core compiles on Windows today. The Windows binary is
not a future aspiration — it is a shipped artifact. The remaining work is
machine lifecycle, transport, and developer workflow.

Given that the binary already exists, the Windows-native approach is the right
choice:

- **Mirrors Podman exactly.** Same `wsl --import`, same shell-script bootstrap,
  same win-sshproxy + named pipe pattern, same readiness layering.
- **Mirrors the macOS pattern.** Same hybrid control-plane architecture:
  host-resident server, guest-resident machine API, forwarded socket transport.
- **True Windows-native DX.** `nimbus.exe` in PowerShell/cmd.exe, no WSL2
  terminal required.
- **Machine lifecycle is explicit.** `nimbus machine init/start/stop/rm`
  gives the developer a clear mental model consistent across macOS and Windows.
- **One developer-facing workflow on all platforms.** Linux, macOS, and Windows
  developers all use the same `nimbus start` + `nimbus compose ...` commands.

### Accepted architecture

```text
Windows host
  └── nimbus.exe (Windows binary — already built and shipped)
        ├── nimbus machine init
        │     ├── downloads Tar rootfs from nimbus-machine-os
        │     ├── wsl --import nimbus-{name} {path} {tarball} --version 2
        │     ├── configures SSH, user, systemd namespace (shell commands)
        │     ├── installs nimbus guest binary + nimbus.socket/service units
        │     └── allocates SSH port from global pool
        ├── nimbus machine start
        │     ├── check SSH port availability (reassign if conflict)
        │     ├── wslInvoke(nimbus-{name}, /root/bootstrap)
        │     │     └── idempotent: exits if systemd already running
        │     │     └── unshare --kill-child ... /lib/systemd/systemd
        │     ├── PostStartNetworking: launches win-sshproxy
        │     │     ├── creates \\.\pipe\nimbus-machine-{name}
        │     │     ├── tunnels to guest /run/nimbus/nimbus.sock via SSH
        │     │     └── polls for pipe existence (80 retries × 250ms)
        │     └── conducts 3-layer readiness check (AFTER win-sshproxy)
        │           ├── WSL distro state == Running
        │           ├── SSH port accepting connections
        │           └── ssh user@localhost:port true
        ├── nimbus machine stop
        │     ├── graceful systemd shutdown:
        │     │     └── wsl -u root -d nimbus-{name} enterns systemctl exit 0
        │     │     └── 60s timeout waiting for systemd to exit
        │     ├── wsl --terminate nimbus-{name}
        │     └── stops win-sshproxy (read TID file, PostThreadMessageW + WM_QUIT)
        ├── nimbus machine rm
        │     └── wsl --unregister nimbus-{name}
        ├── nimbus start
        │     ├── authoritative API/runtime/storage on Windows host
        │     └── guest machine-API client over named pipe
        └── nimbus compose ...
              └── same guest machine-API client

WSL2 Linux guest (nimbus-{name} distro)
  ├── systemd in nested namespace (unshare --kill-child ...)
  ├── nimbus.socket / nimbus.service (narrow machine API)
  ├── SSH configured via shell commands (not ignition)
  ├── buildah + conmon + crun (standard containers)
  └── services run as standard containers
  Port forwarding: WSL2 native networking (same as Podman)
```

### Control-plane boundary

Nimbus on Windows follows the same hybrid control-plane architecture as macOS,
with named pipes replacing Unix sockets on the host side:

- the **host binary** (`nimbus.exe`) is the authoritative Nimbus
  API/runtime/storage loop on Windows
- the **guest binary/service** (`nimbus.socket`/`nimbus.service`) is a narrow
  machine API for service execution, not a second public Nimbus control plane
- the **guest** owns container lifecycle, observed container state, logs,
  readiness checks inside the guest, and published guest ports
- the **host** owns machine lifecycle (WSL2 distro management), image
  materialization/cache, Compose intent, local API/runtime/storage, and the
  developer-facing control surface

### Key platform differences from macOS

| Concern | macOS | Windows |
| --- | --- | --- |
| VM provider | krunkit (libkrun) | WSL2 (`wsl --import`) |
| Guest bootstrap | FCOS + ignition via vsock | Tar rootfs + shell commands (no ignition) |
| Ready signal | `virtio-vsock` ready device | none (WSL bootstrap is synchronous) |
| API forwarding | gvproxy + SSH (Unix socket on host) | win-sshproxy + SSH (named pipe on host) |
| Port forwarding | gvproxy (host localhost) | WSL2 native networking (host localhost) |
| File sharing | virtiofs mounts | Plan9/9P auto-mount (`/mnt/c/`, `/mnt/d/`) |
| Multiple machines | one at a time (Apple HV exclusive) | multiple allowed (WSL2 non-exclusive) |
| Systemd | native in FCOS guest | nested namespace in WSL2 guest |
| Machine image format | Raw (from OCI artifact) | Tar (for WSL import) |

### Rejected architecture

```text
Windows host
  └── WSL2 distribution
        └── nimbus (Linux binary running inside WSL2)
```

Rejected because:
- Nimbus already ships `nimbus.exe` — there is no compilation barrier
- Running inside WSL2 gives up Windows-native DX for no benefit
- Podman chose the Windows-native path for good reason; we should too
- The `nimbus machine` abstraction does not exist in this model
- Diverges from the macOS hybrid control-plane pattern

### Target command flows

#### `nimbus machine init` on Windows

```text
PowerShell / cmd.exe
  -> nimbus.exe machine init [--name default] [--cpus 4] [--memory 4096]
      -> download Tar rootfs from nimbus-machine-os (OCI or URL)
      -> wsl --import nimbus-{name} {dataDir}\wsldist\{name} {tarball} --version 2
         (all WSL commands use WSL_UTF8=1 env, CREATE_NO_WINDOW for background)
      -> wsl -u root -d nimbus-{name} rpm --restore shadow-utils (fix newuidmap)
      -> wsl -u root -d nimbus-{name} mkdir -p /usr/local/bin
      -> configureSystem (wsl -u root -d nimbus-{name} sh -c ...):
           1. append SSH port to /etc/ssh/sshd_config
           2. enable sshd.service + nimbus.socket symlinks
           3. disable getty, resolved, oom services
           4. add user to wheel group, sudoers NOPASSWD
           5. override systemd-sysusers (WSL kernel lacks sg/crypto_user)
           6. enable user linger
           7. install containers.conf (cgroup_manager = "cgroupfs")
           8. install nimbus.socket + nimbus.service units
      -> installScripts (wsl -u root -d nimbus-{name}):
           1. /usr/local/bin/enterns (chmod 755) — nsenter wrapper
           2. /etc/profile.d/enterns.sh — auto-enter namespace on login
           3. /root/bootstrap (chmod 755) — idempotent systemd launcher
      -> createKeys:
           1. read SSH public key
           2. install to /root/.ssh/authorized_keys
           3. install to /home/{user}/.ssh/authorized_keys
      -> allocate SSH port from global pool (file-locked)
      -> persist machine config (JSON + lockfile)
      -> wsl --terminate nimbus-{name} (recycle distro after config)
```

#### `nimbus machine start` on Windows

Mirrors Podman's exact startup sequence from `shim/host.go`:

```text
PowerShell / cmd.exe
  -> nimbus.exe machine start [--name default]
      -> acquire machine lock
      -> validate machine not already running
      -> setup signal handler (SIGINT/SIGTERM → mark Starting=false)
      -> startNetworking:
           check SSH port availability (reassign if conflict)
           provider.StartNetworking() → no-op for WSL default networking
      -> StartVM:
           wslInvoke(nimbus-{name}, "/root/bootstrap")
           └── bootstrap checks if systemd already running → exit 0
           └── if not: unshare --kill-child --fork --pid ... systemd
      -> WaitForReady:
           immediate no-op (WSL bootstrap is synchronous)
      -> PostStartNetworking:                ← win-sshproxy launches HERE
           get API socket path
           build WinProxyOpts (name, identity, SSH port, rootful, socket)
           LaunchWinProxy:
             check \\.\pipe\nimbus-machine-{name} available (5s timeout)
             find win-sshproxy.exe binary
             build args: name, stateDir, pipe path, ssh://root@localhost:{port}/run/nimbus/nimbus.sock, identity
             start win-sshproxy.exe process
             poll for pipe existence (80 retries × 250ms = 20s max)
      -> conductVMReadinessCheck:            ← AFTER win-sshproxy
           1. WSL distro state == Running
           2. SSH port accepting TCP connections
           3. ssh root@localhost:{port} true succeeds
           exponential backoff: 500ms → 1s → 2s → 4s → 8s → 16s (6 iterations)
      -> cleanup signal handler
```

#### `nimbus start` on Windows

```text
PowerShell / cmd.exe
  -> nimbus.exe start
      -> load machine config
      -> ensure machine is started (or auto-start)
      -> verify named pipe \\.\pipe\nimbus-machine-{name} is answering
      -> verify guest nimbus.sock health/capabilities behind the pipe
      -> build the remote guest machine-API client (over named pipe)
      -> start the authoritative host Nimbus API/runtime/storage loop
      -> expose the developer-facing API on localhost
      -> on ctx.services.*, call the guest machine API and wait for guest
         service readiness
```

## Cross-Platform Architecture Comparison

```text
┌─────────────────────────────────────────────────────────────────────┐
│                    Linux (Production)                               │
│                                                                     │
│  nimbus start (native Linux binary)                                 │
│    └── nimbus-sandbox krun backend                                  │
│          └── conmon → crun → libkrun (per-service microVMs)         │
│                                                                     │
│  No VM. No proxy. Direct kernel access.                             │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                    macOS (Developer)                                 │
│                                                                     │
│  macOS host                                                         │
│    ├── nimbus start (host-resident, authoritative)                  │
│    ├── nimbus machine ... (krunkit + gvproxy)                       │
│    │     └── gvproxy: API forwarding AND port forwarding            │
│    └── forwarded <machine>-api.sock (Unix socket via gvproxy/SSH)   │
│                                                                     │
│  Linux guest VM (krunkit, FCOS + ignition bootstrap)                │
│    ├── nimbus.socket / nimbus.service (narrow machine API)          │
│    └── buildah + conmon + crun (standard containers)                │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                    Windows (Developer)                               │
│                    Mirrors Podman WSL2 provider                      │
│                                                                     │
│  Windows host                                                       │
│    ├── nimbus.exe serve (host-resident, authoritative)              │
│    ├── nimbus.exe machine ...                                       │
│    │     ├── wsl --import nimbus-{name} (Tar rootfs, shell config)  │
│    │     ├── win-sshproxy: API forwarding via named pipe            │
│    │     │     └── \\.\pipe\nimbus-machine-{name} → SSH → guest     │
│    │     └── NO gvproxy (WSL2 owns networking)                      │
│    └── named pipe client for machine-API                            │
│                                                                     │
│  WSL2 Linux guest (nimbus-{name} distro)                            │
│    ├── systemd in nested namespace (unshare --kill-child ...)       │
│    ├── nimbus.socket / nimbus.service (narrow machine API)          │
│    ├── SSH configured via shell commands (not ignition)             │
│    └── buildah + conmon + crun (standard containers)                │
│    Port forwarding: WSL2 native networking (same as Podman)         │
└─────────────────────────────────────────────────────────────────────┘
```

## Feature Preservation Matrix

| Concern | Linux production baseline | macOS developer target | Windows developer target | Must preserve |
| --- | --- | --- | --- | --- |
| Service isolation | per-service krun microVMs | one machine VM + standard guest containers | one WSL2 distro + standard guest containers | same server/service API |
| Host runtime stack | `conmon → patched crun → libkrun` | `krunkit + gvproxy` on host, `buildah/conmon/crun` in guest | `win-sshproxy` on host, `buildah/conmon/crun` in WSL2 guest | Linux path stays unchanged |
| Host app/runtime locality | local Nimbus server owns runtime + storage | host Nimbus server still owns runtime + storage on macOS | host `nimbus.exe` server owns runtime + storage on Windows | fast local edit-run-observe loop |
| Remote control seam | n/a | host talks to guest machine API via forwarded Unix socket | host talks to guest machine API via named pipe (win-sshproxy) | do not grow a generic remote engine |
| Service networking | krun TSI host:guest ports | host localhost → gvproxy → guest container ports | WSL2 native networking → Windows localhost | `ctx.services.<name>.port` semantics |
| Readiness model | server waits for actual service reachability | same layered contract across host and guest | same layered readiness contract (3-layer + machine-API + service) | no "running means ready" regression |
| Compose/service UX | landed `nimbus start --compose-file ...` and `nimbus compose ...` | same commands from mac host | same commands from Windows PowerShell/cmd.exe | one developer-facing workflow |

## Transport Reality Matrix

| Surface | Linux production | macOS | Windows |
| --- | --- | --- | --- |
| Per-service data plane | krun TSI | not used | not used |
| Machine ready signal | n/a | `virtio-vsock` ready device (ignition) | none (WSL bootstrap is synchronous, same as Podman) |
| Guest networking | native Linux/KVM + TSI | `gvproxy` + `virtio-net` | WSL2 native networking (no gvproxy, same as Podman WSL2) |
| Guest API exposure | local server | forwarded Unix socket via gvproxy/SSH | named pipe via win-sshproxy/SSH (same as Podman WSL2) |
| Guest bootstrap | n/a | FCOS + ignition via vsock | Tar import + shell commands (same as Podman WSL2, NOT ignition) |
| File sharing | native Linux fs | `virtiofs` mounts | WSL2 Plan9/9P mounts (`/mnt/c/`, `/mnt/d/`) |
| Port publishing to host | native | gvproxy localhost forwarding | WSL2 native localhost forwarding (same as Podman WSL2) |

## Lifecycle and Probe Layers

The probe model mirrors Podman's 3-layer readiness check
(`conductVMReadinessCheck`), extended with Nimbus-specific machine-API
verification:

| Layer | What it answers | Podman parallel | Target status |
| --- | --- | --- | --- |
| W0: WSL2 distro state | is the WSL2 distribution running? | `provider.State()` returns `Running` | to implement |
| W1: SSH port listening | is the SSH port accepting TCP connections? | `isListening(mc.SSH.Port)` | to implement |
| W2: SSH command execution | can the host execute a command inside the guest via SSH? | `LocalhostSSHSilent(..., "true")` | to implement |
| W3: named pipe reachability | is `\\.\pipe\nimbus-machine-{name}` answering? | `WaitPipeExists` (80 retries × 250ms) | to implement |
| W4: guest machine-API health | does the forwarded guest `nimbus.sock` answer health/capabilities? | Nimbus-specific (narrower than Podman API) | to implement |
| W5: host Nimbus readiness | is host `nimbus.exe start` ready with its guest machine-API client wired? | Nimbus-specific | to implement |
| W6: guest service readiness | are published guest services reachable from Windows localhost? | Nimbus-specific | to implement |

Windows architectural rule:

- WSL2 distro readiness, SSH readiness, named pipe readiness, guest machine-API
  readiness, host Nimbus readiness, and service readiness are all separate
- a running WSL2 distro is not enough to declare SSH reachable
- a reachable SSH is not enough to declare the named pipe answering
- a reachable named pipe is not enough to declare the guest machine API healthy
- a healthy guest machine API is not enough to declare host `nimbus start` ready
- a ready host `nimbus start` is not enough to declare every declared guest
  service ready

## Podman Source Reference for Implementation

These are the exact Podman source files to use as implementation references,
with the specific patterns to mirror:

| Nimbus concern | Podman source file | Pattern to mirror |
| --- | --- | --- |
| WSL2 provider abstraction | `pkg/machine/wsl/stubber.go` | `WSLStubber` type, capability flags, `StartVM` return signature |
| WSL distro import | `pkg/machine/wsl/stubber.go` `CreateVM` | `wsl --import {name} {path} {tarball} --version 2` |
| WSL distro bootstrap | `pkg/machine/wsl/machine.go` | SSH/user/systemd setup via `wsl -u root -d` shell commands |
| Systemd namespace | `pkg/machine/wsl/declares.go` | `unshare --kill-child --fork --pid --mount --mount-proc` bootstrap |
| WSL command wrappers | `pkg/machine/wsl/wutil/wutil.go` | `CREATE_NO_WINDOW` flags, `WSL_UTF8=1` env |
| Named pipe creation | `pkg/machine/machine_windows.go` | `LaunchWinProxy`, `WaitPipeExists`, `DialNamedPipe`, TID lifecycle |
| Named pipe socket setup | `pkg/machine/shim/networking_windows.go` | `setupMachineSockets` returns pipe paths, optional docker_engine claim |
| Readiness check | `pkg/machine/shim/networking.go` | `conductVMReadinessCheck`: 3-layer with exponential backoff |
| SSH port allocation | `pkg/machine/ports/ports.go` | Global file-locked allocation, conflict reassignment on startup |
| Machine config persistence | `pkg/machine/vmconfigs/machine.go` | JSON + lockfile atomic writes, provider-specific nullable fields |
| Image pulling | `pkg/machine/shim/diskpull/diskpull.go` | `Disker` interface, OCI/URL/local paths, `Tar` format for WSL |
| gvproxy lifecycle | `pkg/machine/gvproxy.go`, `gvproxy_windows.go` | PID file, `winquit.QuitProcess` with 30s grace |
| User-mode networking | `pkg/machine/wsl/usermodenet.go` | Separate `podman-net-usermode` distro, file-based locking |
| Windows elevation | `pkg/machine/windows/util_windows.go` | `HasAdminRights`, `RelaunchElevatedWait`, `--reexec` flag |
| Helper binary discovery | `vendor/go.podman.io/common/pkg/config/config.go` | `FindHelperBinary` search order |

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Roadmap Status Ledger` and `Execution Log`
3. `docs/architecture/sandbox/macos-machine-flow.md` (architectural precedent)
4. `docs/architecture/sandbox/microvm-service-baseline.md`
5. `docs/plans/distribution-plan.md`
6. the reviewed Podman source files listed at the top of this document

General rules:

- Keep the Linux production runtime exactly as landed. This plan is for Windows
  developer support, not for re-architecting the Linux microVM path.
- Keep the macOS developer path exactly as designed. This plan should not
  regress or complicate the macOS machine architecture.
- Do not add nested per-service microVMs on Windows.
- Do not make Podman CLI or Podman Desktop a product dependency. Use them as
  architecture and diagnostics references only.
- Use WSL2 as the default and initially only Windows VM provider. Defer
  Hyper-V support until there is a concrete need.
- Follow the same forwarded machine-API pattern as macOS with named pipes
  replacing Unix sockets on the host side.
- Mirror Podman's WSL2-specific patterns exactly: shell-script bootstrap (not
  ignition), WSL2-native networking (not gvproxy), nested systemd namespace,
  Tar image format. Do not apply macOS/Hyper-V patterns to the WSL2 provider.
- When writing "named pipe" in code or docs, name the exact pipe path and
  purpose. Do not use "named pipe" as a fuzzy synonym for all Windows IPC.
- Do not target native Windows containers. Nimbus on Windows is a Linux
  container story, same as Podman.
- win-sshproxy is the API-forwarding component. gvproxy is the networking
  component. On WSL2, only win-sshproxy is needed (Podman does not use gvproxy
  for WSL2 default networking). Do not conflate the two.
- Every substantive work burst must update this plan's ledger and execution log
  in the same change set.

## Problem Statement

Some Nimbus engineers and users will develop on Windows and deploy to Linux.
We need a Windows developer experience that is reliable without creating a
third product architecture.

Target experience:

```text
Windows host (PowerShell / cmd.exe / Windows Terminal)
  -> nimbus.exe machine init    (wsl --import nimbus-default ...)
  -> nimbus.exe machine start   (starts WSL distro + win-sshproxy)
  -> nimbus.exe serve            (host-resident server + named pipe client)
  -> nimbus.exe service up/list/logs/down
  -> same compose.yaml as Linux and macOS
  -> host-local V8/runtime/storage/debug loop (on Windows host)
  -> remote guest service execution through forwarded machine API
  -> same ctx.services.<name>.port behavior
  -> published ports reachable from Windows browser at localhost
```

## Scope

This plan covers:

- the canonical Windows-native developer machine architecture
- `nimbus.exe machine ...` WSL2 provider: distro import, shell-script
  bootstrap, nested systemd, SSH, lifecycle management
- win-sshproxy integration for named-pipe API forwarding
- host machine-API client over named pipes
- transparent `nimbus.exe serve` and `nimbus.exe service ...` paths
- WSL2 networking characterization and port forwarding validation
- WSL2-format Tar image from nimbus-machine-os
- source-backed Podman reference mapping for every implementation seam

This plan does not cover:

- changing the Linux production microVM architecture
- changing the macOS developer machine architecture
- native Windows container support
- Hyper-V VM provider (deferred behind WSL2)
- Windows ARM64 support (deferred behind free GitHub runner availability)
- user-mode networking (deferred behind default WSL2 networking validation)

## Verification Contract

### Minimum verification for every code item

- `cargo fmt --all --check`
- focused `cargo check` for touched crates
- targeted tests for touched machine, transport, or platform-detection seams
- plan ledger and execution-log update in the same change set

### Required verification lanes

- **Windows host lane**
  - `nimbus.exe machine init` imports a WSL2 distro via `wsl --import`
  - `nimbus.exe machine start` starts the distro and launches win-sshproxy
  - named pipe `\\.\pipe\nimbus-machine-{name}` reachable and answering
  - 3-layer readiness check passes (WSL distro state, SSH port, SSH exec)
  - guest machine-API health/capabilities answering over the named pipe
  - `nimbus.exe serve` reaches readiness with guest machine-API client wired
  - published ports reachable from Windows localhost via WSL2 networking
  - `nimbus.exe machine stop/rm` clean shutdown and cleanup
  - clean recreate-from-stale-state

- **WSL2 guest lane inside the Windows machine**
  - the guest machine API boots predictably behind `nimbus.socket`
  - guest standard-container backend can drive buildah/conmon/crun
  - Compose-backed service flows work through the guest machine API
  - guest container networking and published ports match the host-facing claims

### Required evidence discipline

- If a verification artifact cannot live in git, record:
  - absolute path
  - exact command that produced it
  - exact command that proved it worked
- Prefer checked-in scripts/runbooks over ad hoc terminal history.

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| WIN1 | todo | Architecture lock: finalize this plan, update distribution-plan Windows row, add `ServiceHostPlatform::Windows` | macOS MAC5+ stabilization |
| WIN2 | todo | WSL2 machine provider: shared lifecycle prerequisites are now landed (`MLH3`-`MLH7`); remaining work is the Windows-specific `wsl --import` / shell-bootstrap / nested-systemd / named-transport implementation on top of that hardened seam | WIN1 |
| WIN3 | todo | win-sshproxy integration: named pipe creation, TID lifecycle, pipe-to-SSH tunnel, 3-layer readiness check, stale process cleanup | WIN2 |
| WIN4 | todo | Host machine-API client over named pipe: `ForwardedMachineApiSandboxBackend` with named pipe transport, `ServiceHostPlatform::Windows` backend loader | WIN3 |
| WIN5 | todo | WSL2 networking: characterize NAT vs mirrored mode, validate published ports reach Windows localhost, document user-mode networking fallback | WIN2 |
| WIN6 | todo | Transparent developer UX: Windows-aware `nimbus.exe serve` path, `nimbus.exe service ...` path, Windows-host-path → WSL-guest-path integration, end-to-end compose-backed flow validation | WIN3, WIN4, WIN5 |
| WIN7 | todo | Packaging and closeout: MSI/WinGet/Scoop packaging, distribution-plan alignment, install docs, final verification summary | WIN2, WIN3, WIN4, WIN5, WIN6 |

## Implementation Checkpoints

### WIN1 — Architecture lock and transport vocabulary

Repo outputs:

- this plan finalized
- distribution-plan Windows row updated from `Future` to `In Progress (dev)`
- `ServiceHostPlatform::Windows` variant added (replacing `Other` for Windows)
- plan-index / agent-entrypoint references to this control plane

Acceptance criteria:

- the Windows-native architecture is documented and justified
- the probe model is defined with explicit layer separation
- WSL2-specific divergences from the macOS path are documented
- a fresh agent can find this plan from `docs/plans/README.md`

### WIN2 — WSL2 machine provider

Repo outputs:

- WSL2-specific machine modules in `crates/nimbus-bin/src/machine/wsl/`
  behind `#[cfg(target_os = "windows")]`, replacing the current stubs
- `MachineProvider::Wsl2` variant added to the existing enum
- WSL command wrapper that sets `WSL_UTF8=1` env and `CREATE_NO_WINDOW`
  (0x08000000) creation flags on every invocation
- Tar rootfs image artifact from nimbus-machine-os (WSL-specific format,
  separate from the macOS Raw artifact)
- WSL distro lifecycle:
  - `wsl --import nimbus-{name} {path} {tarball} --version 2`
  - `wsl -d nimbus-{name}` (start)
  - `wsl --terminate nimbus-{name}` (stop)
  - `wsl --unregister nimbus-{name}` (remove)
- Shell-script bootstrap via `wsl -u root -d nimbus-{name}`, mirroring
  Podman's exact `configureSystem` + `installScripts` + `createKeys` sequence:
  - `rpm --restore shadow-utils` (fix newuidmap)
  - SSH port config and key installation (root + user authorized_keys)
  - user creation with wheel group, sudoers NOPASSWD
  - systemd service symlinks (sshd, nimbus.socket), disable getty/resolved/oom
  - systemd-sysusers override (WSL kernel lacks sg/crypto_user)
  - `containers.conf` with `cgroup_manager = "cgroupfs"`
  - `nimbus.socket` + `nimbus.service` unit installation
  - `/root/bootstrap` script (idempotent systemd launcher via `unshare`)
  - `/usr/local/bin/enterns` script (nsenter wrapper)
  - `/etc/profile.d/enterns.sh` (auto-enter namespace on login)
  - `wsl --terminate` after config (recycle distro)
- Global SSH port allocation with file-based locking
- Machine config persistence (JSON + lockfile, WSL-specific fields —
  `ignition_file_path` and `efi_variable_store_path` left `None`)
- Windows-host-path translation for guest-visible execution paths:
  - translate compose build contexts, Dockerfile paths, env-file paths,
    working directories, and bind-mount sources from Windows host form to
    WSL guest form (`C:\...` → `/mnt/c/...`)
  - normalize drive-letter casing and separators consistently
  - reject unsupported path forms with explicit errors until support is added
- Windows directory layout:
  - Config: `%APPDATA%\nimbus\machine\`
  - State: `%LOCALAPPDATA%\nimbus\machine\`
  - WSL distro data: `%LOCALAPPDATA%\nimbus\machine\wsldist\{name}\`
- `nimbus.exe machine init/start/stop/status/ssh/rm` wired for Windows
- Graceful stop sequence: `enterns systemctl exit 0` (60s timeout) → 
  `wsl --terminate` → stop win-sshproxy

Acceptance criteria:

- `nimbus.exe machine init` downloads a Tar rootfs and imports it as a WSL2
  distribution named `nimbus-{machineName}`
- shell-script bootstrap mirrors Podman's exact configuration sequence: SSH,
  user, systemd namespace, `nimbus.socket`/`nimbus.service`, containers.conf
  with cgroupfs (NOT ignition, NOT FCOS)
- `/root/bootstrap` is idempotent (exits immediately if systemd already running)
- `nimbus.exe machine start` follows Podman's exact startup sequence:
  SSH port check → bootstrap → PostStartNetworking → readiness check
- `nimbus.exe machine stop` follows Podman's exact stop sequence:
  `enterns systemctl exit 0` → `wsl --terminate` → stop win-sshproxy
- `nimbus.exe machine rm` runs `wsl --unregister nimbus-{name}`
- SSH port allocated from global pool, conflict detection and reassignment
  on startup
- Windows compose/build/bind paths are translated into guest-valid WSL paths
  before Nimbus asks the guest to use them
- unsupported path forms fail with clear validation errors instead of
  surfacing as opaque guest-side path failures
- `nimbus.exe machine ssh` connects to the guest via localhost SSH with
  host-key bypass (same pattern as macOS)
- `nimbus.exe machine status` reports WSL distro state and win-sshproxy
  named pipe reachability separately
- all WSL commands use `WSL_UTF8=1` and `CREATE_NO_WINDOW`

### WIN3 — win-sshproxy integration

Repo outputs:

- win-sshproxy binary sourced from `containers/gvisor-tap-vsock` releases
  (same binary Podman uses) or Nimbus-owned equivalent
- Named pipe creation: `\\.\pipe\nimbus-machine-{name}`
- TID file lifecycle: `{stateDir}/win-sshproxy.tid` with `{pid}:{tid}` format
- Pipe-to-SSH tunnel: named pipe → SSH → guest `/run/nimbus/nimbus.sock`
- Startup verification: pipe existence polling (80 retries × 250ms)
- Shutdown: `PostThreadMessageW` with `WM_QUIT`, fallback to
  `TerminateProcess`
- 3-layer readiness check: WSL distro state → SSH port → SSH exec
- Stale process detection and cleanup on startup

Acceptance criteria:

- win-sshproxy launches and creates the named pipe during `nimbus machine start`
- the named pipe tunnels API requests to the guest `nimbus.sock` via SSH
- the 3-layer readiness check mirrors Podman's `conductVMReadinessCheck`
- clean shutdown via TID-based `PostThreadMessageW` during `nimbus machine stop`
- stale win-sshproxy processes from crashed sessions are detected and cleaned up

### WIN4 — Host machine-API client over named pipe

Repo outputs:

- `MachineApiClient` transport abstraction: the current client uses
  `std::os::unix::net::UnixStream` for HTTP-over-Unix-socket. Add a parallel
  named pipe transport using `tokio::net::windows::named_pipe` (or
  `windows::Win32::Storage::FileSystem` for sync). The HTTP request/response
  protocol stays identical — only the underlying byte stream changes.
- `ForwardedMachineApiSandboxBackend` adapted to accept either a Unix socket
  path (macOS) or a named pipe path (Windows) as the transport target
- `ServiceHostPlatform::Windows` variant replacing `Other` in the enum
- `load_forwarded_machine_api_backend` extended with a `Windows` arm that
  builds a `MachineApiClient` targeting the named pipe
  `\\.\pipe\nimbus-machine-{name}` instead of a Unix socket
- host-aware service-manager loader that selects the named-pipe-forwarded guest
  backend for container-backed Compose projects on Windows

Acceptance criteria:

- the Windows host can reach the guest machine-API surface via named pipe
- the HTTP protocol over the named pipe is byte-identical to the Unix socket
  protocol (same request format, same response format, same content types)
- health/capabilities responses parse correctly over the named pipe transport
- service sandbox operations (image-start, build-start, inspect, stop) work
  through the named pipe
- the host service-manager loader correctly selects the forwarded backend on
  Windows and rejects it on Linux (same as current `Other` behavior)

### WIN5 — WSL2 networking characterization

Repo outputs:

- documentation characterizing WSL2 NAT vs mirrored networking behavior
- verification script for Windows-side port reachability
- documented user-mode networking fallback path (deferred implementation)

Acceptance criteria:

- published service ports are reachable from a Windows browser at `localhost`
- WSL2 networking mode trade-offs are documented:
  - NAT mode: ports bound to 0.0.0.0 inside WSL2 are forwarded to Windows
    localhost (default behavior, may vary by Windows build)
  - Mirrored mode: WSL2 shares the Windows network stack (Windows 11 22H2+)
- fallback path documented if neither mode works reliably (user-mode networking
  via gvproxy/gvforwarder, same as Podman's optional path)

### WIN6 — Transparent developer UX

Repo outputs:

- Windows-aware host-resident `nimbus.exe serve` path
- Windows-aware `nimbus.exe service ...` path
- Windows-host-path → WSL-guest-path translation integrated into the
  compose-backed service flow (build context, Dockerfile, env file,
  working_dir, bind mounts)
- end-to-end developer workflow documentation

Required host-local outputs:

- one clean end-to-end project root on a Windows machine
- `nimbus.exe serve` startup log showing machine-API client connection
- `nimbus.exe service up/list/logs/down` transcript

Acceptance criteria:

- from a Windows PowerShell/cmd.exe prompt, a developer can run the same
  compose-backed workflow they use on Linux without manually SSHing into the
  WSL2 guest
- the end-to-end flow proves WSL2 distro readiness, SSH readiness, named pipe
  readiness, guest machine-API readiness, host Nimbus readiness, and guest
  service readiness as separate steps
- compose-backed guest operations use translated guest-valid WSL paths rather
  than raw Windows host paths
- `ctx.services.<name>.port` behavior matches the Linux UX contract
- pure runtime/storage edits on Windows do not require moving the authoritative
  Nimbus server into the WSL2 guest

### WIN7 — Packaging and closeout

Repo outputs:

- MSI installer and/or WinGet/Scoop package for `nimbus.exe` + win-sshproxy
- distribution-plan alignment for the Windows row
- install documentation
- final verification summary

Required host-local outputs:

- install/init/start verification bundle on a clean Windows machine
- recovery-drill bundle (stale state recreate)
- packaging/install notes

Acceptance criteria:

- the Windows developer path is documented, testable, and repeatable
- this plan can be archived and the stable baseline updated
- the Windows row in the distribution plan is updated from `Future` to
  `Supported (dev)`

## Dependency Graph

- `WIN1` depends on macOS `MAC5`+ stabilization (reuses the same hybrid
  control-plane pattern).
- `WIN2` depends on `WIN1`.
- `WIN3` depends on `WIN2`.
- `WIN4` depends on `WIN3`.
- `WIN5` depends on `WIN2` (can proceed in parallel with `WIN3` and `WIN4`).
- `WIN6` depends on `WIN3`, `WIN4`, and `WIN5`.
- `WIN7` depends on `WIN2` through `WIN6`.

## Recommended Delivery Order

1. `WIN1` — architecture lock
2. `WIN2` — WSL2 machine provider
3. `WIN3` and `WIN5` — win-sshproxy integration + WSL2 networking (parallel)
4. `WIN4` — host machine-API client over named pipe
5. `WIN6` — transparent developer UX
6. `WIN7` — packaging and closeout

## Execution Log

- 2026-04-15: Created the Windows machine-support plan. Initial version
  included two options: WSL2-native (Option A, run Linux binary inside WSL2)
  and Windows-native (Option B, mirrors Podman's approach with `nimbus.exe`).
- 2026-04-15: Performed source-backed review against the actual Podman source at
  `/Users/jack/src/github.com/containers/podman/`. Key corrections: WSL2
  provider uses `UseProviderNetworkSetup() = true` (no gvproxy); WSL2 bootstrap
  is shell-script-based (not ignition); systemd runs in nested namespace; WSL2
  allows multiple concurrent machines; Tar image format; no ready signal unit.
- 2026-04-15: Removed Option A and committed to Windows-native (Option B).
  Rationale: the release workflow already builds and ships `nimbus.exe` for
  `x86_64-pc-windows-msvc` on every release — V8/deno_core compiles on Windows
  today. The "V8 on Windows is a non-trivial verification burden" concern that
  motivated Option A was based on a false premise. With the binary already
  shipping, there is no reason to diverge from Podman's proven Windows-native
  pattern. The Windows-native approach also mirrors the macOS hybrid
  control-plane architecture (host-resident server, guest-resident machine API)
  and gives developers a consistent `nimbus machine` + `nimbus start` workflow
  across all platforms. Renumbered roadmap items from WIN1-WIN7 with clear
  separation of WSL2 provider, win-sshproxy, named pipe client, networking,
  developer UX, and packaging.
- 2026-04-15: Deep review against Podman WSL2 source (stubber.go, machine.go,
  declares.go, usermodenet.go, wutil.go) and existing nimbus machine module.
  Key additions from the deep review:
  - Added exact Podman WSL2 startup sequence (10-step ordering from
    shim/host.go): lock → state check → signal handler → startNetworking →
    StartVM → WaitForReady → PostStartNetworking (win-sshproxy HERE) →
    conductVMReadinessCheck → volume mounts → signal cleanup.
  - Added exact Podman WSL2 stop sequence: enterns systemctl exit 0 (60s
    timeout) → wsl --terminate → stop win-sshproxy. Plan previously only
    mentioned wsl --terminate.
  - Added exact bootstrap script content with idempotency check (ps -ef |
    grep systemd && exit 0). Added enterns script and containers.conf
    (cgroup_manager = cgroupfs).
  - Added WSL command wrapping requirements: WSL_UTF8=1 env and
    CREATE_NO_WINDOW (0x08000000) creation flags on every invocation.
  - Added nimbus machine module integration details: cfg gating strategy
    (#[cfg(target_os = "windows")] alongside existing unix/stub split),
    MachineProvider::Wsl2 variant, MachineGuestConfig divergence (no ignition
    fields), MachineApiClient transport abstraction (HTTP protocol stays
    identical, only byte stream changes), Windows directory layout, and guest
    image artifact (WSL Tar format, separate from macOS Raw).
  - Expanded WIN2 checkpoint with Podman's exact configureSystem +
    installScripts + createKeys sequence including rpm --restore shadow-utils,
    systemd-sysusers override, service symlinks, and distro recycle.
  - Expanded WIN4 checkpoint with MachineApiClient transport abstraction
    details (same HTTP protocol, different byte stream).
- 2026-04-15: Tightened the plan after comparing it against the current
  Nimbus compose/machine code as well as Podman's Windows provider sources.
  Removed the stale "21-method" `VMProvider` wording, clarified that WSL
  auto-mounts remove the need for an extra sharing layer but do **not** remove
  the need for explicit Windows-host-path → WSL-guest-path translation, and
  made that translation a first-class requirement of the Windows compose-backed
  developer flow. This keeps the Windows plan Podman-aligned on machine
  topology and transport while remaining tailored to Nimbus's host-resident
  control plane.
- 2026-04-16: The shared machine-lifecycle hardening control plan completed
  `MLH3` through `MLH7`, which materially advances `WIN2` even though the
  Windows provider implementation itself is still pending. Nimbus now has the
  shared file-locked SSH port allocator, atomic/versioned machine records,
  explicit state rebuild policy, provider capability contract
  (`uses_provider_networking`, `requires_exclusive_active`, `image_format`,
  `bootstrap_mode`), and a phased startup orchestrator already landed in the
  existing machine module. Durable conclusion: `WIN2` can now focus on the
  Windows-specific WSL2 provider, named-transport, and bootstrap plumbing
  instead of re-solving shared lifecycle seams. Verification inherited from the
  shared hardening closeout: `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`; `cargo test -p nimbus-bin machine::`.
- 2026-04-16: Updated the Windows companion plan after the macOS machine-image
  decision was locked. Durable rule: the shared machine-os repo may ship
  different artifact families per provider, but WSL2 remains Tar plus shell
  bootstrap, while the macOS FCOS-first raw-image decision and any separate
  `fedora-bootc` experiments stay outside the Windows provider contract.
  Verification: docs-only plan update.
