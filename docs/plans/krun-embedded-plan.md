# Plan: krun-embedded — Embeddable libkrun for Neovex

Canonical design and execution plan for creating `agentstation/krun-embedded`,
a patched and packaged libkrun that can be embedded in Neovex as a Cargo
dependency, eliminating the need for system-installed libkrun/libkrunfw and
enabling a single-binary deployment.

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
   when the VM exits, killing the entire process. This prevents in-process
   embedding and forces a helper binary / child process pattern.

2. **System dependencies** — requires `libkrun.so` and `libkrunfw.so`
   installed on the system. libkrunfw bundles a custom Linux kernel with TSI
   patches. These are not available on most systems without manual
   installation.

**Goal:** Create a Cargo-consumable crate that:
- Patches out `_exit()` so `krun_start_enter()` returns cleanly
- Embeds the guest kernel (from libkrunfw) as `include_bytes!`
- Exposes a safe, idiomatic Rust API
- Allows neovex to be deployed as a single binary + `/dev/kvm`

---

## The Patch

The `_exit()` fix requires changes in two locations. The patch is intentionally
minimal to reduce rebase burden against upstream.

### Patch 1: `src/vmm/src/lib.rs` — Replace `_exit()` with event loop break

```rust
// BEFORE (upstream):
pub fn stop(&mut self, exit_code: i32) {
    info!("Vmm is stopping.");
    for observer in &self.exit_observers {
        observer
            .lock()
            .expect("Poisoned mutex for exit observer")
            .on_vmm_exit();
    }
    unsafe {
        libc::_exit(exit_code);
    }
}

// AFTER (patched):
pub fn stop(&mut self, exit_code: i32) {
    info!("Vmm is stopping.");
    for observer in &self.exit_observers {
        observer
            .lock()
            .expect("Poisoned mutex for exit observer")
            .on_vmm_exit();
    }
    // Store exit code for the event loop to read, instead of killing
    // the process. The event loop in krun_start_enter() checks this
    // and breaks out.
    self.exit_code.store(exit_code, Ordering::SeqCst);
}
```

### Patch 2: `src/libkrun/src/lib.rs` — Break the event loop and return

```rust
// BEFORE (upstream):
loop {
    match event_manager.run() {
        Ok(_) => {}
        Err(e) => {
            error!("Error in EventManager loop: {e:?}");
            return -libc::EINVAL;
        }
    }
}

// AFTER (patched):
loop {
    match event_manager.run() {
        Ok(_) => {
            // Check if stop() was called (exit_code changed from sentinel)
            let code = vmm_exit_code.load(Ordering::SeqCst);
            if code != i32::MAX {
                // Intentionally leak the VMM and all devices to avoid
                // running untested Drop impls. The maintainers confirmed
                // (issue #373) that clean shutdown paths are "untested or
                // not implemented at all." Leaking is safe: bounded memory
                // (~10-20MB per VM), reclaimed by OS at process exit.
                std::mem::forget(_vmm);
                return code;
            }
        }
        Err(e) => {
            error!("Error in EventManager loop: {e:?}");
            return -libc::EINVAL;
        }
    }
}
```

**Why `std::mem::forget()` instead of Drop:**

The upstream maintainers explicitly stated (issue #373, mtjhrc): "We probably
don't have proper Drop implementations in places, don't handle a lot of
objects lifetimes (e.g. using RawFd) and we don't have mechanisms for stopping
worker threads cleanly."

`mem::forget()` is the correct tool here: it prevents the VMM, device objects,
and vCPU handles from running their (broken) destructors. The leaked memory is:
- **Bounded:** ~10-20MB per VM lifecycle (VMM struct + device state)
- **Reclaimable:** OS reclaims everything at process exit
- **Acceptable for the use case:** VMs run for minutes/hours; a few MB of
  leaked state per VM lifecycle is negligible

If upstream fixes Drop impls in v2, this can be changed to proper cleanup.

### Signal handler considerations

The signal handler in `src/vmm/src/signal_handler.rs` also calls `_exit()` for
SIGBUS, SIGSEGV, SIGSYS. These are crash handlers and should remain as-is —
a segfault in the VMM should still terminate the process. Only the normal
shutdown path (guest exits cleanly) is patched.

---

## Kernel Embedding

libkrunfw compiles a Linux kernel and packages it as a C shared library via
`bin2cbundle.py`. The kernel binary (~15-20MB), qboot BIOS (~64KB), and
initrd (~1MB) are embedded as byte arrays.

### Strategy: Download pre-built, embed via `include_bytes!`

```rust
// krun-kernel/src/lib.rs
/// Pre-built vmlinux from libkrunfw releases (with TSI patches)
pub static KERNEL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vmlinux"));

/// QBOOT BIOS for x86_64 boot
pub static QBOOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/qboot.rom"));

/// Minimal initrd with libkrun's init
pub static INITRD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/initrd"));
```

```rust
// krun-kernel/build.rs
fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let version = "5.3.0"; // match libkrunfw version

    // Download pre-built kernel from libkrunfw GitHub releases
    // These are the .so files — we extract the embedded kernel bytes
    // OR: download the raw kernel/qboot/initrd artifacts if published
    download_kernel_artifacts(&out_dir, version);
}
```

**Alternative:** Build libkrunfw from source in `build.rs`. This requires a
full kernel build toolchain and takes ~10 minutes. Only viable for CI, not
developer builds. Pre-built artifacts are strongly preferred.

**Binary size impact:** +15-20MB for the kernel. Neovex already embeds V8
(~30-50MB), so the total binary would be ~80-100MB. This is acceptable for
a single-binary server deployment.

---

## Repo Structure

```
agentstation/krun-embedded/
  Cargo.toml                  # workspace
  README.md
  LICENSE                     # Apache 2.0 (same as libkrun)

  crates/
    krun-core/                # patched libkrun (git submodule + patches)
      libkrun/                # git submodule → containers/libkrun
      patches/
        0001-replace-exit-with-return.patch
      Cargo.toml              # re-exports libkrun's src/libkrun with patches
      build.rs                # applies patches to submodule at build time

    krun-kernel/              # kernel bytes embedded
      build.rs                # downloads pre-built kernel from libkrunfw releases
      src/lib.rs              # exposes KERNEL, QBOOT, INITRD as &[u8]

    krun/                     # safe Rust API (what neovex depends on)
      Cargo.toml
      src/
        lib.rs                # MicroVm, VmConfig, VmHandle
        builder.rs            # MicroVm::builder() fluent API
        handle.rs             # VmHandle: wait(), shutdown(), vsock_connect()
```

### The safe API (`krun` crate)

```rust
use krun::{MicroVm, VsockPort};

// Configure a VM
let vm = MicroVm::builder()
    .vcpus(1)
    .ram_mib(256)
    .root("/path/to/unpacked/oci/rootfs")
    .workdir("/app")
    .env("DATABASE_URL", "postgres://localhost:5432/mydb")
    .env("NODE_ENV", "production")
    .vsock_port(VsockPort::stream(10000))  // for neovex ↔ guest comms
    .build()?;

// Start the VM on a dedicated OS thread
let handle = vm.start()?;

// Connect to a vsock port on the guest
let stream = handle.vsock_connect(10000).await?;

// Wait for the VM to exit (blocks)
let exit_code = handle.wait().await?;

// OR: trigger graceful shutdown
handle.shutdown()?;
```

**Thread model:**

```
neovex tokio runtime
  │
  ├── VmHandle::start() spawns std::thread (NOT tokio)
  │     └── krun_start_enter() blocks on this thread
  │           └── VMM event loop runs here
  │           └── vCPU threads spawned by KVM
  │
  ├── VmHandle::wait() uses tokio::sync::oneshot
  │     └── Notified when the event loop breaks and returns
  │
  ├── VmHandle::vsock_connect() uses tokio::net::UnixStream
  │     └── Connects to libkrun's vsock UDS proxy
```

`krun_start_enter()` must run on a dedicated `std::thread` because:
- It blocks indefinitely (event loop)
- KVM_RUN ioctl blocks the thread
- It must NOT run on a tokio worker thread

The `VmHandle` bridges this to tokio's async world via channels and `AsyncFd`.

---

## Upstream Strategy

### Contribute the patch

The `_exit()` → `mem::forget()` + return patch should be submitted upstream
to `containers/libkrun` as a PR. Arguments:

1. **Minimal diff** — two functions changed, ~10 lines
2. **No behavior change for existing consumers** — crun, muvm, krunkit all
   call `krun_start_enter()` in a sacrificial process where the return value
   is ignored (process was going to exit anyway)
3. **Enables library embedding** — the stated goal of PR #494 (Rust API)
4. **`mem::forget()` is honest** — it acknowledges the broken Drop paths
   instead of pretending cleanup works
5. **Aligns with v2 direction** — maintainers already want this; this gives
   them an incremental step

**If accepted upstream:** `krun-embedded` becomes a thin wrapper that embeds
the kernel and provides the safe API. No patch maintenance needed.

**If rejected:** Maintain the patch as a rebase-able series. The patch is
small enough (~10 lines) that rebasing on new libkrun releases is trivial.

### Track upstream version

Pin to specific libkrun tags (e.g., v1.17.4). When upstream releases a new
version:
1. Update the git submodule
2. Rebase the patch (if still needed)
3. Test
4. Tag a new krun-embedded release

---

## Phase Plan

### Phase K1: Repository Setup and Patch

**Goal:** Create the `agentstation/krun-embedded` repo with the patched
libkrun that returns from `krun_start_enter()` instead of calling `_exit()`.

**Scope:**
- Create repo with workspace structure
- Add `containers/libkrun` as git submodule pinned to v1.17.4
- Create and apply the two-location patch
- Verify: `krun_start_enter()` returns exit code when VM shuts down
- Verify: calling process survives after VM exits

**Acceptance criteria:**
- A test program calls `krun_start_enter()`, the VM runs `echo hello && exit 0`,
  and the test program continues executing after the VM exits
- The test program can read the exit code (0)
- No segfaults, no deadlocks on the happy path

### Phase K2: Kernel Embedding

**Goal:** Embed the guest kernel so no system-installed libkrunfw is needed.

**Scope:**
- `krun-kernel` crate with `build.rs` that downloads pre-built kernel from
  libkrunfw GitHub releases
- `include_bytes!` for kernel, qboot, initrd
- Modify `krun-core` to use embedded kernel instead of `dlopen("libkrunfw.so")`
- Verify: VM boots without any system-installed libkrunfw

**Acceptance criteria:**
- Boot Alpine rootfs using only the embedded kernel
- No libkrunfw.so on the system
- Binary size is documented

### Phase K3: Safe Rust API

**Goal:** Expose a clean, safe Rust API that neovex can depend on.

**Scope:**
- `krun` crate with `MicroVm::builder()` fluent API
- `VmHandle` with `start()`, `wait()`, `shutdown()`, `vsock_connect()`
- Thread management (dedicated `std::thread` for the event loop)
- Tokio bridge (`VmHandle` is `Send + Sync`, works from async context)
- Error types (KVM not available, kernel load failed, etc.)

**Acceptance criteria:**
- Can boot a VM, connect via vsock, exchange messages, shut down cleanly
- Works from within a tokio runtime
- API is `Send + Sync` — VmHandle can be stored in an Arc and shared
- `cargo doc` produces clean documentation

### Phase K4: Upstream PR

**Goal:** Submit the `_exit()` patch to `containers/libkrun`.

**Scope:**
- Clean up the patch for upstream contribution
- Write PR description explaining the rationale
- Respond to review feedback
- If accepted: remove patch from krun-embedded, depend on upstream directly

**Acceptance criteria:**
- PR submitted with tests
- Maintainer feedback addressed

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| K1: Repo + Patch | `todo` | libkrun source, KVM access | |
| K2: Kernel Embedding | `todo` | K1 | libkrunfw release artifacts |
| K3: Safe Rust API | `todo` | K1 | |
| K4: Upstream PR | `todo` | K1 tested and stable | |

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `mem::forget()` causes resource leaks | Certain | Low — bounded, ~10-20MB per VM | Acceptable for long-running VMs; OS reclaims at process exit |
| Patch doesn't apply to future libkrun versions | Low | Medium | Patch is ~10 lines; rebase is trivial |
| Upstream rejects the patch | Medium | Low | Maintain as fork; patch is tiny |
| Embedded kernel becomes stale | Medium | Low | Track libkrunfw releases; update periodically |
| vCPU worker threads don't stop after `mem::forget()` | Medium | Medium | Test thoroughly; may need to signal vCPU threads to exit before breaking event loop |
| `mem::forget()` leaks file descriptors (KVM, eventfd) | Likely | Low | FDs are reclaimed at process exit; for in-process use, may accumulate — test with repeated VM creation |

### FD leak concern for in-process embedding

If neovex creates and destroys many VMs in-process (without process exit),
`mem::forget()` will leak KVM fds, eventfds, and other kernel resources.
Linux has a default per-process fd limit of 1024 (soft) / 1048576 (hard).

**Mitigations:**
- Phase K3 should track fd count before/after VM lifecycle and log warnings
- For high-volume VM creation, the helper binary pattern (process per VM) is
  still available as a fallback — the OS reclaims all fds at process exit
- Long term: contribute proper Drop impls upstream (post K4)

---

## Execution Log

_Empty — no work started._
