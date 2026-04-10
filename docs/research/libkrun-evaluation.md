# libkrun Evaluation

Deep evaluation of `containers/libkrun` as a VMM-as-library alternative to
Firecracker for embedding microVM execution in Neovex.

**Date:** 2026-04-09
**Updated:** 2026-04-09 (verified against source code at
`~/src/github.com/containers/libkrun`, GitHub issues #373/#561, PR #494)
**Status:** Research complete

---

## Overview

libkrun is a library VMM derived from Firecracker, rust-vmm, and Cloud
Hypervisor code. It is maintained by Red Hat (Sergio Lopez / `slp`) in the
`containers` GitHub organization (same org as podman, buildah, crun). Licensed
Apache 2.0.

- **Latest release:** v1.17.4 (2026-02-18)
- **Last commit:** 2026-04-07 (actively maintained)
- **Stars:** 1,808
- **API stability:** Stable since v1.0.0 (SemVer guaranteed)

---

## API Surface

The C API (`include/libkrun.h`) exposes ~55 functions. Key groups:

### Lifecycle
```c
uint32_t krun_create_ctx();
int32_t  krun_free_ctx(uint32_t ctx_id);
int32_t  krun_start_enter(uint32_t ctx_id);  // BLOCKS FOREVER
int32_t  krun_get_shutdown_eventfd(uint32_t ctx_id);
```

### VM Configuration
```c
krun_set_vm_config(ctx_id, num_vcpus, ram_mib)
krun_set_root(ctx_id, root_path)        // virtiofs root
krun_set_root_disk(ctx_id, disk_path)   // virtio-block root
krun_set_exec(ctx_id, exec_path, argv, envp)
krun_set_env(), krun_set_workdir()
krun_set_kernel(), krun_set_firmware()
```

### Devices
```c
krun_add_disk(), krun_add_disk2()            // block devices
krun_add_virtiofs(), krun_add_virtiofs2()    // filesystem sharing
krun_add_vsock(), krun_add_vsock_port()      // vsock
krun_add_net_tap(), krun_set_passt_fd()      // networking
krun_set_port_map()                          // TSI port forwarding
krun_set_gpu_options(), krun_add_display()   // GPU (optional)
krun_set_snd_device()                        // sound (optional)
```

### Device Support Matrix

| Device | Supported | Notes |
|--------|-----------|-------|
| virtio-vsock | Yes | Used for TSI and socket redirection |
| virtio-block | Yes | Compile with `BLK=1` feature flag |
| virtio-fs | Yes | Built-in, primary rootfs mechanism |
| virtio-net | Yes | Compile with `NET=1`, requires passt/gvproxy |
| virtio-console | Yes | Built-in |
| virtio-gpu | Yes | Optional (`GPU=1`), venus + native-context |
| virtio-balloon | Yes | Free-page reporting only |
| virtio-rng | Yes | Built-in |
| virtio-snd | Yes | Optional (`SND=1`) |

---

## Calling from Rust

Three options exist:

### Option 1: Direct Rust crate dependency (NEW — March 2026)

A commit on 2026-03-07 ("Produce a proper Rust library") changed the
`Cargo.toml` to produce both `cdylib` and `lib` crate types. The library name
is `krun`. Functions still use C-style signatures (`*const c_char`, etc.), but
linkage is native Rust — no FFI overhead.

```toml
# Git dependency (not yet on crates.io as a lib)
krun = { git = "https://github.com/containers/libkrun", tag = "v1.17.4" }
```

### Option 2: krun-sys crate (FFI bindings)

```toml
krun-sys = "1.10.1"  # on crates.io, 3,381 downloads
```

Requires `libkrun.so` pre-installed on the system. Uses pkg-config to find it.

### Option 3: Third-party wrappers

- `msb_krun` (v0.1.9) — "Native Rust API for libkrun microVMs"
- `libkrun-sys` (v0.8.2) — independent FFI bindings

---

## Critical Limitations

### 1. `krun_start_enter()` blocks forever and calls `_exit()`

**Verified in source code** at `src/vmm/src/lib.rs:357-371`:

```rust
// Vmm::stop() — called when guest VM exits
pub fn stop(&mut self, exit_code: i32) {
    info!("Vmm is stopping.");
    for observer in &self.exit_observers {
        observer.lock().expect("Poisoned mutex").on_vmm_exit();
    }
    // Comment in code: "Exit from Firecracker using the provided exit code.
    // Safe because we're terminating the process anyway."
    unsafe { libc::_exit(exit_code); }
}
```

The call chain: `krun_start_enter()` → infinite `event_manager.run()` loop →
on guest exit event → `Vmm::process()` → `self.stop(exit_code)` →
`libc::_exit()`.

**Impact:**
- The entire process dies immediately — no stack unwinding, no Drop, no atexit.
- Multiple VMs on multiple threads: any VM exiting kills all of them.
- You must isolate each VM in its own process.

**Confirmed by maintainers in GitHub issues:**

- **Issue #561** (Feb 2026, open): "libkrun terminates entire process on VM
  exit, the caller has no chances to clean up resources." Maintainer `slp`
  confirmed: "a known inconvenient... On 2.x there'll be an option to not tear
  down the whole process on VM exit. For now, the best approach is
  fork+spawn."

- **Issue #373** (Jul 2025, open): Maintainer `mtjhrc` explained the root
  cause: "The clean shutdown paths are untested or are not implemented at all,
  hence we just `_exit` the process currently. We probably don't have proper
  Drop implementations in places."

- **PR #494** (Dec 2025, draft, open): "Rust API" — proposes separating VM
  building from VM execution as a prerequisite for eventually making
  `krun_start_enter` return cleanly. **Not merged, does not yet fix _exit.**

**Fix timeline:** v2.x, no specific date. The infrastructure work (Drop impls,
clean thread shutdown) is acknowledged as significant.

### The fork/subprocess workaround

This is NOT as bad as it sounds for neovex. Two viable patterns:

**Pattern A: Small helper binary (recommended, what smolvm does)**

```rust
// neovex-vmm-helper: tiny static binary (~100 lines)
fn main() {
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL); }
    let config = read_config_from_stdin();
    let ctx = krun_create_ctx();
    krun_set_vm_config(ctx, config.vcpus, config.ram_mib);
    // ... configure VM ...
    krun_start_enter(ctx); // blocks, calls _exit on VM shutdown
}

// In neovex (has tokio, V8, etc.):
let child = tokio::process::Command::new("neovex-vmm-helper")
    .stdin(Stdio::piped())
    .spawn()?;
// Send config, monitor via waitpid, communicate via vsock
```

This is safe, efficient (tiny process, no COW overhead from V8), and is
exactly what `tokio::process::Command` is designed for. The child dying via
`_exit()` is handled gracefully by `child.wait()`.

**Pattern B: Direct fork() (risky in multi-threaded Rust)**

Fork from the neovex process directly. Risky because:
- Forking a process with tokio runtime leaves corrupted epoll state in child
- V8's large address space means expensive page table duplication (10-50ms)
- Lock corruption risk from any mutex held at fork time

**Pattern A is strictly superior.** It's the same pattern neovex already uses
for V8 test isolation (`crates/neovex-runtime/src/test_support/isolation.rs`
uses `Command::new()` for process isolation).

`krun_get_shutdown_eventfd()` is only available in the EFI variant and only
signals the guest to shut down — it does NOT prevent `_exit()` on the host.

### 2. No snapshot/restore

libkrun has **no snapshot or restore capability**. No API functions exist for
this. This was never implemented, and there are no open issues or PRs for it.

This means:
- Every VM boot is a cold boot.
- You cannot pre-warm VM templates and restore them in ~5-10ms.
- Boot times are the full cold-boot path (estimated sub-100ms but not sub-10ms).

This is the **single biggest gap** compared to Firecracker.

### 3. Requires libkrunfw

`libkrunfw` is a separate library that bundles a custom Linux kernel (with TSI
patches not upstreamed to mainline). It is a hard dependency on Linux.

- Must be installed system-wide (or bundled)
- Adds deployment complexity
- The TSI kernel patches tie you to libkrunfw's kernel version

### 4. Build system uses Make, not pure Cargo

The build uses `make` with feature flags (`BLK=1`, `NET=1`, `GPU=1`). This
complicates integration into a pure-Cargo workspace. You would need custom
build scripts or pre-built artifacts.

---

## TSI (Transparent Socket Impersonation)

TSI is libkrun's signature feature. It intercepts socket syscalls (AF_INET,
AF_INET6, optionally AF_UNIX) in the guest kernel and transparently redirects
them over vsock to the VMM, which performs the actual socket operations on the
host.

**Effect:** Guest processes can make network connections (HTTP, database, etc.)
without any virtual network interface, TAP device, bridge, or IP assignment.
Port forwarding from host to guest is available via `krun_set_port_map()`.

**Requires:** The custom kernel from libkrunfw (patches not in mainline Linux).

**Limitations:**
- Raw sockets not supported
- Listening on SOCK_DGRAM not supported from guest
- AF_UNIX only with absolute paths
- Guest inherits host's network security context

**Relevance to Neovex:** Very useful for agent workloads that need HTTP access.
Eliminates all networking complexity. However, ties you to the libkrunfw kernel.

---

## Projects That Embed libkrun

| Project | Stars | Process Model | _exit() Handling | Key Insight |
|---------|-------|--------------|------------------|-------------|
| **crun/krun** (containers/crun) | 3.5k+ | `clone()` child (OCI runtime) | **By design**: called where `execve()` would go. Podman reads exit code via `waitpid()`. | Production-proven. virtiofs rootfs, built-in init, OCI config via file. |
| **muvm** (AsahiLinux/muvm) | 846 | Singleton (file lock) | **Accepts it**: process is sacrificial. Subsequent invocations RPC to guest server. | Elegant singleton pattern — first call creates VM, rest send commands to it. |
| **krunkit** (containers/krunkit) | 264 | Single process (macOS) | **Accepts it** + `krun_get_shutdown_eventfd()` for graceful shutdown via REST API. | Background thread runs REST server for Podman to control VM lifecycle. |
| **krunvm** (containers/krunvm) | ~400 | Single process | Accepts it. | Uses buildah for OCI images + virtiofs. |
| **smolvm** (smol-machines/smolvm) | 162 | Fork per VM | **fork()** before `krun_start_enter()`. Parent monitors child. | Closest to neovex's helper binary pattern. |

### How each project handles the process lifecycle

**crun (most relevant for neovex):** The OCI runtime is already a forked child
process by the time it calls `krun_start_enter()`. This is the canonical
pattern — the VMM replaces the child process, same as `execve()`.

**muvm (interesting alternative):** Uses a singleton pattern. First `muvm`
invocation acquires a file lock and becomes the VM. Subsequent `muvm`
invocations detect the lock, connect to a Unix socket, and send launch requests
to the `muvm-guest` server running inside the VM. This means only one VM per
user, but multiple commands share it.

**krunkit (macOS reference):** Spawns a background REST API thread *before*
calling `krun_start_enter()`. The thread survives because it runs inside the
same process libkrun takes over. Podman sends `POST` to shut down the VM
gracefully via `krun_get_shutdown_eventfd()`.

### Pattern analysis for neovex

| Pattern | Used By | Pros | Cons | Fit for Neovex |
|---------|---------|------|------|----------------|
| **Helper binary** (Command::new) | smolvm, implied by crun | Safe, clean, tokio-compatible | Extra binary to ship | **Best fit** |
| **Singleton** (file lock + RPC) | muvm | Elegant, one VM serves many | Only one VM per user | Good for long-running agents |
| **Sacrificial process** (just accept it) | krunvm, krunkit | Simplest | Parent process dies | Only if neovex IS the VM |
| **Background thread + REST** | krunkit | Graceful shutdown control | macOS-specific pattern | Useful for lifecycle API |

---

## crun/krun: The Reference Implementation

crun (`containers/crun`) is the canonical OCI container runtime for
Podman/Red Hat. Its libkrun integration (`src/libcrun/handlers/krun.c`) is the
most battle-tested consumer of libkrun, shipping in Fedora as `crun-krun`.

### How crun uses libkrun

```c
// Simplified from crun's krun handler
int libkrun_exec(/* ... */) {
    ctx_id = krun_create_ctx();
    krun_set_vm_config(ctx_id, vcpus, ram_mib);
    krun_set_root(ctx_id, "/");      // virtiofs passthrough of chrooted rootfs
    krun_set_workdir(ctx_id, cwd);
    krun_set_env(ctx_id, env);
    krun_start_enter(ctx_id);        // never returns — _exit() on VM shutdown
    return -1; // unreachable
}
```

### Key architectural lessons from crun

1. **virtiofs for rootfs:** `krun_set_root("/")` exposes the host directory to
   the guest via virtiofs. No ext4 images, no block devices. For neovex: unpack
   OCI layers to a directory, pass directory path to `krun_set_root()`.

2. **OCI config via file:** crun writes `.krun_config.json` to the rootfs.
   libkrun's init reads it. No vsock config protocol needed for basic cases.

3. **Exit code via virtiofs ioctl:** The guest init calls
   `ioctl(fd, KRUN_EXIT_CODE_IOCTL, code)` on the rootfs. The VMM captures
   this and passes it to `_exit()`. The parent reads it via `waitpid()`.

4. **Dynamic loading:** crun uses `dlopen("libkrun.so.1")` and resolves
   symbols via `dlsym()`. This keeps libkrun as an optional dependency. For
   neovex's helper binary, static linking is simpler.

5. **Production-proven:** Red Hat's RamaLama project uses `--oci-runtime krun`
   for AI model inference isolation. Confidential Containers uses the SEV
   variant.

### What neovex's helper would look like (modeled on crun)

```rust
// neovex-vmm-helper/src/main.rs (~50 lines)
fn main() {
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL); }

    let config: VmConfig = serde_json::from_reader(std::io::stdin()).unwrap();

    let ctx = krun_create_ctx();
    krun_set_vm_config(ctx, config.vcpus, config.ram_mib);
    krun_set_root(ctx, &config.rootfs_path);  // virtiofs!
    krun_set_workdir(ctx, &config.working_dir);

    for env_var in &config.env {
        krun_set_env(ctx, env_var);
    }

    // Add vsock for host↔guest communication beyond basic lifecycle
    krun_add_vsock_port(ctx, config.vsock_port, VSOCK_TYPE_STREAM);

    krun_start_enter(ctx);
    // unreachable — _exit() propagates workload exit code
}
```

This is ~50 lines. Compare to the Firecracker sidecar approach which requires
~200+ lines of HTTP-over-UDS API client code.

---

## Comparison with Firecracker

| Dimension | libkrun | Firecracker |
|-----------|---------|-------------|
| **Deployment** | Helper binary (~50 lines) | Helper binary (~200+ lines) or direct sidecar |
| **API** | C function calls (direct) | REST over Unix socket (HTTP serialization) |
| **Boot time** | Sub-100ms (cold only) | ~125ms cold, **~5-10ms snapshot restore** |
| **Snapshots** | **No** | Yes (mature, critical for rapid invocation) |
| **Rootfs handling** | **virtiofs from directory** (no ext4 images!) | ext4 block device (requires `mkfs.ext4` pipeline) |
| **Networking** | TSI (transparent, zero config) | TAP + iptables (manual setup, error-prone) |
| **Guest init** | **Built-in** (`init/init.c`, reads OCI config) | Must provide your own (neovex-init) |
| **Exit code** | Propagated via virtiofs ioctl, automatic | Must implement via vsock protocol |
| **Security isolation** | Helper process (same as FC) | Jailer provides extra sandboxing |
| **virtiofs** | Yes (built-in, primary mechanism) | No |
| **Kernel** | Bundled in libkrunfw (zero management) | Separate vmlinux (download, cache, verify) |
| **macOS support** | Yes (Hypervisor.framework) | No (Linux only) |
| **OCI integration** | Proven (crun/Podman ships it) | Must build from scratch |
| **Helper binary complexity** | ~50 lines | ~200+ lines (HTTP client + API) |
| **License** | Apache 2.0 | Apache 2.0 |

---

## Verdict for Neovex

### The `_exit()` is a design pattern, not a bug

**The crun integration proves this conclusively.** crun is the production OCI
runtime used by Podman/Red Hat. It ships in Fedora as `crun-krun`. Here's how
it works:

1. Podman unpacks the OCI image to a rootfs directory
2. crun `clone()`s into namespaces and chroots into that rootfs
3. The child calls `krun_start_enter()` — the child process *becomes* the VMM
4. When the VM exits, `_exit()` terminates the child with the workload's exit code
5. Podman reads the exit code via `waitpid()` — it looks identical to a normal
   container process

**`krun_start_enter()` is designed to be called where `execve()` would go.**
The VMM replaces the child process, just like `execve()` replaces a process
with a new program. The `_exit()` is the normal process termination.

This maps perfectly to neovex's helper binary pattern:

```
crun pattern:                    neovex pattern:
podman/conmon                    neovex (tokio + V8)
  └── clone(crun/krun)             └── Command::new("neovex-vmm-helper")
        └── krun_start_enter()           └── krun_start_enter()
              └── _exit(code)                  └── _exit(code)
```

Both are architecturally identical. The parent monitors the child via
`waitpid()` and reads the exit code. This is standard Unix process management.

### Real advantages of libkrun over Firecracker

| Advantage | Impact for Neovex |
|-----------|-------------------|
| **virtiofs eliminates ext4 pipeline** | crun proves you can pass an unpacked OCI rootfs directory directly to `krun_set_root("/path/to/rootfs")`. libkrun exposes it via virtiofs to the guest. **No `mkfs.ext4`, no disk images, no block devices.** This eliminates the most complex part of the Firecracker pipeline. |
| **TSI networking** | No TAP devices, no iptables, no IP assignment. Guest processes transparently use host network. Massive simplification for agent workloads that need HTTP. |
| **OCI config via `.krun_config.json`** | crun writes the OCI config.json to the rootfs. libkrun's built-in init reads it for entrypoint/cmd/env/workdir. No custom init needed for basic cases. |
| **Exit code propagation** | libkrun's init propagates the workload exit code via a virtiofs ioctl → the helper process exits with the workload's actual exit code. No vsock protocol needed for this. |
| **macOS support** | Agents work on developer machines (macOS + Hypervisor.framework), not just Linux servers. |
| **No kernel management** | libkrunfw bundles the kernel. No separate vmlinux download/caching. |
| **Stable API (SemVer since v1.0)** | Firecracker's API changes between versions. libkrun guarantees stability. |
| **Built-in init** | libkrun ships a C init (`init/init.c`) that handles mounts, networking, OCI config parsing, and workload exec. Neovex may not need a custom `neovex-init` at all for basic cases. |

### Real disadvantages

| Disadvantage | Impact |
|--------------|--------|
| **No snapshots** | Cannot achieve sub-10ms restore. Cold boot only (~100ms). Acceptable for long-running agents, problematic for rapid invocations. |
| **libkrunfw dependency** | System-level dependency with custom kernel. Adds deployment complexity. |
| **Build uses Make** | Can't add as a pure Cargo dependency without a build script or pre-built binary. |
| **TSI requires custom kernel** | Guest kernel patches not in mainline Linux. |

### Recommendation (updated)

**libkrun deserves equal consideration with Firecracker, not dismissal.** The
architecture is the same either way (helper binary per VM), and libkrun offers
significant simplification (TSI, virtiofs, no kernel management).

**Proposed evaluation path:**

1. **Prototype both** as `neovex-vmm-helper` variants:
   - `neovex-vmm-helper-fc`: spawns Firecracker, configures via REST API
   - `neovex-vmm-helper-krun`: calls libkrun C API directly
   - Same vsock protocol to the guest init in both cases

2. **Compare on real workloads:**
   - Cold boot time (Firecracker ~125ms vs libkrun ~100ms)
   - Networking setup complexity (TAP+iptables vs TSI)
   - OCI image handling (ext4 rootfs vs virtiofs mount)
   - Memory overhead

3. **Decision point:** If snapshot/restore (sub-10ms boot) is critical for
   neovex's agent invocation model → Firecracker wins. If agents are
   long-running (boot once, run for minutes/hours) → libkrun's simplicity
   wins.

4. **The custom rust-vmm VMM (Phase 2 in the original plan) may be
   unnecessary** if either Firecracker or libkrun meets needs via the helper
   binary pattern. Building a custom VMM is 2,500-4,000 lines of low-level
   KVM code that you must maintain forever.

### The helper binary pattern IS the answer

Regardless of which VMM backend (Firecracker, libkrun, or custom rust-vmm),
the architectural pattern is the same:

```
neovex process (tokio + V8 + engine)
    |
    +-- tokio::process::Command::new("neovex-vmm-helper")
    |       |-- communicates via vsock to guest init
    |       |-- parent monitors via child.wait()
    |       |-- PR_SET_PDEATHSIG ensures cleanup
    |
    +-- The VMM runs inside the helper process
    |       |-- Firecracker: helper spawns FC as grandchild
    |       |-- libkrun: helper calls krun_start_enter() directly
    |       |-- custom VMM: helper runs VMM in-process
    |
    +-- Guest VM
            |-- neovex-init (PID 1)
            |-- vsock API server
            |-- OCI entrypoint
```

This pattern gives you:
- Process isolation (VMM crash doesn't kill neovex)
- Clean lifecycle management (waitpid + PR_SET_PDEATHSIG)
- Backend swappability (same interface, different VMM)
- Tokio compatibility (no fork from async context)
