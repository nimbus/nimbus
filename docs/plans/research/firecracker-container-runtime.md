# Firecracker Container Runtime for Nimbus

Research into adding a Firecracker-based container runtime that runs OCI/Docker
images inside microVMs, as a new execution backend alongside V8 and the planned
Wasmtime backend.

**Date:** 2026-04-09
**Updated:** 2026-04-09 (verification pass — corrected crate versions, Cloud
Hypervisor claims, added libkrun evaluation)
**Status:** Research complete, ready for planning

---

## Problem Statement

Nimbus embeds V8 isolates for running user-defined JavaScript functions. The
planned Wasmtime backend adds WASM execution. A third runtime is needed to run
arbitrary Docker images inside hardware-isolated microVMs — enabling agent
workloads that need a full Linux environment (filesystem, processes, networking)
without the security risks of running containers on the host.

Fly.io pioneered the "Docker without Docker" pattern: convert OCI images to
ext4 rootfs, boot them in Firecracker microVMs with a custom init, communicate
over vsock. This research evaluates three approaches to implementing this in
Nimbus.

---

## Three Approaches Evaluated

### Approach A: Firecracker as a Managed Child Process (Sidecar)

Spawn the upstream Firecracker binary as a child process, configure it via its
REST API over a Unix domain socket, communicate with the guest over vsock.

**This is what every production system uses:** AWS Lambda, Fly.io, E2B, Kata
Containers. It is proven at enormous scale.

**Pros:**
- Security: Firecracker runs in its own process with jailer (chroot, seccomp,
  cgroup isolation). A VMM bug does not compromise Nimbus.
- Simplicity: Well-documented REST API. ~10 endpoints to implement.
- Battle-tested: Powers AWS Lambda (billions of invocations).
- No Rust VMM expertise needed.

**Cons:**
- Requires bundling or downloading the Firecracker binary (~3MB).
- IPC overhead: HTTP serialization over Unix socket for each API call.
- Socket/PID file management, cleanup on crash.
- Limited to Firecracker's feature set (no virtiofs, no GPU, no hotplug).

#### Firecracker API Overview

| Method | Endpoint | Purpose |
|--------|----------|---------|
| PUT | `/boot-source` | Kernel image path + boot args |
| PUT | `/drives/{id}` | Attach block device (rootfs) |
| PUT | `/network-interfaces/{id}` | Attach TAP network interface |
| PUT | `/vsock` | Configure virtio-vsock (CID + UDS path) |
| PUT | `/machine-config` | vCPU count, memory size |
| PUT | `/actions` | `InstanceStart`, `SendCtrlAltDel` |
| PATCH | `/vm` | Pause/Resume |
| PUT | `/snapshot/create` | Full or diff snapshot |
| PUT | `/snapshot/load` | Restore from snapshot |

Configure-then-boot model: PUT all resources, then PUT `/actions` with
`InstanceStart`.

#### Rust SDK Landscape

| Crate | Status | Notes |
|-------|--------|-------|
| `firepilot` | Exists on crates.io, young (~v0.1.x) | Community SDK, single maintainer. Evaluate carefully. |
| `firecracker-http-client` | Uncertain existence | May be internal to Firecracker repo. Verify on crates.io. |
| `firecracker-rs-sdk` | Uncertain existence | Verify on crates.io. |

**Recommendation:** Write a thin HTTP-over-UDS client. The API is ~10 endpoints
of simple JSON. Use `hyper` + `hyperlocal` (or `reqwest` with Unix socket
connector). This is what Fly.io, E2B, and AWS do — they all wrote their own
thin client rather than depending on third-party SDKs.

#### The Jailer

The `jailer` binary creates a chroot jail, drops privileges, sets cgroup limits,
then execs Firecracker inside the jail. Use it in production (configurable via
`use_jailer: bool`). Without it, Firecracker runs as root with full filesystem
access.

---

### Approach B: Cloud Hypervisor's `vmm` Crate as a Library

**⚠ CORRECTED after verification. Several original claims were wrong.**

Cloud Hypervisor (currently at **v51.1**, ~5k stars, Intel/Microsoft/ARM) is
structured as a Cargo workspace with 23 crates. The `vmm` crate is a library,
but the claims about easy embedding were overstated.

**Corrections from verification:**
- `main.rs` is **2,038 lines**, not ~200. It contains substantial CLI parsing
  (70+ clap arguments), logging setup, signal handling, and orchestration.
- `ApiRequest` is **not an enum** — it is a `Box<dyn FnOnce(&mut dyn
  RequestHandler)>` closure type alias. There are ~31 action structs
  implementing an `ApiAction` trait.
- **Zero evidence exists** of anyone successfully using the `vmm` crate as a
  git dependency from an external project. No GitHub issues, blog posts, or
  projects demonstrate this.
- The latest version is **v51.1** (Feb 2026), not v43.0.
- Uses Rust edition 2024, MSRV 1.89.0.

**Pros:**
- Rich feature set: CPU/memory hotplug, virtiofs, live migration, PCI, VFIO.
- Battle-tested VMM code.
- Direct Rust code (no IPC if you could embed it).

**Cons:**
- **Security:** VMM in your process. A guest escape compromises Nimbus.
- **No stability guarantee:** Internal API, changes every release, no semver.
- **Not published on crates.io.** Must use git dep on full repo (23 crates).
- **Unverified embeddability.** Nobody has done this. The API surface (boxed
  closures, trait objects) is not designed for external consumers.
- **Compile time:** 15-25 minutes clean on the E550.
- **Heavy:** 23 workspace crates, ~120k lines of Rust.
- **main.rs is 2,038 lines** — integrating without the CLI would require
  reimplementing significant orchestration logic.

```toml
# Theoretically possible but UNVERIFIED — nobody has done this
vmm = { git = "https://github.com/cloud-hypervisor/cloud-hypervisor", tag = "v51.1" }
```

**Verdict: Not recommended.** The embeddability story is theoretical, the API
is not designed for library consumers, and nobody has demonstrated it working.

---

### Approach C: Custom Minimal VMM from rust-vmm Crates (Recommended)

Build a purpose-built minimal VMM from the `rust-vmm` ecosystem's published
crates. This is exactly what Firecracker, Cloud Hypervisor, and crosvm each
did.

**Pros:**
- Full control over API surface — design it to fit `WorkerLoopFactory`/`WorkerLoop`.
- Uses stable, semver'd crates from crates.io.
- Minimal compile time (~3-5 min clean).
- Minimal code (~2,500-4,000 lines in a new `nimbus-vmm` crate).
- Can be embedded in-process OR run as a child process (your choice per deployment).

**Cons:**
- Must implement virtio-block and virtio-vsock device backends.
- Requires understanding KVM, virtio, and x86_64 boot.
- More upfront engineering than the sidecar approach.

#### Minimum Crate Set (verified on crates.io 2026-04-09)

```toml
[dependencies]
kvm-ioctls = "0.24"          # last updated 2025-10-29
kvm-bindings = { version = "0.14", features = ["fam-wrappers"] }  # 2025-09-16
vm-memory = { version = "0.18", features = ["backend-mmap"] }     # 2025-12-11
linux-loader = { version = "0.13", features = ["bzimage", "elf"] } # 2025-11-20
virtio-queue = "0.17"        # 2025-11-17
vm-superio = "0.8"           # 2025-09-27
vmm-sys-util = "0.15"        # 2025-08-07
event-manager = "0.4"        # 2025-11-20
```

All published on crates.io with proper semver. Actively maintained by AWS,
Google, Intel, Red Hat. Download counts in the millions.

#### Code Size Estimate

| Component | Lines |
|-----------|-------|
| KVM setup + CPU configuration | 400-600 |
| x86_64 boot (page tables, GDT, CPUID) | 300-500 |
| Kernel loading + boot params | 150-250 |
| Virtio MMIO transport | 300-500 |
| Virtio-block device | 500-800 |
| Virtio-vsock (via vhost-vsock kernel module) | 200-400 |
| VM lifecycle / event loop | 200-400 |
| Error handling, config, tests | 500-800 |
| **Total** | **2,500-4,250** |

Comparable to a single nimbus crate.

**Note on vmm-reference:** `rust-vmm/vmm-reference` was previously cited as a
starting point. **Verification found it is effectively abandoned** (last commit
September 2022, stale PRs, no vsock support). It supports virtio-block and
virtio-net but NOT virtio-vsock. The code structure is still useful as a
learning reference but should not be forked or depended on. Build from the
published rust-vmm crates directly instead.

#### Tokio Compatibility

- `KVM_RUN` ioctl is blocking — must run on dedicated `std::thread`, not tokio
  worker.
- `EventFd` (device signaling) is compatible with tokio's `AsyncFd`.
- Bridge pattern: tokio channels for VM commands, `AsyncFd` for device
  notifications, dedicated thread for vCPU execution.

---

### Approach D: libkrun (VMM as Library)

`containers/libkrun` (**1,808 stars**, Red Hat-backed, Sergio Lopez maintainer)
is the only true "VMM as library" in the Rust ecosystem. Built on rust-vmm
crates. Apache 2.0. **v1.17.4** (Feb 2026), last commit 2026-04-07. Stable API
since v1.0.0 (SemVer guaranteed).

As of March 2026, libkrun can be used as a **direct Rust crate dependency**
(no FFI needed) — a commit added `lib` crate type alongside `cdylib`.

**See `docs/plans/research/libkrun-evaluation.md` for full analysis.**

**Unique features:** virtiofs (built-in), TSI (Transparent Socket
Impersonation — guest sockets transparently proxied through host, no TAP/
bridges/IP config), GPU support on macOS via Hypervisor.framework.

**Critical limitations:**
1. **`krun_start_enter()` blocks forever and calls `_exit()` on VM shutdown**
   — killing the entire process. You must `fork()` per VM. This negates the
   "single-process" advantage and is acknowledged as a bug (issue #373) but
   not yet fixed.
2. **No snapshot/restore** — every boot is cold. Cannot achieve sub-10ms
   restore like Firecracker.
3. **Requires libkrunfw** — separate shared library bundling a custom Linux
   kernel with TSI patches not in mainline.
4. **Build uses Make, not pure Cargo** — complicates workspace integration.

**Verdict:** Interesting but not the right primary choice. The `_exit()` bug
means you end up with child processes anyway (via `fork()`), and no snapshots
means slower boot than Firecracker. Consider as a future macOS backend.

---

## OCI Image to Rootfs Pipeline

### Pipeline Steps

1. **Pull image manifest** from OCI registry
2. **Download layers** (gzipped tarballs)
3. **Flatten layers** in order, handling whiteout files (`.wh.*` deletions)
4. **Create ext4 image** using `mkfs.ext4 -d <source_dir>` (available in
   e2fsprogs >= 1.43, present on Debian 13)
5. **Inject custom init** binary at `/sbin/nimbus-init`

### Rust Crates (verified on crates.io 2026-04-09)

| Crate | Version | Downloads | Purpose |
|-------|---------|-----------|---------|
| `oci-client` | 0.16.1 | 2.9M | Pull images from OCI registries. Successor to `oci-distribution` (deprecated). Now under `oras-project`. |
| `oci-spec` | 0.9.0 | 12.3M | OCI image/runtime spec types. `containers` org. |
| `flate2` | 1.x | — | Gzip decompression. Stable. |
| `tar` | 0.4.x | — | Tarball extraction. Stable. |

**Note:** Do NOT use `oci-distribution` (v0.11.0, last updated 2024-03, deprecated).
Use `oci-client` instead — same project, renamed and actively maintained.

### Fly.io vs firecracker-containerd

**Fly.io approach (recommended for Nimbus):**
- Direct: OCI image → ext4 rootfs → Firecracker VM with custom init
- No containerd, no Docker daemon at runtime
- Init handles mounts, networking, runs entrypoint, exposes vsock API
- Simpler, fewer moving parts, matches single-binary philosophy

**firecracker-containerd approach:**
- Uses containerd as orchestrator + a v2 shim that spawns Firecracker
- Guest runs full container agent speaking containerd gRPC over vsock
- More complex, more "standard", designed for Kubernetes integration
- Overkill for Nimbus's use case

---

## Guest Init Design

### Responsibilities (PID 1 in the microVM)

1. Mount `/proc`, `/sys`, `/dev`, `/dev/pts`, `/dev/shm`, `/tmp`, `/run`
2. Create device symlinks (`/dev/stdin`, `/dev/stdout`, `/dev/stderr`)
3. Read configuration from host via vsock (JSON blob with entrypoint, env,
   networking config)
4. Configure networking (loopback + eth0 with IP/gateway)
5. Set hostname, write `/etc/resolv.conf`
6. Optionally signal "ready for snapshot" to host
7. Start vsock API server (background thread)
8. Fork and exec the OCI entrypoint
9. Reap zombie children (PID 1 obligation)
10. Handle shutdown signals

### Build

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl -p nimbus-init
# Result: statically linked binary, zero runtime dependencies
```

### Vsock API Protocol

**Recommendation: Length-prefixed JSON-RPC 2.0 over vsock.**

- 4-byte big-endian length prefix + JSON body
- Methods: `health.check`, `process.signal`, `process.status`, `logs.stream`,
  `vm.shutdown`
- gRPC is overkill; custom binary is hard to debug; JSON-RPC is simple,
  self-describing, good Rust support

---

## Vsock Communication

### How it Works

CID addressing: Host is always CID 2. Guests get CID >= 3. Port numbers are
32-bit (no privilege for ports < 1024).

**Firecracker-specific:** On the host, Firecracker proxies vsock over Unix
domain sockets. The host does NOT use `AF_VSOCK` directly.

- **Host → Guest:** Connect to the vsock UDS, send `CONNECT <port>\n`, read
  `OK <port>\n`, then bidirectional stream.
- **Guest → Host:** Guest connects to CID 2 on port P. Firecracker forwards to
  `{uds_path}_{port}` — a separate UDS file per port.

### Crates

| Crate | Use | Notes |
|-------|-----|-------|
| `tokio-vsock` | Host side (if using AF_VSOCK directly) | Async, tokio-based |
| `vsock` | Guest init (synchronous) | Minimal deps |

**Important:** On the host with Firecracker, you use `tokio::net::UnixStream`
to talk to the vsock UDS, not `AF_VSOCK`. Only the guest uses `AF_VSOCK`.

---

## Kernel Management

### Sources

- Firecracker publishes pre-built `vmlinux` kernels in releases and S3
- Kernel configs at `firecracker/resources/guest_configs/`
- Must be uncompressed ELF (`vmlinux`), not `bzImage`

### Minimum Kernel Config

```
CONFIG_KVM_GUEST=y
CONFIG_VIRTIO=y CONFIG_VIRTIO_MMIO=y CONFIG_VIRTIO_BLK=y
CONFIG_VIRTIO_NET=y CONFIG_VSOCKETS=y CONFIG_VIRTIO_VSOCKETS=y
CONFIG_EXT4_FS=y CONFIG_TMPFS=y CONFIG_DEVTMPFS=y CONFIG_DEVTMPFS_MOUNT=y
CONFIG_PROC_FS=y CONFIG_SYSFS=y
CONFIG_SERIAL_8250=y CONFIG_SERIAL_8250_CONSOLE=y
# Disable: MODULES, SOUND, USB, WIRELESS, BLUETOOTH, DRM, FB, DEBUG_INFO
```

### Strategy

Download on first use, cache at `~/.nimbus/kernels/`. Verify SHA-256 checksum.
Don't bundle in the binary (15-25MB bloat).

---

## Snapshot/Restore

### Cold Boot vs Snapshot Restore

| Metric | Time |
|--------|------|
| Cold boot to init | ~200-300ms |
| Snapshot restore to running | ~5-50ms (depends on RAM size) |

### Template Snapshot Strategy

For each OCI image:
1. Cold-boot a VM, wait for init to signal "ready" over vsock
2. Pause VM, take full snapshot (state + memory file)
3. Cache snapshot files
4. Subsequent invocations: copy memory file (use reflink/CoW on btrfs/xfs),
   load snapshot, resume in ~10ms
5. Send per-instance config over vsock after resume

---

## Resource Constraints (ThinkPad E550: 2c/4t, 8GB)

After host OS (~1GB) and Nimbus+V8 (~200-500MB), ~6.5-7GB available for VMs.

| Scenario | vCPU | RAM | Max Concurrent |
|----------|------|-----|----------------|
| Lightweight (Alpine) | 1 | 128MB | 20-30 |
| Medium (Node.js/Python) | 1 | 256MB | 10-15 |
| Heavy (JVM) | 1 | 512MB | 5-8 |
| Development | 1 | 128MB | 5-10 |

**Optimizations:**
- Enable KSM (Kernel Same-page Merging) for cross-VM memory deduplication
- Use Firecracker's balloon device to reclaim unused guest memory
- Use reflink copy for snapshot memory files (btrfs/xfs)

---

## Existing Projects Reference

| Project | Pattern | VMM | Status | Stars | Lesson |
|---------|---------|-----|--------|-------|--------|
| AWS Lambda | Sidecar | Firecracker | Active | — | Proven at extreme scale |
| Fly.io | Sidecar (forked FC) | Firecracker | Active | — | "Docker without Docker" pattern |
| E2B | Sidecar | Firecracker | Active | ~1.5k | ~28ms boot via snapshots |
| Kata Containers | Sidecar | FC/CH/QEMU | Active (CNCF) | ~5.5k | Go shim + Rust guest agent |
| libkrun | Embedded | Custom (rust-vmm) | Active (Red Hat) | 1,808 | Only "VMM as library" but `_exit()` bug |
| krunvm | Embedded (via libkrun) | libkrun | Active | ~400 | OCI-to-microVM CLI |
| muvm | Embedded (via libkrun) | libkrun | Active | 846 | GPU VMs on Apple Silicon |
| Hocus | Sidecar | Firecracker | **Dead** (2023) | ~650 | Cautionary: VMM simple, surrounding infra hard |
| Ignite | Sidecar | Firecracker | **Dead** (Weaveworks) | ~3.5k | Company died (Feb 2024) |
| Flintlock | Sidecar | FC/CH | **Dead** (Weaveworks) | ~450 | gRPC VM lifecycle API worth studying |

---

## Recommendation

**For Nimbus, Approach C (custom minimal VMM from rust-vmm crates) is the best
fit**, with Approach A (Firecracker sidecar) as a pragmatic alternative for
faster time-to-first-VM.

**Phased strategy:**

1. **Phase 1:** Implement Firecracker sidecar to validate the end-to-end
   architecture (OCI pull → rootfs → boot → vsock → HostBridge). This gets a
   working system quickly.
2. **Phase 2:** Build `nimbus-vmm` crate from rust-vmm building blocks,
   replacing the Firecracker child process with an in-process VMM. This
   eliminates the external binary dependency and gives full API control.
3. **Phase 3:** Add snapshot/restore for sub-10ms VM creation.

This mirrors how the V8 runtime evolved — get it working first, optimize later.
