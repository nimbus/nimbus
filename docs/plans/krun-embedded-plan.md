# Plan: krun-embedded — Embeddable libkrun for Neovex

Canonical design and execution plan for creating `agentstation/krun-embedded`,
a packaged libkrun that neovex can consume as a Cargo dependency, eliminating
the need for system-installed libkrun/libkrunfw and enabling a single-binary
deployment.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when microVM runtime work begins; this plan
  should execute before or in parallel with `microvm-runtime-plan.md` Phase M2
- **Relationship to microvm-runtime-plan:** This plan produces the VMM
  dependency that Phase M2 consumes. If this plan is deferred, Phase M2 falls
  back to system-installed libkrun + helper binary pattern.

## Control Plan Rules

This document is the durable control plane for the krun-embedded workstream.
The source of truth is:

1. the current git worktree (of `agentstation/krun-embedded`, a separate repo)
2. this plan's `Phase Status Ledger` and `Execution Log`
3. `docs/research/libkrun-evaluation.md` for the libkrun deep evaluation
4. `containers/libkrun` upstream at `~/src/github.com/containers/libkrun`

### Status model

- `todo`: not started
- `in_progress`: actively being implemented
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked

---

## Problem Statement

libkrun is the best VMM for neovex's use case (long-running Docker-image-based
service VMs), but it has two deployment problems:

1. **`_exit()` on VM shutdown** — `krun_start_enter()` calls `libc::_exit()`
   when the VM exits, killing the entire process.

2. **System dependencies** — requires `libkrun.so` and `libkrunfw.so`
   installed on the system. libkrunfw bundles a custom Linux kernel with TSI
   patches. These are not available on most systems without manual
   installation.

**Goal:** Create a Cargo-consumable crate that:
- Embeds the guest kernel (from libkrunfw) as `include_bytes!`
- Statically links libkrun (no system `.so` needed)
- Exposes a safe, idiomatic Rust API
- Uses the re-exec self pattern to handle `_exit()` with zero resource leaks
- Allows neovex to be deployed as a single binary + `/dev/kvm`

---

## Architecture: Re-exec Self Pattern

### Why not patch `_exit()`?

An earlier version of this plan proposed patching `_exit()` out of libkrun and
using `std::mem::forget()` to avoid broken Drop impls. **This was rejected
because it leaks ~10-20MB of memory and file descriptors per VM lifecycle.**
For a server that creates/destroys VMs over time, this leads to resource
exhaustion.

### The re-exec self pattern

Instead of trying to make `krun_start_enter()` return cleanly, we accept that
it kills the process — and make that process a **child** of neovex. The child
is a re-execution of the same binary in VMM mode:

```
neovex binary (single binary, contains everything)
  │
  ├── Server mode (default):  neovex serve
  │     tokio + V8 + engine + VM manager
  │     spawns child processes for each VM
  │
  └── VMM mode (internal):    neovex --internal-vmm
        reads config from stdin
        calls krun_start_enter()
        _exit() kills only this child process
        OS reclaims ALL resources — zero leaks
```

```rust
// When neovex needs to start a VM:
let mut child = Command::new(std::env::current_exe()?)
    .arg("--internal-vmm")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;

// Send VM config
child.stdin.take().unwrap().write_all(&config_json).await?;

// Monitor the child — full observability (see Observability section)
```

**Why this is correct:**
- **Zero leaks:** `_exit()` kills the child process, OS reclaims all memory,
  FDs, KVM state, threads. Nothing accumulates.
- **No patch needed:** Uses upstream libkrun as-is. No fork maintenance.
- **Single binary:** The VMM code (libkrun + kernel) is statically linked
  into the neovex binary. `current_exe()` re-executes the same file.
- **Process isolation:** A VMM bug or guest escape only compromises the child,
  not the neovex server. This is the same security model as Firecracker.

---

## Observability Model

The re-exec pattern provides rich observability through layered signals, from
cheapest (always available) to deepest (requires guest cooperation).

### Layer 1: Process Liveness (free)

```rust
// Always available — OS-level signal
match child.try_wait()? {
    None => { /* VM process alive = VMM event loop running */ }
    Some(status) => {
        if status.success() {
            // Workload exited cleanly (exit code 0)
        } else if let Some(code) = status.code() {
            // Workload exited with error code
        } else {
            // Killed by signal (crash, OOM, SIGKILL)
            let signal = status.signal().unwrap();
        }
    }
}
```

| Signal | Meaning |
|--------|---------|
| `try_wait()` returns `None` | VM is running |
| Exit code 0 | Workload exited successfully |
| Exit code N | Workload exited with error |
| Signal SIGSEGV/SIGABRT | VMM crashed |
| Signal SIGKILL | OOM killer or force kill |

### Layer 2: Boot Readiness (one line in child)

The child writes to stdout after configuring libkrun but before entering the
event loop:

```rust
// In VMM mode (child process):
fn vmm_main(config: VmConfig) {
    let ctx = krun_create_ctx();
    // ... configure VM ...
    
    // Signal parent that VMM is configured and about to start
    println!("READY");
    
    krun_start_enter(ctx);
}
```

Parent reads this:
```rust
let mut line = String::new();
let stdout = child.stdout.as_mut().unwrap();
stdout.read_line(&mut line).await?;
assert_eq!(line.trim(), "READY");
// VM is now booting
```

### Layer 3: Service Readiness via TSI (no guest-side code)

TSI maps guest ports to the host. The parent TCP-connects to check if the
service inside the VM is accepting connections:

```rust
// Poll until service is ready
let start = Instant::now();
loop {
    match TcpStream::connect(("127.0.0.1", tsi_mapped_port)).await {
        Ok(_) => break, // Service is accepting connections
        Err(_) if start.elapsed() < timeout => {
            sleep(Duration::from_millis(250)).await;
        }
        Err(e) => return Err(anyhow!("Service failed to start: {e}")),
    }
}
```

This works for any TCP service (postgres, redis, HTTP APIs) without any
guest-side health check code.

### Layer 4: Hang Detection (timeout on health check)

If the service stops responding to TCP connects (Layer 3) for a configurable
duration, the VM is considered hung:

```rust
let health_check = TcpStream::connect(addr);
match timeout(Duration::from_secs(5), health_check).await {
    Ok(Ok(_)) => VmHealth::Healthy,
    Ok(Err(_)) => VmHealth::ServiceDown,
    Err(_) => VmHealth::Hung, // timeout — VM may be deadlocked
}
```

Remediation options: log warning, restart VM, notify user.

### Layer 5: Guest-Level Health via vsock (deepest, optional)

For advanced introspection beyond "is the port open," a lightweight vsock
agent inside the VM can report:

- Process status (is PID 1 healthy? is the workload running?)
- Resource usage (CPU, memory, disk from `/proc`)
- Application-level health (custom health check endpoint)
- Graceful shutdown (clean stop instead of SIGKILL)

This requires adding a small sidecar to the guest rootfs. **Not needed for
the initial implementation** — Layers 1-4 provide sufficient observability
for most service VMs. Add when neovex needs `ctx.services.db.status()`.

### Summary

| Layer | What it tells you | Cost | When to add |
|-------|-------------------|------|-------------|
| 1. Process liveness | Running / exited / crashed | Free | Always |
| 2. Boot readiness | VMM configured, guest booting | 1 line | Phase K3 |
| 3. Service readiness | TCP service accepting connections | TSI (free) | Phase K3 |
| 4. Hang detection | Service stopped responding | Timeout wrapper | Phase K3 |
| 5. Guest health | Deep introspection (CPU, memory, app health) | vsock agent | Future (M3+) |

---

## Repo Structure

```
agentstation/krun-embedded/
  Cargo.toml                  # workspace
  README.md
  LICENSE                     # Apache 2.0 (same as libkrun)

  crates/
    krun-core/                # libkrun as a Rust crate (upstream, no patches)
      libkrun/                # git submodule → containers/libkrun
      Cargo.toml              # re-exports libkrun's crate with features

    krun-kernel/              # kernel bytes embedded
      build.rs                # downloads pre-built kernel from libkrunfw releases
      src/lib.rs              # exposes KERNEL, QBOOT, INITRD as &[u8]

    krun/                     # safe Rust API (what neovex depends on)
      Cargo.toml
      src/
        lib.rs                # MicroVm, VmConfig, VmHandle
        builder.rs            # MicroVm::builder() fluent API
        handle.rs             # VmHandle: wait(), shutdown(), health()
        vmm_mode.rs           # --internal-vmm entry point
        observability.rs      # Health check layers
```

### The safe API (`krun` crate)

```rust
use krun::{MicroVm, VmHandle, VmHealth, VsockPort};

// Configure a VM
let vm = MicroVm::builder()
    .vcpus(1)
    .ram_mib(256)
    .root("/path/to/unpacked/oci/rootfs")
    .workdir("/app")
    .env("DATABASE_URL", "postgres://localhost:5432/mydb")
    .vsock_port(VsockPort::stream(10000))
    .tsi_port_map(5432, 15432)  // guest 5432 → host 15432
    .build()?;

// Start the VM (re-execs current binary in VMM mode)
let handle = vm.start().await?;
// handle.start() internally does:
//   1. Command::new(current_exe()).arg("--internal-vmm").spawn()
//   2. Sends config via stdin
//   3. Waits for "READY" on stdout
//   4. Returns VmHandle

// Check service readiness
handle.wait_for_service(15432, Duration::from_secs(30)).await?;

// Health check
match handle.health(15432).await {
    VmHealth::Healthy => { /* service responding */ }
    VmHealth::ServiceDown => { /* port not accepting connections */ }
    VmHealth::Hung => { /* timeout on health check */ }
    VmHealth::Exited(code) => { /* VM process exited */ }
}

// Graceful stop
handle.kill().await?;  // SIGTERM → wait → SIGKILL

// Or wait for natural exit
let exit_code = handle.wait().await?;
```

### VMM mode entry point

```rust
// krun/src/vmm_mode.rs
// This runs in the re-exec'd child process

pub fn vmm_main() -> ! {
    // Prevent orphan processes if parent dies
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL); }

    // Read config from stdin
    let config: VmConfig = serde_json::from_reader(std::io::stdin()).unwrap();

    let ctx = unsafe { krun_create_ctx() };
    unsafe {
        krun_set_vm_config(ctx, config.vcpus, config.ram_mib);
        krun_set_root(ctx, &config.root);
        krun_set_workdir(ctx, &config.workdir);

        for (k, v) in &config.env {
            krun_set_env(ctx, &format!("{k}={v}"));
        }

        for port in &config.vsock_ports {
            krun_add_vsock_port(ctx, port.guest_port, port.socket_type);
        }

        for (guest, host) in &config.tsi_port_map {
            krun_set_port_map(ctx, *guest, *host);
        }
    }

    // Signal parent: VMM configured, about to boot
    println!("READY");

    // Enter the VM — this never returns. _exit() is called on VM shutdown.
    // OS reclaims all resources. Zero leaks.
    unsafe { krun_start_enter(ctx) };

    unreachable!()
}
```

### How neovex integrates

```rust
// In neovex's main.rs:
fn main() {
    // Check for internal VMM mode before doing anything else
    if std::env::args().any(|a| a == "--internal-vmm") {
        krun::vmm_mode::vmm_main();
        // unreachable — vmm_main calls _exit()
    }

    // Normal neovex server startup
    // ...
}
```

This adds ~10 lines to neovex's main.rs. The entire VMM mode is handled
by the `krun` crate.

---

## Kernel Embedding

libkrunfw compiles a Linux kernel (with TSI patches) and packages it as a
shared library. The kernel binary (~15-20MB), qboot BIOS (~64KB), and initrd
(~1MB) are embedded as byte arrays.

### Strategy: Download pre-built artifacts in build.rs

```rust
// krun-kernel/build.rs
fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let version = "5.3.0";
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    // Download from libkrunfw GitHub releases
    let base_url = format!(
        "https://github.com/containers/libkrunfw/releases/download/v{version}"
    );

    download(&format!("{base_url}/vmlinux-{arch}"), &format!("{out_dir}/vmlinux"));
    download(&format!("{base_url}/qboot-{arch}.rom"), &format!("{out_dir}/qboot.rom"));
    download(&format!("{base_url}/initrd-{arch}"), &format!("{out_dir}/initrd"));
}
```

**Binary size impact:** +15-20MB for the kernel. Neovex already embeds V8
(~30-50MB), so the total binary would be ~80-100MB. Acceptable for a
single-binary server deployment.

**Alternative:** Download kernel on first run instead of embedding. Keeps the
binary smaller (~60-70MB) but requires network access on first start.
Configurable via cargo feature flag.

---

## Phase Plan

### Phase K1: Repository Setup

**Goal:** Create the `agentstation/krun-embedded` repo with libkrun as a
buildable Cargo dependency.

**Scope:**
- Create repo with workspace structure
- Add `containers/libkrun` as git submodule pinned to v1.17.4
- `krun-core` crate that re-exports libkrun's `src/libkrun` crate
- Verify: can build and link libkrun statically from the workspace

**Acceptance criteria:**
- `cargo build` succeeds in the workspace
- A test binary can call `krun_create_ctx()` and `krun_free_ctx()`

### Phase K2: Kernel Embedding

**Goal:** Embed the guest kernel so no system-installed libkrunfw is needed.

**Scope:**
- `krun-kernel` crate with `build.rs` that downloads pre-built kernel
  artifacts from libkrunfw GitHub releases
- `include_bytes!` for kernel, qboot, initrd
- Modify `krun-core` to use embedded kernel instead of `dlopen("libkrunfw.so")`

**Acceptance criteria:**
- Boot Alpine rootfs using only the embedded kernel
- No libkrunfw.so on the system
- Binary size documented

### Phase K3: Safe Rust API + Observability

**Goal:** Expose a clean, safe Rust API with the re-exec self pattern and
layered observability.

**Scope:**
- `krun` crate with `MicroVm::builder()` fluent API
- `VmHandle` with `start()`, `wait()`, `kill()`, `health()`,
  `wait_for_service()`
- Re-exec self pattern: `vmm_mode::vmm_main()` entry point
- Observability layers 1-4:
  - Process liveness via `try_wait()`
  - Boot readiness via stdout `READY` signal
  - Service readiness via TCP connect (TSI)
  - Hang detection via timeout on health check
- `VmHealth` enum: `Healthy`, `ServiceDown`, `Hung`, `Exited(i32)`
- `PR_SET_PDEATHSIG(SIGKILL)` in child for cleanup

**Acceptance criteria:**
- Can boot a VM, wait for service readiness, health check, shut down cleanly
- VmHandle is `Send + Sync` — works from async context
- No resource leaks after VM shutdown (verified with `/proc/self/fd` count)
- `cargo doc` produces clean documentation
- Integration test: boot Alpine, run `nc -l -p 8080`, connect from host via
  TSI, shut down, verify process cleaned up

### Phase K4: Upstream Engagement

**Goal:** Engage with libkrun maintainers on the embedding use case.

**Scope:**
- Open an issue on `containers/libkrun` describing the krun-embedded approach
- Ask about official support for static linking + kernel embedding
- Propose the re-exec pattern as a documented use case
- If maintainers are receptive, explore contributing the safe API upstream

**Acceptance criteria:**
- Issue opened, feedback received
- Any upstream changes that simplify krun-embedded are tracked

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| K1: Repo Setup | `todo` | libkrun source, KVM access | |
| K2: Kernel Embedding | `todo` | K1, libkrunfw release artifacts | |
| K3: Safe API + Observability | `todo` | K1 | Can parallelize with K2 |
| K4: Upstream Engagement | `todo` | K1 tested and stable | |

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| libkrunfw doesn't publish raw kernel artifacts in releases | Medium | Medium | Build from source in CI, cache the artifacts |
| libkrun internal crate APIs change between versions | Medium | Low | Pin to submodule tag, update deliberately |
| Static linking issues (symbol conflicts, missing deps) | Medium | Medium | Test early in K1; may need build.rs tweaks |
| Re-exec pattern doesn't work on all platforms | Low | Medium | Linux-only initially; `current_exe()` is reliable on Linux |
| TSI port mapping has conflicts with multiple VMs | Medium | Low | Assign unique host ports per VM; document in API |
| Binary size too large with embedded kernel | Low | Low | Optional feature flag for download-on-first-use |

---

## What This Plan Does NOT Do

- **Does not patch libkrun.** No `_exit()` modification, no `mem::forget()`,
  no fork of libkrun's behavior. Uses upstream as-is.
- **Does not implement guest-side health agents.** Layer 5 observability
  (vsock agent) is deferred to `microvm-runtime-plan.md` Phase M3+.
- **Does not handle OCI image management.** That belongs in
  `microvm-runtime-plan.md` Phase M1.

---

## Execution Log

_Empty — no work started._
