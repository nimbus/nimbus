# Plan: VMM Infrastructure — Static Single-Binary MicroVM Support

Canonical design and execution plan for building the VMM infrastructure that
enables neovex to run OCI/Docker images in hardware-isolated microVMs as a
single statically-linked binary. Covers forking libkrun and crun, static
build system integration, kernel embedding, FFI bindings, and the
`--internal-vmm` re-exec entry point.

This plan produces the VMM foundation that `microvm-runtime-plan.md` builds on.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when microVM runtime work begins
- **Relationship:** This plan produces the VMM layer. `microvm-runtime-plan.md`
  builds the OCI management, lifecycle probes, engine integration, and DX on
  top of it.

## Control Plan Rules

Source of truth:
1. the current git worktrees (neovex, agentstation/libkrun, agentstation/crun)
2. this plan's `Phase Status Ledger` and `Execution Log`
3. `docs/research/libkrun-evaluation.md`
4. `docs/research/firecracker-container-runtime.md`
5. `docs/research/vm-lifecycle-probes.md`

### Status model

- `todo` / `in_progress` / `blocked` / `done` / `deferred`

---

## Architecture

### Single binary, re-exec pattern

```
neovex binary (~100MB, single file)
  ├── neovex Rust code (server, engine, V8 bridge)
  ├── V8 engine (C++, statically linked — existing)
  ├── libcrun.a (C, statically linked via FFI — new)
  │     ├── libseccomp.a (C, static)
  │     ├── libcap.a (C, static)
  │     └── libyajl.a (C, static)
  ├── libkrun (Rust crate, statically linked via Cargo — new)
  │     └── kernel + qboot + initrd (include_bytes! ~20MB)
  └── neovex-init (include_bytes! ~2MB, injected into rootfs at runtime)
```

Two execution modes:

```
neovex serve               →  database server (tokio + V8 + engine)
neovex --internal-vmm      →  OCI runtime + VMM (re-exec'd child, _exit safe)
```

When neovex needs a VM:
```rust
let child = Command::new(std::env::current_exe()?)
    .arg("--internal-vmm")
    .arg("--bundle").arg(&bundle_path)
    .arg("--id").arg(&container_id)
    .spawn()?;
```

The child process calls libcrun → libkrun → `krun_start_enter()` →
`_exit()`. Only the child dies. Zero resource leaks.

### Why re-exec?

`krun_start_enter()` calls `libc::_exit()` on VM shutdown
(`src/vmm/src/lib.rs:370` in libkrun). This kills the entire process. The
re-exec pattern isolates this in a child process. This is the same pattern
crun/Podman uses (crun is fork'd by conmon, `_exit()` kills only the fork'd
child).

### Why static linking?

Single binary deployment is a core product value. neovex already statically
links V8 (~50MB of C++). Adding libcrun (~500KB) + libkrun (~2MB) + kernel
(~20MB) is proportional. No system packages, no LD_LIBRARY_PATH, no
extraction. Just `neovex` + `/dev/kvm`.

---

## Forks Required

### Fork 1: `agentstation/libkrun`

**Upstream:** `containers/libkrun` (v1.17.4, Apache 2.0)
**Purpose:** Embed guest kernel (eliminate dlopen of libkrunfw)
**Patch size:** ~30 lines changed in one file

#### What to change

**File:** `src/libkrun/src/lib.rs`

The upstream code dynamically loads libkrunfw.so via the `libloading` crate:

```rust
// UPSTREAM (src/libkrun/src/lib.rs:90-112)
static KRUNFW: LazyLock<Option<libloading::Library>> =
    LazyLock::new(|| unsafe { libloading::Library::new(KRUNFW_NAME).ok() });

pub struct KrunfwBindings {
    get_kernel: libloading::Symbol<
        'static,
        unsafe extern "C" fn(*mut u64, *mut u64, *mut size_t) -> *mut c_char,
    >,
    // ...
}

impl KrunfwBindings {
    fn load_bindings() -> Result<KrunfwBindings, libloading::Error> {
        let krunfw = match KRUNFW.as_ref() {
            Some(krunfw) => krunfw,
            None => return Err(libloading::Error::DlOpenUnknown),
        };
        // ...
    }
}
```

**Replace with embedded kernel bytes:**

```rust
// FORK: Embed kernel artifacts from libkrunfw build
static KERNEL_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vmlinux"));

#[cfg(feature = "tee")]
static INITRD_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/initrd"));

#[cfg(feature = "tee")]
static QBOOT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/qboot.rom"));

pub struct KrunfwBindings;

impl KrunfwBindings {
    fn load_bindings() -> Result<KrunfwBindings, &'static str> {
        Ok(KrunfwBindings)
    }

    pub fn get_kernel(&self) -> (*const u8, u64) {
        (KERNEL_BYTES.as_ptr(), KERNEL_BYTES.len() as u64)
    }
    // ...
}
```

**File:** `src/libkrun/build.rs` (new or modified)

**Important:** libkrunfw releases publish only `.so` files (e.g.,
`libkrunfw-x86_64.tgz` containing `lib64/libkrunfw.so.5.3.0`), NOT raw
kernel binaries. The kernel bytes are embedded inside the `.so` as a C char
array, accessible only via `krunfw_get_kernel()`.

**Two approaches to get the raw kernel:**

**Approach A (recommended): Build libkrunfw from source in CI**
```bash
# In CI (not build.rs — too slow for developer builds):
git clone --depth 1 https://github.com/containers/libkrunfw
cd libkrunfw
make vmlinux  # produces the raw kernel binary before .so packaging
cp vmlinux ${NEOVEX_KERNEL_DIR}/vmlinux
# Also extract qboot.rom and initrd if needed for TEE variants
```
Cache the built kernel artifact in CI. build.rs uses it via `include_bytes!`.

**Approach B: Extract kernel from the .so at build time**
```rust
fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    // Download the .so from releases
    let tgz_url = "https://github.com/containers/libkrunfw/releases/\
                    download/v5.3.0/libkrunfw-x86_64.tgz";
    download_and_extract(tgz_url, &out_dir);
    // Load the .so and call krunfw_get_kernel() to get the raw bytes
    let lib = libloading::Library::new(format!("{out_dir}/lib64/libkrunfw.so"))?;
    let get_kernel: Symbol<unsafe extern "C" fn(...)> = lib.get(b"krunfw_get_kernel")?;
    // Extract and write to file for include_bytes!
}
```
This is complex and fragile. Approach A is preferred.

**build.rs (assuming CI has placed kernel at a known path):**
```rust
fn main() {
    let kernel_path = std::env::var("NEOVEX_KERNEL_PATH")
        .unwrap_or_else(|_| "build/vmlinux".into());
    if !std::path::Path::new(&kernel_path).exists() {
        panic!("Kernel not found at {}. Build libkrunfw first or set \
                NEOVEX_KERNEL_PATH", kernel_path);
    }
    // The actual include_bytes! is in lib.rs, not build.rs
}
```

**Also remove:** `libloading` from `Cargo.toml` dependencies (no longer needed).

#### Reference files in upstream

| File | Purpose | Lines to examine |
|------|---------|-----------------|
| `src/libkrun/src/lib.rs:88-150` | KrunfwBindings, dlopen logic | Replace with include_bytes |
| `src/libkrun/src/lib.rs:2149-2175` | `load_kernel()` function that calls krunfw | Adapt to use embedded bytes |
| `src/libkrun/Cargo.toml:25` | `libloading = "0.8"` | Remove |
| `src/libkrun/Cargo.toml:47-49` | `[lib]` section, crate-type | Ensure `"lib"` is present (already is since PR #588) |

#### Build verification

```bash
cd agentstation/libkrun
# Should build without libkrunfw.so on the system
cargo build --release -p libkrun
# Verify no libkrunfw dependency
ldd target/release/libkrun.so  # should NOT show libkrunfw
```

---

### Fork 2: `agentstation/crun`

**Upstream:** `containers/crun` (latest release, GPL-2.0 with LGPL-2.1 for libcrun)
**Purpose:** Static linkage to libkrun (eliminate dlopen) + vsock/TSI support
**Patch size:** ~100 lines changed in one file

#### Important: License

crun is **GPL-2.0**. libcrun (the library portion) is **LGPL-2.1**. Neovex
links against libcrun.a (the library), which is LGPL-2.1. Under LGPL-2.1,
static linking is permitted if neovex provides a mechanism for users to
relink with a modified libcrun (e.g., providing object files or using
dynamic linking). **Review with legal counsel before finalizing.**

Alternatively, neovex can dynamically link libcrun.so (extracting it at
runtime), which has no LGPL relinking requirement. This conflicts with the
single-binary goal but avoids license issues.

**If the LGPL is problematic:** Reimplement crun's OCI runtime setup in Rust
(~200 lines using `oci-spec`, `nix`, `seccompiler` crates) and skip the crun
fork entirely. This is the fallback path described at the end of this plan.

#### What to change

**File:** `src/libcrun/handlers/krun.c`

**Change 1: Replace dlopen/dlsym with direct static declarations (~80 lines removed, ~30 added)**

```c
// UPSTREAM: ~80 lines of dlopen + dlsym
// handler->private_data = dlopen("libkrun.so.1", RTLD_NOW);
// krun_set_vm_config = dlsym(handle, "krun_set_vm_config");
// ... 20 more dlsym calls ...

// FORK: Direct declarations (symbols resolved at link time from libkrun.a)
// These are the extern "C" functions exported by libkrun's Rust code
extern int32_t krun_create_ctx(void);
extern int32_t krun_free_ctx(uint32_t ctx_id);
extern int32_t krun_set_log_level(uint32_t level);
extern int32_t krun_set_vm_config(uint32_t ctx_id, uint8_t num_vcpus,
                                   uint32_t ram_mib);
extern int32_t krun_set_root(uint32_t ctx_id, const char *root_path);
extern int32_t krun_set_workdir(uint32_t ctx_id, const char *workdir);
extern int32_t krun_set_env(uint32_t ctx_id, const char *env);
extern int32_t krun_set_gpu_options(uint32_t ctx_id, uint32_t virgl_flags);
extern int32_t krun_set_kernel(uint32_t ctx_id, const char *path);
extern int32_t krun_start_enter(uint32_t ctx_id);
// vsock: maps a guest vsock port to a host Unix socket path
extern int32_t krun_add_vsock_port(uint32_t ctx_id, uint32_t port,
                                    const char *c_filepath);
// vsock2: same but with listen flag (host initiates connection)
extern int32_t krun_add_vsock_port2(uint32_t ctx_id, uint32_t port,
                                     const char *c_filepath, bool listen);
extern int32_t krun_set_port_map(uint32_t ctx_id, const char *port_map);
```

**Change 2: Remove dlopen loading in `libkrun_load()`**

```c
// UPSTREAM:
static int libkrun_load(void **cookie, ...) {
    *cookie = dlopen("libkrun.so.1", RTLD_NOW);
    // ...
}

// FORK:
static int libkrun_load(void **cookie, ...) {
    *cookie = (void *)1; // no-op, symbols are statically linked
    return 0;
}
```

**Change 3: Add vsock port configuration (~25 lines)**

**Important:** `krun_add_vsock_port(ctx_id, port, filepath)` maps a guest
vsock port to a **host-side Unix socket path**. The host neovex process
connects to the guest by connecting to this UDS path. This is how the host
sends shutdown commands and health checks to the guest init.

```c
// Add after krun_set_vm_config() call in libkrun_exec():

// Read vsock port mappings from OCI annotation
// Format: "10000:/run/neovex/vm-abc/vsock-10000.sock"
// Multiple ports: "10000:/path/a.sock,10001:/path/b.sock"
const char *vsock_annotation = find_annotation(def, "krun.neovex.vsock.ports");
if (vsock_annotation) {
    char *ports = strdup(vsock_annotation);
    char *saveptr;
    char *token = strtok_r(ports, ",", &saveptr);
    while (token) {
        uint32_t port;
        char filepath[PATH_MAX];
        if (sscanf(token, "%u:%s", &port, filepath) == 2) {
            // listen=true: guest listens, host connects to this UDS
            krun_add_vsock_port2(ctx_id, port, filepath, true);
        }
        token = strtok_r(NULL, ",", &saveptr);
    }
    free(ports);
}
```

**Change 4: Add TSI port mapping from ExposedPorts (~15 lines)**

```c
// Read TSI port map from OCI annotation
const char *tsi_annotation = find_annotation(def, "krun.neovex.tsi.port_map");
if (tsi_annotation) {
    // Format: "5432:15432,6379:16379" (guest:host)
    krun_set_port_map(ctx_id, tsi_annotation);
}
```

#### Reference files in upstream

| File | Purpose | Lines to examine |
|------|---------|-----------------|
| `src/libcrun/handlers/krun.c` | Entire krun handler | All — this is the only file to patch |
| `src/libcrun/container.h` | libcrun public API | Reference for FFI bindings |
| `src/libcrun/container.c:1850-1900` | `libcrun_container_run()` implementation | Understand the lifecycle |
| `src/libcrun/linux.c:4000+` | Namespace/cgroup/seccomp setup | Understanding (don't modify) |
| `configure.ac:85-120` | Library detection (libseccomp, libcap, libyajl) | Build system reference |
| `Makefile.am` | Build targets for libcrun.a | May need modification for static build |

#### Build verification

```bash
cd agentstation/crun
./autogen.sh
./configure --enable-static --disable-shared \
    --with-libkrun=static  # new flag from our fork
make libcrun.a
# Verify symbols
nm libcrun.a | grep libcrun_container_run  # should exist
nm libcrun.a | grep dlopen                  # should NOT exist (removed)
```

---

## Neovex Build System Integration

### build.rs

**Important build notes:**
- crun uses autotools (`configure.ac` + `Makefile.am`). Nearly every `.c`
  file includes `<config.h>` which is generated by `./configure`. You
  **cannot** compile crun source files directly with the `cc` crate — you
  must use its native autotools build system.
- neovex-init is a separate workspace member. It **cannot** be built inside
  another crate's build.rs (circular dependency). It must be built as a
  separate `cargo build` step (in CI or via a `make` target) and its output
  binary placed where build.rs can `include_bytes!` it.

```rust
// crates/neovex-vmm/build.rs

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // ── Step 1: Build libcrun.a via autotools ──────────────────────
    // crun REQUIRES autotools (./configure generates config.h with
    // HAVE_* defines that every source file depends on).
    // Cannot use the cc crate for this.

    let crun_dir = PathBuf::from("vendor/crun");

    // Run autogen + configure + make (only if libcrun.a doesn't exist)
    let libcrun_a = crun_dir.join(".libs/libcrun.a");
    if !libcrun_a.exists() {
        // autogen.sh generates the configure script
        let status = Command::new("sh")
            .arg("-c")
            .arg("cd vendor/crun && ./autogen.sh && \
                  ./configure --disable-shared --enable-static \
                  CFLAGS='-DHAVE_LIBKRUN=1 -DLIBKRUN_STATIC=1' && \
                  make -j$(nproc) libcrun.la")
            .status()
            .expect("Failed to build libcrun. Ensure autotools is installed: \
                     apt install autoconf automake libtool");
        assert!(status.success(), "libcrun build failed");
    }

    // Link the static library
    println!("cargo:rustc-link-search=native={}", crun_dir.join(".libs").display());
    println!("cargo:rustc-link-lib=static=crun");

    // Link crun's dependencies (system static libraries)
    println!("cargo:rustc-link-lib=static=seccomp");
    println!("cargo:rustc-link-lib=static=cap");
    println!("cargo:rustc-link-lib=static=yajl");

    // ── Step 2: Generate Rust FFI bindings for libcrun ──────────────
    let bindings = bindgen::Builder::default()
        .header("vendor/crun/src/libcrun/container.h")
        .header("vendor/crun/src/libcrun/custom-handler.h")
        .allowlist_function("libcrun_container_run")
        .allowlist_function("libcrun_container_load_from_file")
        .allowlist_function("libcrun_configure_handler")
        .allowlist_type("libcrun_context_s")
        .allowlist_type("libcrun_error_t")
        .generate()
        .expect("Failed to generate libcrun bindings");

    bindings
        .write_to_file(out_dir.join("libcrun_bindings.rs"))
        .expect("Failed to write libcrun bindings");

    // ── Step 3: Embed neovex-init binary ───────────────────────────
    // neovex-init is built SEPARATELY (not in this build.rs) via:
    //   cargo build --release --target x86_64-unknown-linux-musl -p neovex-init
    // The binary is placed at a known path by CI or the Makefile.
    // build.rs just verifies it exists.
    let init_path = PathBuf::from(
        env::var("NEOVEX_INIT_BINARY")
            .unwrap_or_else(|_| "target/x86_64-unknown-linux-musl/release/neovex-init".into())
    );
    if !init_path.exists() {
        panic!(
            "neovex-init binary not found at {}. Build it first:\n\
             cargo build --release --target x86_64-unknown-linux-musl -p neovex-init",
            init_path.display()
        );
    }

    // ── Step 4: libkrun is linked via Cargo ────────────────────────
    // The forked libkrun crate is a Cargo dependency (via [patch.crates-io]).
    // It is linked automatically by cargo. No build.rs step needed.

    println!("cargo:rerun-if-changed=vendor/crun");
}
```

### Cargo.toml additions

```toml
# neovex workspace Cargo.toml — add to [patch.crates-io]
[patch.crates-io]
deno_core = { git = "https://github.com/agentstation/deno_core", tag = "0.395.0-locker.2" }
v8 = { git = "https://github.com/agentstation/rusty_v8", tag = "v147.0.0-locker.2" }
# NEW: forked libkrun with embedded kernel
libkrun = { git = "https://github.com/agentstation/libkrun", tag = "v1.17.4-neovex.1" }
```

```toml
# crates/neovex-vmm/Cargo.toml
[dependencies]
# libkrun publishes as crate name "krun" (see [lib] name = "krun")
# but the package name is "libkrun". Resolved via [patch.crates-io].
libkrun = "1.17"
oci-spec = "0.9"
oci-client = "0.16"
# ... (see microvm-runtime-plan.md for full list)

[build-dependencies]
bindgen = "0.70"
# NOTE: cc crate is NOT used for crun (autotools required).
# build.rs calls make directly.
```

**Crate naming note:** libkrun's `Cargo.toml` has `name = "libkrun"` (package)
and `[lib] name = "krun"` (crate). In Rust code you `use krun::*`, but in
`Cargo.toml` you depend on `libkrun`. The `[patch.crates-io]` entry should
reference the package name `libkrun`.

### Vendor directory structure

```
neovex/
  vendor/
    crun/           # git submodule → agentstation/crun
    yajl/           # git submodule → lloyd/yajl (or vendored source)
```

libcap and libseccomp are linked from system-provided static libraries
(`libcap-dev`, `libseccomp-dev` packages on Debian). These are stable, small,
and available on every Linux distribution. They are **build-time deps only** —
the resulting neovex binary has no runtime dependency on them.

---

## neovex `--internal-vmm` Entry Point

### main.rs integration

```rust
// crates/neovex-bin/src/main.rs
// Add before any tokio/V8 initialization:

fn main() {
    // Check for VMM mode FIRST — before creating tokio runtime or V8
    if std::env::args().any(|a| a == "--internal-vmm") {
        neovex_vmm::vmm_mode::run();
        // unreachable — krun_start_enter calls _exit()
    }

    // Normal neovex server startup
    // ...
}
```

### vmm_mode implementation

```rust
// crates/neovex-vmm/src/vmm_mode.rs

use std::ffi::CString;
use std::os::raw::c_char;

// Generated by bindgen in build.rs
include!(concat!(env!("OUT_DIR"), "/libcrun_bindings.rs"));

/// Entry point for `neovex --internal-vmm --bundle <path> --id <id>`
///
/// This function is called in a re-exec'd child process. It calls libcrun
/// to set up OCI runtime isolation (namespaces, cgroups, seccomp), then
/// libcrun's krun handler calls libkrun to boot the VM.
///
/// krun_start_enter() blocks forever. When the VM exits, it calls _exit().
/// This kills only this child process. The parent neovex server is unaffected.
///
/// # Safety
/// This function never returns. It calls _exit() via krun_start_enter().
pub fn run() -> ! {
    // Ensure this child dies if parent dies
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL); }

    // Parse arguments
    let args: Vec<String> = std::env::args().collect();
    let bundle_path = args.iter()
        .position(|a| a == "--bundle")
        .and_then(|i| args.get(i + 1))
        .expect("--bundle <path> required");
    let container_id = args.iter()
        .position(|a| a == "--id")
        .and_then(|i| args.get(i + 1))
        .expect("--id <name> required");

    // Signal parent: VMM child process started
    println!("READY");

    // Call libcrun to run the OCI container in a krun VM
    let bundle_c = CString::new(bundle_path.as_str()).unwrap();
    let id_c = CString::new(container_id.as_str()).unwrap();

    unsafe {
        let mut err: *mut libcrun_error_t = std::ptr::null_mut();
        let context = libcrun_context_new(&mut err);
        if context.is_null() {
            eprintln!("libcrun_context_new failed");
            std::process::exit(1);
        }

        // Configure context
        (*context).id = id_c.as_ptr() as *mut c_char;
        (*context).bundle = bundle_c.as_ptr() as *mut c_char;

        // CRITICAL: Select the krun handler explicitly.
        // Without this, libcrun uses standard container execution (execv).
        // The handler can be set via:
        //   1. context->handler = "krun"  (programmatic)
        //   2. OCI annotation "run.oci.handler" = "krun" (in config.json)
        // We use both for safety.
        let handler_name = CString::new("krun").unwrap();
        (*context).handler = handler_name.as_ptr() as *mut c_char;

        // Load OCI config from bundle
        let container = libcrun_container_load_from_file(
            bundle_c.as_ptr(),
            &mut err,
        );
        if container.is_null() {
            eprintln!("Failed to load OCI config from bundle");
            std::process::exit(1);
        }

        // Run: libcrun handles namespace/cgroup/seccomp, then krun handler
        // calls libkrun API (including vsock ports from annotations) and
        // enters krun_start_enter() which blocks forever.
        let ret = libcrun_container_run(
            context,
            container,
            0, // flags
            &mut err,
        );
        if ret < 0 {
            eprintln!("libcrun_container_run failed: {}", ret);
            std::process::exit(1);
        }
    }

    // krun_start_enter calls _exit(), so we should never reach here
    unreachable!("krun_start_enter should have called _exit()");
}
```

---

## Phase Plan

### Phase V1: Fork libkrun — Embed Kernel

**Goal:** Create `agentstation/libkrun` that embeds the guest kernel and
does not depend on libkrunfw.so.

**Scope:**
1. Fork `containers/libkrun` at tag v1.17.4
2. Replace `libloading` dlopen of libkrunfw with `include_bytes!` kernel
3. Add `build.rs` that downloads kernel artifacts from libkrunfw releases
4. Remove `libloading` dependency from Cargo.toml
5. Tag as `v1.17.4-neovex.1`

**Key files to modify:**
- `src/libkrun/src/lib.rs` (lines 88-150: KrunfwBindings)
- `src/libkrun/src/lib.rs` (lines 2149-2175: load_kernel)
- `src/libkrun/Cargo.toml` (remove libloading)
- `src/libkrun/build.rs` (new: download kernel artifacts)

**Implementation reference:**
- libkrunfw's `Makefile` shows how kernel is compiled and packaged
- libkrunfw releases at `https://github.com/containers/libkrunfw/releases`
  publish binary artifacts per architecture

**Acceptance criteria:**
- `cargo build -p libkrun` succeeds without libkrunfw.so on system
- `cargo test -p libkrun` passes (if any tests exist)
- Binary includes embedded kernel (check with `strings target/release/libkrun.a | grep "Linux version"`)

### Phase V2: Fork crun — Static Linkage + vsock

**Goal:** Create `agentstation/crun` with static libkrun linkage and
vsock/TSI support in the krun handler.

**Scope:**
1. Fork `containers/crun` at latest release
2. Replace dlopen/dlsym in `handlers/krun.c` with direct extern declarations
3. Add `krun_add_vsock_port()` calls driven by OCI annotation
4. Add `krun_set_port_map()` calls for TSI port mapping
5. Verify libcrun.a builds with static libkrun linkage
6. **Review LGPL-2.1 license implications** for static linking

**Key files to modify:**
- `src/libcrun/handlers/krun.c` (the only file)

**Implementation references:**
- Upstream krun handler: `containers/crun/src/libcrun/handlers/krun.c`
- libkrun public API: `containers/libkrun/include/libkrun.h`
- OCI annotation spec: annotations are arbitrary key-value strings in
  `config.json` under `annotations`

**Acceptance criteria:**
- `make libcrun.a` succeeds
- `nm libcrun.a | grep krun_start_enter` shows the symbol (from libkrun)
- `nm libcrun.a | grep dlopen` shows NO matches (dlopen removed)
- A test program can call `libcrun_container_run()` with a krun-configured
  OCI bundle and boot a VM with vsock ports

### Phase V3: neovex Build System Integration

**Goal:** Integrate forked crun and libkrun into neovex's cargo build.

**Scope:**
1. Add `agentstation/crun` as git submodule under `vendor/crun`
2. Add libyajl source under `vendor/yajl` (or as git submodule)
3. Write `build.rs` that compiles libcrun.a, generates bindgen FFI bindings
4. Add `agentstation/libkrun` as Cargo dependency via `[patch.crates-io]`
5. Ensure `cargo build` compiles everything in one pass
6. Document build prerequisites: `libcap-dev`, `libseccomp-dev` (Debian)

**Build prerequisites (system packages):**
```bash
# Debian/Ubuntu
sudo apt install libcap-dev libseccomp-dev

# Fedora
sudo dnf install libcap-devel libseccomp-devel
```

**Implementation references:**
- neovex's existing build: `Cargo.toml`, `[patch.crates-io]` section
- The `cc` crate: https://docs.rs/cc/latest/cc/
- The `bindgen` crate: https://docs.rs/bindgen/latest/bindgen/
- libyajl source: https://github.com/lloyd/yajl (~2000 lines of C)

**Acceptance criteria:**
- `cargo build -p neovex-bin` succeeds on a clean Debian 13 system
  (with libcap-dev and libseccomp-dev installed)
- The resulting binary has no runtime dependency on libkrun.so,
  libkrunfw.so, or libyajl.so (verify with `ldd`)
- Binary size is documented
- Build time is documented

### Phase V4: `--internal-vmm` Entry Point

**Goal:** Add the re-exec VMM mode to neovex.

**Scope:**
1. `crates/neovex-vmm/src/vmm_mode.rs`: Parse args, call libcrun FFI
2. `crates/neovex-bin/src/main.rs`: Check for `--internal-vmm` before
   tokio/V8 initialization
3. `crates/neovex-vmm/src/vm.rs`: `VmHandle` that spawns child via
   `Command::new(current_exe()).arg("--internal-vmm")`
4. Child process signals `READY` on stdout before entering krun
5. `PR_SET_PDEATHSIG(SIGKILL)` for cleanup

**Implementation references:**
- neovex's existing `crates/neovex-bin/src/main.rs` entry point
- neovex-runtime's test isolation: `crates/neovex-runtime/src/test_support/isolation.rs`
  (uses `Command::new()` for process isolation — same pattern)

**Acceptance criteria:**
- `neovex --internal-vmm --bundle /tmp/test-bundle --id test1` boots a VM
- `neovex serve` still works normally (VMM mode doesn't interfere)
- Parent process spawning `--internal-vmm` child:
  - Reads READY signal
  - Can wait for child exit
  - Gets correct exit code from workload
  - Child dies when parent dies (PR_SET_PDEATHSIG)

### Phase V5: Upstream Contributions

**Goal:** Submit patches upstream to reduce fork maintenance burden.

**Scope:**
1. PR to `containers/libkrun`: Option to use embedded kernel instead of
   dlopen (feature flag, non-breaking)
2. PR to `containers/crun`: vsock port configuration via OCI annotation
   in krun handler
3. PR to `containers/crun`: Static libkrun linkage option

**Acceptance criteria:**
- PRs submitted with tests and documentation
- Maintainer feedback addressed
- If accepted: update neovex to use upstream instead of forks

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| V1: Fork libkrun | `todo` | libkrunfw release artifacts | |
| V2: Fork crun | `todo` | V1 (need libkrun symbols to test static linkage) | LGPL review needed |
| V3: Build system | `todo` | V1, V2 | libcap-dev, libseccomp-dev |
| V4: --internal-vmm | `todo` | V3 | |
| V5: Upstream PRs | `todo` | V1, V2 tested and stable | |

---

## LGPL Fallback: Rust Reimplementation

If LGPL-2.1 static linking is not acceptable for neovex's license, replace
libcrun with a Rust implementation of the OCI runtime setup. This uses only
permissively-licensed crates:

```rust
// ~200 lines total, all Apache-2.0 / MIT crates
use oci_spec::runtime::Spec;          // OCI config.json parsing
use nix::sched::{clone, CloneFlags};  // namespace setup
use nix::unistd::{setuid, setgid};    // user/group
use seccompiler::SeccompFilter;       // seccomp (from rust-vmm, Apache-2.0)

fn setup_isolation(spec: &Spec) {
    // 1. Create namespaces (~20 lines)
    unshare(CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNS | ...);

    // 2. Set up cgroups (~30 lines)
    write_cgroup_limits(&spec.linux.resources);

    // 3. Apply seccomp filter (~30 lines)
    apply_seccomp(&spec.linux.seccomp);

    // 4. Drop capabilities (~10 lines)
    drop_caps();

    // 5. Configure libkrun and enter VM (~50 lines)
    configure_and_start_vm(spec);
}
```

**Crates (all permissive license):**
- `oci-spec` (Apache-2.0) — OCI config parsing
- `nix` (MIT) — namespace, mount, user/group
- `seccompiler` (Apache-2.0) — seccomp BPF filters (from rust-vmm project)
- `caps` (MIT/Apache-2.0) — Linux capabilities

This gives the same runtime behavior as libcrun without the LGPL constraint.
The trade-off: no formal OCI conformance test validation, must track spec
changes manually. But the OCI runtime spec is stable and changes rarely.

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| LGPL static linking is problematic | Medium | High | Fallback to Rust reimplementation (see above) |
| libkrunfw doesn't publish kernel artifacts for all archs | Medium | Medium | Build from source in CI, cache artifacts |
| build.rs compilation of C deps fails on some systems | Medium | Medium | Document prerequisites, provide Dockerfile for reproducible builds |
| crun internal API changes between releases | Low | Medium | Pin to submodule tag, update deliberately |
| Static linking causes symbol conflicts | Low | Medium | Test early in V3, use symbol visibility to isolate |
| Binary size exceeds user expectations | Low | Low | V8 is already 50MB — kernel adds 20MB proportionally |
| bindgen generates incompatible bindings | Low | Medium | Pin bindgen version, test in CI |

---

## Research References

| Document | Contents |
|----------|----------|
| `docs/research/libkrun-evaluation.md` | libkrun API, _exit() analysis, crun/muvm/krunkit consumer patterns |
| `docs/research/firecracker-container-runtime.md` | Approach comparison, Firecracker alternative if libkrun doesn't work |
| `docs/research/firecracker-implementation-sketches.md` | Code sketches for OCI pipeline, guest init, vsock |
| `docs/research/vm-lifecycle-probes.md` | K8s/Docker/Fly.io probe models, graceful shutdown, neovex-init design |

## Source Code References

| File | Repo | What to study |
|------|------|---------------|
| `src/libkrun/src/lib.rs` | containers/libkrun | KrunfwBindings (L88-150), krun_start_enter (L2528), Vmm::stop (L357) |
| `src/vmm/src/lib.rs` | containers/libkrun | _exit() call (L370), exit_evt handling (L397-428) |
| `src/vmm/src/signal_handler.rs` | containers/libkrun | Signal handlers that also call _exit() |
| `src/libcrun/handlers/krun.c` | containers/crun | krun handler — dlopen, VM config, the function to patch |
| `src/libcrun/container.h` | containers/crun | libcrun public API — FFI binding target |
| `src/libcrun/container.c` | containers/crun | libcrun_container_run() implementation |
| `src/libcrun/linux.c` | containers/crun | Namespace/cgroup/seccomp setup (reference, don't modify) |
| `include/libkrun.h` | containers/libkrun | Complete libkrun C API (~55 functions) |
| `init/init.c` | containers/libkrun | Built-in guest init (reference for neovex-init) |
| `src/tini.c` | krallin/tini | PID 1 signal forwarding pattern |
| `dumb-init.c` | Yelp/dumb-init | PID 1 signal forwarding + rewriting |
| `src/agent/src/rpc.rs` | kata-containers/kata-containers | vsock shutdown protocol (ttrpc) |
| `pkg/kubelet/prober/worker.go` | kubernetes/kubernetes | Probe state machine reference |
| `daemon/health.go` | moby/moby | Docker health check state machine |

---

## Execution Log

_Empty — no work started._
