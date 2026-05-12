# Patched crun CI Build & Distribution
---

## Full context

### Goal

Create a GitHub Actions workflow that builds the nimbus-patched crun binary
(crun + libkrun TSI port mapping patch) and publishes it as a versioned
release artifact. The binary must be built for linux/amd64 and linux/arm64.

### What exists

**Patch:**
- `patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`
  Adds ~50 lines to `src/libcrun/handlers/krun.c` — reads `krun.port_map`
  OCI annotation and calls `krun_set_port_map()` for TSI port forwarding.

**Build scripts (all working):**
- `scripts/build-nimbus-crun.sh` — builds patched binary from crun source
- `scripts/verify-crun-patch.sh` — dry-run patch verification
- `scripts/verify-nimbus-crun-fedora-userspace.sh` — full build in Fedora container

**CI (verification only, no artifact publishing):**
- `.github/workflows/verify-nimbus-crun-patch.yml` — verifies patch applies
  and does a Fedora userspace build. Does NOT publish artifacts.
  **Bug:** workflow pins `CRUN_VERSION: "1.22"` but scripts target 1.27.

**Documentation:**
- `docs/plans/research/krun-ci-build-and-distribution.md` — full build process,
  deps, timing estimates, validated on Debian 13

### Pinned versions
- **crun:** 1.27 (upstream tag)
- **libkrun:** 1.17.4 (containers/libkrun)
- **libkrunfw:** 5.3.0 (containers/libkrunfw, downloads kernel tarball)

### Build chain

```
libkrunfw 5.3.0  (C, downloads + compiles Linux kernel → libkrunfw.so)
     ↓                   5-15 min cold, cacheable
libkrun 1.17.4   (Rust + C FFI → libkrun.so, libkrun.h, libkrun.pc)
     ↓                   ~60s
crun 1.27 + patch (C, autotools → /usr/libexec/nimbus/crun)
                         ~90s
```

All three build with `make && sudo make install`. libkrun needs Rust stable.
libkrunfw downloads a kernel tarball (needs outbound HTTPS).

**Build dependencies (Fedora):**
```
autoconf automake bash gcc git libcap-devel libkrun-devel
libseccomp-devel libtool patch pkgconf-pkg-config python3
systemd-devel yajl-devel flex bison dwarves bc libelf-devel
cpio python3-pyelftools libclang-dev libcap-ng-devel
```

On Fedora, libkrun-devel and libkrunfw are in repos — skip building them
from source. On Debian/Ubuntu, must build from source.

### What to build

**New workflow: `.github/workflows/nimbus-crun.yml`**

Triggers:
- Push to main (path-filtered: patches/crun/**, scripts/*crun*)
- Tags: `crun/v*`
- workflow_dispatch

Strategy: Use Fedora container for the build (deps already in repos).
The existing `verify-nimbus-crun-fedora-userspace.sh` already does this
pattern — use it as reference.

Jobs:

1. **verify** — patch syntax, help entrypoints (fast, ubuntu-latest)

2. **build** (matrix: amd64 + arm64)
   - Run on `ubuntu-latest` / `ubuntu-24.04-arm`
   - Build inside a Fedora 43 container (has libkrun-devel in repos)
   - Clone crun at pinned tag, apply patch, build with `--with-libkrun`
   - Extract the binary
   - Upload as artifact

3. **publish** (needs: build, only on tag push)
   - Download artifacts from both architectures
   - Create checksums
   - Attest provenance (actions/attest, matching release.yml pattern)
   - Create GitHub Release with both binaries
   - Name convention: `nimbus-crun-linux-amd64`, `nimbus-crun-linux-arm64`

Caching: Cache the Fedora container image across runs (same pattern as
machine-os workflow). libkrun/libkrunfw come from Fedora repos so no source
build needed.

**Fix existing workflow:**
- Update `CRUN_VERSION` from `"1.22"` to `"1.27"` in
  `verify-nimbus-crun-patch.yml`

### Key constraints
- The binary must link against libkrun — verify with `ldd` or `crun --version`
  showing `+LIBKRUN`
- Install to `/usr/libexec/nimbus/crun` (not system path)
- Binary is statically usable once libkrun.so is on the target system's
  LD_LIBRARY_PATH
- Test: `./crun --version` should show `crun version 1.27-dirty ... +LIBKRUN`

### Reference files to read first
1. `CLAUDE.md` — repo rules and verification commands
2. `docs/plans/research/krun-ci-build-and-distribution.md` — full build walkthrough
3. `scripts/build-nimbus-crun.sh` — existing build script
4. `scripts/verify-nimbus-crun-fedora-userspace.sh` — Fedora container build pattern
5. `.github/workflows/verify-nimbus-crun-patch.yml` — existing CI (verify only)
6. `/Users/jack/src/github.com/nimbus/nimbus-machine-os/.github/workflows/build.yml`
   — reference for workflow patterns (caching, attestation, parallel pulls,
   artifact upload)
