# krun CI Build and Distribution

Research note capturing everything needed to build the neovex patched-crun
stack from a bare Linux host, derived from the first successful `LH1`-`LH6`
validation run on Debian 13 (2026-04-12). The primary audience is a GitHub
Actions runner definition, but the same information applies to any fresh Linux
build environment.

## Validated baseline

| Property | Value |
|----------|-------|
| Distro | Debian GNU/Linux 13 (trixie) |
| Arch | x86_64 |
| Kernel | 6.12.74+deb13+1-amd64 |
| `/dev/kvm` | present, user-accessible |
| Rust | 1.94.1 |
| Cargo | 1.94.1 |
| Python | 3.13 |

## Dependency map

### Apt packages — runtime and container tooling

```bash
sudo apt-get install -y \
  conmon \
  buildah \
  crun \
  podman \
  uidmap \
  passt \
  fuse-overlayfs \
  catatonit
```

Installed versions on the validated host:

| Package | Version |
|---------|---------|
| conmon | 2.1.12-4 |
| buildah | 1.39.3+ds1-1+b7 |
| crun | 1.21-1 |
| podman | 5.4.2+ds1-2+b2 |
| uidmap | 1:4.17.4-2 |
| passt | 0.0~git20250503.587980c-2+deb13u1 |
| fuse-overlayfs | 1.14-1+b1 |
| catatonit | 0.2.1-2+b12 |

### Apt packages — crun build dependencies

Required by `./configure --with-libkrun && make` in the upstream crun source:

```bash
sudo apt-get install -y \
  libyajl-dev \
  libseccomp-dev \
  libcap-dev \
  libsystemd-dev
```

### Apt packages — libkrun build dependencies

Required by `make` in the `containers/libkrun` source (Rust crate with C FFI):

```bash
sudo apt-get install -y \
  libclang-dev \
  libcap-ng-dev
```

Rust stable toolchain must be available (`rustc`, `cargo`).

### Apt packages — libkrunfw build dependencies

Required by `make` in the `containers/libkrunfw` source (builds a Linux kernel
into a shared library):

```bash
sudo apt-get install -y \
  flex \
  bison \
  dwarves \
  bc \
  libelf-dev \
  cpio \
  python3-pyelftools
```

### Not in Debian repos

| Component | Status in Debian 13 | How we get it |
|-----------|-------------------|---------------|
| libkrun | **not packaged** | build from source |
| libkrunfw | **not packaged** | build from source |

This is the single biggest difference from a distro where these are packaged
(e.g., Fedora ships `libkrun-devel` and `libkrunfw`). On Debian, the entire
libkrun+libkrunfw stack must be compiled and installed before the patched crun
can be built.

## Source build: libkrun

```bash
git clone https://github.com/containers/libkrun.git
cd libkrun
git checkout v1.17.4   # or latest stable tag

make                    # builds target/release/libkrun.so.<version>
sudo make install       # installs to /usr/local/lib64/ and /usr/local/include/
```

Installs:
- `/usr/local/lib64/libkrun.so.1.17.4` (plus `libkrun.so.1` and `libkrun.so` symlinks)
- `/usr/local/include/libkrun.h`
- `/usr/local/lib64/pkgconfig/libkrun.pc`

## Source build: libkrunfw

```bash
git clone https://github.com/containers/libkrunfw.git
cd libkrunfw
git checkout v5.3.0    # or latest stable tag

make                    # downloads kernel tarball (~140 MB), builds vmlinux, wraps into .so
                        # this step takes 5-15 minutes depending on CPU
sudo make install       # installs to /usr/local/lib64/
```

Installs:
- `/usr/local/lib64/libkrunfw.so.5.3.0` (plus `libkrunfw.so.5` and `libkrunfw.so` symlinks)

The `make` step:
1. Downloads `linux-6.12.76.tar.xz` from `cdn.kernel.org` into `tarballs/`
2. Extracts and builds a minimal kernel config
3. Runs `bin2cbundle.py` (needs `pyelftools`) to convert `vmlinux` into `kernel.c`
4. Compiles `kernel.c` into the shared library

## System library registration

After installing both libraries, they must be visible to the dynamic linker:

```bash
echo "/usr/local/lib64" | sudo tee /etc/ld.so.conf.d/libkrun.conf
sudo ldconfig
```

And to pkg-config for the crun build:

```bash
export PKG_CONFIG_PATH="/usr/local/lib64/pkgconfig:${PKG_CONFIG_PATH:-}"
```

Without this, `./configure --with-libkrun` in the crun build will fail to find
`libkrun.pc`. And without the ldconfig entry, `crun --version` and any
`crun run` invocation will fail with a missing shared library error at runtime.

## Source build: patched crun (neovex-crun)

The repo-owned helper handles this:

```bash
# PKG_CONFIG_PATH must include /usr/local/lib64/pkgconfig
export PKG_CONFIG_PATH="/usr/local/lib64/pkgconfig:${PKG_CONFIG_PATH:-}"

bash scripts/build-neovex-crun.sh \
  --source ~/src/github.com/containers/crun \
  --output /tmp/neovex-crun-stage/crun \
  --install-path /usr/libexec/neovex/crun \
  --sudo-install
```

The source checkout must be at tag `1.27` (the patch is pinned to that version):

```bash
cd ~/src/github.com/containers/crun
git checkout 1.27
```

The resulting binary reports `+LIBKRUN` in its version string:

```
crun version 1.27-dirty
+SYSTEMD +SELINUX +APPARMOR +CAP +SECCOMP +EBPF +LIBKRUN +YAJL
```

## GitHub runner requirements

### Hardware

- **KVM access is mandatory.** The krun handler calls `krun_start_enter()` which
  needs `/dev/kvm`. Standard GitHub-hosted runners do NOT expose `/dev/kvm`.
  Options:
  - Self-hosted runner on bare-metal or a VM with nested virtualization enabled
  - GitHub larger runners with KVM (currently in limited availability)
  - A separate build+test machine triggered via SSH or webhook

### Runner image

A minimal Debian 13 / Ubuntu 24.04+ image with:

1. Rust stable toolchain
2. All apt packages listed above
3. libkrun and libkrunfw built and installed from source
4. `/etc/ld.so.conf.d/libkrun.conf` entry and ldconfig run
5. The user running the job must be in the `kvm` group (or `/dev/kvm` must be
   world-accessible)
6. Rootless container support: `uidmap` installed, `/etc/subuid` and
   `/etc/subgid` configured for the runner user

### Build cache opportunities

| Artifact | Cache key | Invalidation |
|----------|-----------|-------------|
| libkrun `.so` | `libkrun-{tag}-{os}-{arch}` | tag bump |
| libkrunfw `.so` | `libkrunfw-{tag}-{os}-{arch}` | tag bump (kernel rebuild is expensive) |
| crun patched binary | `neovex-crun-{crun-tag}-{patch-sha}-{os}-{arch}` | crun tag bump or patch change |
| Rust build cache | `cargo-{lockfile-hash}` | dependency change |

libkrunfw is the slowest to build (5-15 min for the kernel). Caching the built
`.so` is the highest-value optimization.

### Estimated CI time budget

| Step | Time |
|------|------|
| apt install (all packages) | ~30s |
| libkrun build (from source) | ~60s |
| libkrunfw build (from source, kernel) | 5-15 min |
| neovex-crun build (patched crun) | ~90s |
| LH1-LH4 validation | ~10s |
| LH5 direct krun drill | ~15s (VM boot + HTTP probe) |
| LH6 conmon drill | ~15s |
| **Total (cold)** | **~8-18 min** |
| **Total (cached libkrunfw)** | **~3-5 min** |

## Key lifecycle learnings

These findings affect how neovex will invoke the krun stack in production and
must inform the `neovex-sandbox` Rust backend (V3).

### 1. krun containers must NOT have a network namespace

The default OCI spec from `crun spec` includes a `network` namespace. For krun
containers this must be removed. TSI (Transparent Socket Impersonation) works
through vsock and binds host-side ports in the parent network namespace. If a
separate network namespace exists, the TSI-bound ports are invisible to the host.

The fix is now checked into `scripts/prepare-krun-bundle.sh`. The V3 Rust
backend must replicate this: when generating OCI config for a krun container,
omit the `network` namespace type from `linux.namespaces`.

### 2. Terminal mode must be disabled for non-interactive krun containers

The default OCI spec sets `process.terminal: true`. When running a krun
container non-interactively (backgrounded, under conmon), this causes
`tcgetattr: Inappropriate ioctl for device` and can interfere with
conmon's stdio handling.

Set `process.terminal: false` in the OCI config for service-mode krun
containers. The fix is checked into `scripts/prepare-krun-bundle.sh`.

### 3. The conmon attach lifecycle gates container start

Conmon with `--full-attach` (which is what Podman always uses) does NOT
auto-start the container. The lifecycle is:

```
conmon                                   client (Podman / neovex)
  │                                        │
  ├── crun create ─────────────────────>   │
  │   (container enters "created" state)   │
  │                                        │
  ├── waits for attach connection ──────   │
  │                                        ├── connect to attach socket
  │                                        │
  ├── receives attach ──────────────────   │
  ├── crun start ──────────────────────>   │
  │   (krun_start_enter() called)          │
  │   (VM boots, TSI ports bind)           │
  │                                        ├── probe HTTP on TSI port
  │                                        │
  ├── VM runs... ──────────────────────>   │
  │                                        │
  ├── VM exits / SIGKILL ──────────────>   │
  ├── writes exit file ────────────────>   │
  └── conmon exits                         │
```

**Implication for neovex:** The `neovex-sandbox` krun backend must connect to
the conmon attach socket after `conmon` has run `crun create`, before the
container will actually start. This is not optional — it is how conmon works.
Podman's `startOCIContainer()` method in
`libpod/oci_conmon_common.go` handles this by reading the sync pipe and
then calling `HTTPAttach()` or writing to the start pipe.

### 4. Rootless operation requires a user namespace wrapper

The krun handler writes `.krun_config.json` to the container rootfs during
`crun create` using `openat2` with `RESOLVE_IN_ROOT`. In rootless mode without
an enclosing user namespace, this fails with `Permission denied` because the
calling process lacks write access to the rootfs before its own user namespace
mapping takes effect.

The working pattern is `buildah unshare`, which establishes a user namespace
with proper UID mapping before any crun operation. Podman does the equivalent
internally. The `neovex-sandbox` backend will need to either:

- Run crun/conmon inside a pre-established user namespace (like Podman does), or
- Run as root (which sidesteps the issue but loses rootless isolation)

### 5. The crun process IS the VMM

In the krun model, `crun` does not exit after starting the VM.
`krun_start_enter()` blocks — the crun process becomes the VMM. The process
tree is:

```
conmon ─── [libcrun:krun] /bin/busybox httpd -f -p 8080
              ├── {libkrun VM} (8 worker threads)
              ...
```

Conmon monitors the crun process. When the VM exits, `_exit()` kills the
crun process, conmon detects it and writes the exit file.

**Implications:**
- SIGTERM to the crun process does not cleanly stop the VM (SIGKILL required)
- The crun process must remain alive for the entire VM lifetime
- `crun state` shows the container as `running` while the VM is up
- Exit code 137 (128 + 9 = SIGKILL) is the expected exit code for forced stop

### 6. TSI port binding is observable via `ss`

TSI ports appear in `ss -tlnp` output as owned by `libkrun VM`:

```
LISTEN 0  9  *:18080  *:*  users:(("libkrun VM",pid=88519,fd=89))
```

This is useful for health checking and readiness probing. The V3 backend can
poll `ss` or use `getsockopt` to verify TSI port readiness after `crun start`.

## Gaps in current checked-in scripts

These are patterns that worked during the validation but are NOT yet encoded
in the repo's helper scripts. They need to be addressed before the scripts can
run unattended on a fresh runner.

| Gap | Affects | Fix needed |
|-----|---------|-----------|
| `PKG_CONFIG_PATH` not set in `build-neovex-crun.sh` | LH3 | Script should detect `/usr/local/lib64/pkgconfig/libkrun.pc` and set the var, or document it as a prerequisite |
| No libkrun/libkrunfw build automation | LH3 | Either a new `scripts/build-libkrun-from-source.sh` or runner setup docs |
| No `/etc/ld.so.conf.d/libkrun.conf` setup | LH3, LH5, LH6 | Build script or runner provisioning |
| `buildah unshare` not in drill wrappers | LH5, LH6 | Generated `start-runtime.sh` and `run-conmon.sh` should run inside `buildah unshare` or document the requirement |
| `crun start` not called after conmon create | LH6 | Generated `run-conmon.sh` or a companion script must connect to the attach socket or call `crun start` |
| `check-vmm-host.sh` doesn't verify libkrun libraries | LH1 | Should check for `libkrun.so` in ldconfig cache and `libkrun.pc` in pkg-config |
| Kernel download in libkrunfw build needs network | CI | Runner must have outbound HTTPS to `cdn.kernel.org` unless the tarball is cached |

## Distribution implications

The fact that libkrun and libkrunfw are not packaged for Debian means neovex's
distribution story must either:

1. **Bundle prebuilt `.so` files** in the neovex release archive or `.deb`
   package (simplest for operators)
2. **Provide a build-from-source script** and document the process (current
   state)
3. **Contribute Debian packages upstream** for libkrun and libkrunfw (long-term)
4. **Target Fedora first** where `libkrun-devel` and `libkrunfw` are already
   packaged (reduces initial packaging burden)

For GitHub Actions CI specifically, option 1 (cache prebuilt `.so` files) is
the pragmatic choice. The CI workflow can build libkrun+libkrunfw once per tag
bump and cache the artifacts, then restore them on subsequent runs.

## Fedora comparison

On Fedora 40+, the setup is dramatically simpler because libkrun is packaged:

```bash
sudo dnf install -y \
  conmon buildah crun podman \
  libkrun-devel libkrunfw \
  yajl-devel libseccomp-devel libcap-devel systemd-devel

# No source builds needed — go straight to building patched crun
bash scripts/build-neovex-crun.sh --source ~/src/github.com/containers/crun --output /tmp/crun
```

This makes Fedora the lower-friction CI target for the krun validation lane.
Debian support works but requires the source-build bootstrapping documented
above.
