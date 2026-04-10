# Firecracker Runtime: Implementation Sketches

Code sketches for integrating a Firecracker-based container runtime into Neovex.
These are reference designs, not production code. See
`firecracker-container-runtime.md` for the research context.

**Date:** 2026-04-09

---

## Workspace Structure

```
crates/
  neovex-vmm/           # NEW: microVM management (host side)
    src/
      lib.rs            # Public API: VmPool, VmHandle
      firecracker.rs    # Firecracker sidecar management (Phase 1)
      vmm.rs            # Custom VMM from rust-vmm (Phase 2)
      oci.rs            # OCI image pull + rootfs creation
      vsock.rs          # Host-side vsock communication
      snapshot.rs       # Snapshot cache and restore
      kernel.rs         # Kernel download and management
  neovex-init/          # NEW: Guest init binary (PID 1 inside VM)
    src/
      main.rs           # Mount, configure, exec entrypoint
      vsock_api.rs      # JSON-RPC API server over vsock
      network.rs        # Guest networking setup
```

---

## 1. Firecracker API Client (Phase 1)

Thin HTTP-over-Unix-socket client. No third-party SDK needed.

```rust
// crates/neovex-vmm/src/firecracker.rs

use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde::Serialize;

pub struct FirecrackerVm {
    id: String,
    api_socket: PathBuf,
    vsock_uds: PathBuf,
    process: Option<Child>,
    guest_cid: u32,
}

impl FirecrackerVm {
    pub fn new(work_dir: &Path, guest_cid: u32) -> Self {
        let id = ulid::Ulid::new().to_string();
        Self {
            api_socket: work_dir.join(format!("fc-{}.sock", &id[..8])),
            vsock_uds: work_dir.join(format!("vsock-{}.sock", &id[..8])),
            id,
            process: None,
            guest_cid,
        }
    }

    /// Spawn Firecracker process. Does not start the VM.
    pub async fn spawn(&mut self, fc_bin: &Path) -> anyhow::Result<()> {
        let _ = std::fs::remove_file(&self.api_socket);

        let child = Command::new(fc_bin)
            .args(["--api-sock", self.api_socket.to_str().unwrap()])
            .spawn()?;
        self.process = Some(child);

        // Wait for socket to appear
        for _ in 0..50 {
            if self.api_socket.exists() {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        anyhow::bail!("Firecracker socket did not appear within 1s")
    }

    /// Send a PUT/PATCH request to the Firecracker API.
    async fn api_put(&self, path: &str, body: &str) -> anyhow::Result<String> {
        let mut stream = UnixStream::connect(&self.api_socket).await?;

        let request = format!(
            "PUT {} HTTP/1.1\r\n\
             Host: localhost\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            path,
            body.len(),
            body
        );
        stream.write_all(request.as_bytes()).await?;

        let mut response = vec![0u8; 4096];
        let n = stream.read(&mut response).await?;
        let response_str = String::from_utf8_lossy(&response[..n]);

        // Check for 2xx status
        if !response_str.starts_with("HTTP/1.1 2") {
            anyhow::bail!("API error: {}", response_str);
        }

        Ok(response_str.to_string())
    }

    /// Configure and start the VM.
    pub async fn start(
        &self,
        kernel: &Path,
        rootfs: &Path,
        vcpus: u8,
        mem_mib: u32,
    ) -> anyhow::Result<()> {
        // Boot source
        let boot = serde_json::json!({
            "kernel_image_path": kernel.to_str().unwrap(),
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off \
                          init=/sbin/neovex-init root=/dev/vda rw"
        });
        self.api_put("/boot-source", &boot.to_string()).await?;

        // Rootfs drive
        let drive = serde_json::json!({
            "drive_id": "rootfs",
            "path_on_host": rootfs.to_str().unwrap(),
            "is_root_device": true,
            "is_read_only": false
        });
        self.api_put("/drives/rootfs", &drive.to_string()).await?;

        // Vsock
        let vsock = serde_json::json!({
            "guest_cid": self.guest_cid,
            "uds_path": self.vsock_uds.to_str().unwrap()
        });
        self.api_put("/vsock", &vsock.to_string()).await?;

        // Machine config
        let machine = serde_json::json!({
            "vcpu_count": vcpus,
            "mem_size_mib": mem_mib,
            "smt": false
        });
        self.api_put("/machine-config", &machine.to_string()).await?;

        // Start
        self.api_put("/actions", r#"{"action_type":"InstanceStart"}"#).await?;

        Ok(())
    }

    /// Connect to a port on the guest via Firecracker's vsock UDS proxy.
    pub async fn vsock_connect(&self, port: u32) -> anyhow::Result<UnixStream> {
        let mut stream = UnixStream::connect(&self.vsock_uds).await?;
        let cmd = format!("CONNECT {}\n", port);
        stream.write_all(cmd.as_bytes()).await?;

        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf).await?;
        let resp = std::str::from_utf8(&buf[..n])?;
        if !resp.starts_with("OK") {
            anyhow::bail!("vsock CONNECT failed: {}", resp.trim());
        }
        Ok(stream)
    }

    /// Take a full snapshot.
    pub async fn snapshot(&self, snap_path: &Path, mem_path: &Path) -> anyhow::Result<()> {
        // Pause first
        // Note: PATCH requires different HTTP formatting; simplified here
        self.api_put("/vm", r#"{"state":"Paused"}"#).await?;

        let snap = serde_json::json!({
            "snapshot_type": "Full",
            "snapshot_path": snap_path.to_str().unwrap(),
            "mem_file_path": mem_path.to_str().unwrap()
        });
        self.api_put("/snapshot/create", &snap.to_string()).await?;
        Ok(())
    }

    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.process {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(&self.api_socket);
        let _ = std::fs::remove_file(&self.vsock_uds);
    }
}

impl Drop for FirecrackerVm {
    fn drop(&mut self) { self.kill(); }
}
```

---

## 2. OCI Image to ext4 Rootfs

```rust
// crates/neovex-vmm/src/oci.rs

use oci_client::{Client, Reference, secrets::RegistryAuth};
use std::path::Path;

pub struct OciEntrypoint {
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
}

/// Pull an OCI image and create an ext4 rootfs with injected init.
pub async fn oci_to_rootfs(
    image_ref: &str,
    output: &Path,
    init_binary: &Path,
    size_mib: u64,
) -> anyhow::Result<OciEntrypoint> {
    let reference: Reference = image_ref.parse()?;
    let mut client = Client::new(ClientConfig::default());
    let auth = RegistryAuth::Anonymous;

    // Pull manifest
    let (manifest, _digest) = client.pull_image_manifest(&reference, &auth).await?;

    // Pull config for entrypoint/cmd/env
    let config_blob = client.pull_blob(&reference, &manifest.config.digest, &auth).await?;
    let image_config: oci_spec::image::ImageConfiguration =
        serde_json::from_slice(&config_blob)?;

    // Stage directory
    let staging = tempfile::tempdir()?;
    let rootfs_dir = staging.path().join("rootfs");
    std::fs::create_dir_all(&rootfs_dir)?;

    // Extract layers in order
    for layer in &manifest.layers {
        let data = client.pull_blob(&reference, &layer.digest, &auth).await?;
        let decoder = flate2::read::GzDecoder::new(&data[..]);
        let mut archive = tar::Archive::new(decoder);
        extract_layer_with_whiteouts(&mut archive, &rootfs_dir)?;
    }

    // Inject init
    let init_dest = rootfs_dir.join("sbin/neovex-init");
    std::fs::create_dir_all(rootfs_dir.join("sbin"))?;
    std::fs::copy(init_binary, &init_dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&init_dest, std::fs::Permissions::from_mode(0o755))?;
    }

    // Create ext4 image using mkfs.ext4 -d (populate from directory)
    let f = std::fs::File::create(output)?;
    f.set_len(size_mib * 1024 * 1024)?;
    drop(f);

    // Note: mkfs.ext4 is at /sbin/mkfs.ext4 on Debian, may not be in PATH
    let status = std::process::Command::new("/sbin/mkfs.ext4")
        .args(["-F", "-d", rootfs_dir.to_str().unwrap(), output.to_str().unwrap()])
        .status()?;
    if !status.success() {
        anyhow::bail!("mkfs.ext4 failed");
    }

    // Extract entrypoint from image config
    let config = image_config.config();
    Ok(OciEntrypoint {
        entrypoint: config.as_ref()
            .and_then(|c| c.entrypoint().clone()).unwrap_or_default(),
        cmd: config.as_ref()
            .and_then(|c| c.cmd().clone()).unwrap_or_default(),
        env: config.as_ref()
            .and_then(|c| c.env().clone()).unwrap_or_default(),
        working_dir: config.as_ref()
            .and_then(|c| c.working_dir().clone()).unwrap_or_else(|| "/".into()),
    })
}

fn extract_layer_with_whiteouts(
    archive: &mut tar::Archive<impl std::io::Read>,
    dest: &Path,
) -> anyhow::Result<()> {
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if name == ".wh..wh..opq" {
            // Opaque whiteout: clear parent directory
            let parent = dest.join(path.parent().unwrap_or(Path::new("")));
            if parent.exists() {
                for child in std::fs::read_dir(&parent)? {
                    let _ = std::fs::remove_dir_all(child?.path());
                }
            }
        } else if name.starts_with(".wh.") {
            // Delete the named file
            let target = dest.join(path.parent().unwrap_or(Path::new(""))).join(&name[4..]);
            let _ = std::fs::remove_dir_all(&target);
        } else {
            entry.unpack_in(dest)?;
        }
    }
    Ok(())
}
```

---

## 3. Guest Init (neovex-init)

Compiled as `x86_64-unknown-linux-musl` static binary.

```rust
// crates/neovex-init/src/main.rs

use nix::mount::{mount, MsFlags};
use nix::unistd::{execvp, fork, ForkResult, sethostname, chdir, Pid};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use std::ffi::CString;
use std::fs;

mod vsock_api;  // JSON-RPC server over vsock

const CONFIG_PORT: u32 = 10001;
const API_PORT: u32 = 10000;

fn main() {
    eprintln!("[neovex-init] PID 1 starting");

    mount_filesystems().expect("mount failed");
    setup_dev_symlinks();

    // Read config from host via vsock
    let config = vsock_api::read_config(CONFIG_PORT)
        .expect("failed to read config from host");

    // Networking
    setup_networking(&config);
    sethostname(config.hostname.as_deref().unwrap_or("neovex-vm")).ok();

    // Start vsock API server (background thread)
    std::thread::spawn(move || vsock_api::serve(API_PORT));

    // Set env vars
    for var in &config.env {
        if let Some((k, v)) = var.split_once('=') {
            std::env::set_var(k, v);
        }
    }
    if !config.working_dir.is_empty() {
        let _ = chdir(config.working_dir.as_str());
    }

    // Build command: entrypoint + cmd
    let mut args = config.entrypoint.clone();
    args.extend(config.cmd.clone());
    if args.is_empty() {
        eprintln!("[neovex-init] No entrypoint, sleeping");
        loop { std::thread::sleep(std::time::Duration::from_secs(3600)); }
    }

    eprintln!("[neovex-init] exec: {:?}", args);

    // Fork: PID 1 reaps children, child execs user process
    match unsafe { fork() }.expect("fork failed") {
        ForkResult::Parent { child } => reap_loop(child),
        ForkResult::Child => {
            let prog = CString::new(args[0].as_str()).unwrap();
            let c_args: Vec<CString> = args.iter()
                .map(|a| CString::new(a.as_str()).unwrap()).collect();
            execvp(&prog, &c_args).expect("execvp failed");
        }
    }
}

fn mount_filesystems() -> anyhow::Result<()> {
    let none: Option<&str> = None;
    let noexec = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC;

    for (src, tgt, fstype, flags) in [
        ("proc",     "/proc",     "proc",      noexec),
        ("sysfs",    "/sys",      "sysfs",     noexec),
        ("devtmpfs", "/dev",      "devtmpfs",  MsFlags::MS_NOSUID),
        ("devpts",   "/dev/pts",  "devpts",    noexec),
        ("tmpfs",    "/dev/shm",  "tmpfs",     MsFlags::MS_NOSUID | MsFlags::MS_NODEV),
        ("tmpfs",    "/tmp",      "tmpfs",     MsFlags::MS_NOSUID | MsFlags::MS_NODEV),
        ("tmpfs",    "/run",      "tmpfs",     MsFlags::MS_NOSUID | MsFlags::MS_NODEV),
    ] {
        fs::create_dir_all(tgt)?;
        mount(Some(src), tgt, Some(fstype), flags, none)?;
    }
    Ok(())
}

fn setup_dev_symlinks() {
    let _ = std::os::unix::fs::symlink("/proc/self/fd", "/dev/fd");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/0", "/dev/stdin");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/1", "/dev/stdout");
    let _ = std::os::unix::fs::symlink("/proc/self/fd/2", "/dev/stderr");
}

fn setup_networking(config: &VmConfig) {
    let run = |args: &[&str]| { std::process::Command::new("ip").args(args).status().ok(); };
    run(&["link", "set", "lo", "up"]);
    if let Some(ref ip) = config.ip_address {
        run(&["addr", "add", ip, "dev", "eth0"]);
        run(&["link", "set", "eth0", "up"]);
    }
    if let Some(ref gw) = config.gateway {
        run(&["route", "add", "default", "via", gw]);
    }
    if let Some(ref dns) = config.dns {
        let _ = fs::write("/etc/resolv.conf", format!("nameserver {}\n", dns));
    }
}

fn reap_loop(main_child: Pid) {
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::__WALL)) {
            Ok(WaitStatus::Exited(pid, status)) if pid == main_child => {
                std::process::exit(status);
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) if pid == main_child => {
                std::process::exit(128 + sig as i32);
            }
            Ok(_) => {}
            Err(nix::errno::Errno::ECHILD) => std::process::exit(0),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

#[derive(serde::Deserialize)]
struct VmConfig {
    entrypoint: Vec<String>,
    cmd: Vec<String>,
    env: Vec<String>,
    working_dir: String,
    ip_address: Option<String>,
    gateway: Option<String>,
    dns: Option<String>,
    hostname: Option<String>,
}
```

---

## 4. Host-Side Vsock Communication

```rust
// crates/neovex-vmm/src/vsock.rs

use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Send a length-prefixed JSON message.
pub async fn send_msg(stream: &mut UnixStream, msg: &[u8]) -> anyhow::Result<()> {
    stream.write_all(&(msg.len() as u32).to_be_bytes()).await?;
    stream.write_all(msg).await?;
    Ok(())
}

/// Receive a length-prefixed JSON message.
pub async fn recv_msg(stream: &mut UnixStream) -> anyhow::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Send VM config to the guest init after boot.
pub async fn send_config(
    vm: &super::firecracker::FirecrackerVm,
    config: &super::oci::OciEntrypoint,
) -> anyhow::Result<()> {
    let mut stream = vm.vsock_connect(10001).await?;

    let cfg = serde_json::json!({
        "entrypoint": config.entrypoint,
        "cmd": config.cmd,
        "env": config.env,
        "working_dir": config.working_dir,
        "hostname": "neovex-agent",
    });

    stream.write_all(serde_json::to_vec(&cfg)?.as_slice()).await?;
    stream.shutdown().await?;
    Ok(())
}

/// JSON-RPC call to the guest agent.
pub async fn rpc_call(
    vm: &super::firecracker::FirecrackerVm,
    method: &str,
    params: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let mut stream = vm.vsock_connect(10000).await?;

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    send_msg(&mut stream, &serde_json::to_vec(&request)?).await?;
    let response = recv_msg(&mut stream).await?;
    Ok(serde_json::from_slice(&response)?)
}
```

---

## 5. Custom Minimal VMM (Phase 2 — rust-vmm crates)

```rust
// crates/neovex-vmm/src/vmm.rs — sketch of the in-process VMM

use kvm_ioctls::{Kvm, VmFd, VcpuFd, VcpuExit};
use kvm_bindings::kvm_userspace_memory_region;
use vm_memory::{GuestMemoryMmap, GuestAddress, Bytes, GuestMemory};
use linux_loader::loader::bzimage::BzImage;
use linux_loader::loader::KernelLoader;

pub struct MicroVm {
    vm_fd: VmFd,
    vcpu_fds: Vec<VcpuFd>,
    guest_memory: GuestMemoryMmap,
}

impl MicroVm {
    pub fn create(
        kernel_path: &str,
        mem_size_mib: u64,
    ) -> anyhow::Result<Self> {
        let kvm = Kvm::new()?;
        let vm_fd = kvm.create_vm()?;

        // Guest memory
        let mem_size = mem_size_mib * 1024 * 1024;
        let guest_memory = GuestMemoryMmap::from_ranges(
            &[(GuestAddress(0), mem_size as usize)]
        )?;

        let host_addr = guest_memory.find_region(GuestAddress(0)).unwrap().as_ptr();
        let region = kvm_userspace_memory_region {
            slot: 0, guest_phys_addr: 0, memory_size: mem_size,
            userspace_addr: host_addr as u64, flags: 0,
        };
        unsafe { vm_fd.set_user_memory_region(region)?; }

        // IRQ chip + PIT
        vm_fd.create_irq_chip()?;
        vm_fd.create_pit2(kvm_bindings::kvm_pit_config::default())?;

        // Load kernel
        let mut kernel_file = std::fs::File::open(kernel_path)?;
        let _kernel_load = BzImage::load(
            &guest_memory, None, &mut kernel_file, None,
        )?;

        // vCPU setup (registers, long mode) — omitted for brevity
        let vcpu_fd = vm_fd.create_vcpu(0)?;
        // ... setup_long_mode(), set_regs(), set_sregs() ...

        Ok(Self {
            vm_fd,
            vcpu_fds: vec![vcpu_fd],
            guest_memory,
        })
    }

    /// Run vCPU on a dedicated thread. Returns a channel for control.
    pub fn run(self) -> tokio::sync::mpsc::Sender<VmCommand> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        std::thread::spawn(move || {
            let vcpu = &self.vcpu_fds[0];
            loop {
                match vcpu.run() {
                    Ok(VcpuExit::MmioRead(addr, data)) => {
                        // Dispatch to virtio MMIO device
                    }
                    Ok(VcpuExit::MmioWrite(addr, data)) => {
                        // Dispatch to virtio MMIO device
                    }
                    Ok(VcpuExit::Hlt | VcpuExit::Shutdown) => break,
                    Ok(exit) => { /* handle other exits */ }
                    Err(e) => { eprintln!("vcpu error: {}", e); break; }
                }
            }
        });

        tx
    }
}

pub enum VmCommand {
    Shutdown,
    Status(tokio::sync::oneshot::Sender<VmStatus>),
}

pub struct VmStatus {
    pub running: bool,
}
```

---

## 6. Neovex Integration: WorkerLoopFactory

How the Firecracker runtime plugs into neovex's existing trait system.

```rust
// crates/neovex-vmm/src/lib.rs — integration with neovex-runtime traits

/// A VM-backed worker loop. Each instance wraps a running Firecracker VM
/// and communicates with the guest agent over vsock.
pub struct VmWorkerLoop {
    vm: FirecrackerVm,
    // Vsock connection to the guest's JSON-RPC API
}

/// Factory that creates and caches VM instances.
pub struct VmWorkerLoopFactory {
    /// Pre-warmed snapshot cache
    snapshot_cache: SnapshotCache,
    /// Path to Firecracker binary
    fc_bin: PathBuf,
    /// Path to kernel
    kernel: PathBuf,
    /// Path to init binary
    init_bin: PathBuf,
    /// Working directory for VM state
    work_dir: PathBuf,
    /// Next CID to assign
    next_cid: AtomicU32,
}

impl VmWorkerLoopFactory {
    /// Create a VM for a given OCI image, using cached snapshot if available.
    pub async fn create(&self, image_ref: &str) -> anyhow::Result<VmWorkerLoop> {
        // 1. Get or create rootfs
        let rootfs = self.ensure_rootfs(image_ref).await?;

        // 2. Get or create snapshot template
        let template = self.snapshot_cache
            .get_or_create(image_ref, &rootfs, &self.kernel)
            .await?;

        // 3. Restore from snapshot (fast path: ~10ms)
        let cid = self.next_cid.fetch_add(1, Ordering::Relaxed);
        let vm = restore_from_snapshot(&template, &self.work_dir, cid).await?;

        // 4. Send per-instance config over vsock
        send_config(&vm, &entrypoint).await?;

        Ok(VmWorkerLoop { vm })
    }
}
```

---

## Dependencies Summary

### neovex-vmm (host crate) — verified 2026-04-09

```toml
[dependencies]
# Existing workspace deps
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
ulid = { workspace = true }

# New deps
anyhow = "1"
flate2 = "1"
tar = "0.4"
tempfile = "3"

# OCI image handling
oci-client = "0.16"       # 2.9M downloads, actively maintained (oras-project)
oci-spec = "0.9"          # 12.3M downloads (containers org)
# NOTE: Do NOT use oci-distribution — deprecated, use oci-client

# Vsock (host side uses Unix sockets via tokio, not AF_VSOCK)
# tokio-vsock = "0.7"     # only if using AF_VSOCK directly (not needed with FC)
vsock = "0.5"             # 6.1M downloads, for reference/testing

# Phase 2 only (custom VMM from rust-vmm crates)
kvm-ioctls = "0.24"       # 3.7M downloads
kvm-bindings = { version = "0.14", features = ["fam-wrappers"] }  # 2.5M
vm-memory = { version = "0.18", features = ["backend-mmap"] }     # 4.4M
linux-loader = { version = "0.13", features = ["bzimage", "elf"] } # 2.9M
virtio-queue = "0.17"     # 1.5M
vm-superio = "0.8"        # 2.4M
vmm-sys-util = "0.15"     # 7.7M
event-manager = "0.4"     # 2.5M
```

### neovex-init (guest binary)

```toml
[dependencies]
nix = { version = "0.28", features = ["mount", "signal", "process", "hostname"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
libc = "0.2"

[profile.release]
opt-level = "z"   # optimize for size
lto = true
strip = true
panic = "abort"
```

Build target: `x86_64-unknown-linux-musl` (static, no glibc dependency).
