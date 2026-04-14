# Patched crun CI Build & Distribution

## Compact prompt

Use this when running `/compact` before starting the work:

```
Preserve: building a GitHub Actions workflow for neovex-patched crun
(crun 1.27 + TSI port mapping patch). Build chain: libkrunfw 5.3.0 →
libkrun 1.17.4 → crun 1.27 + patch. Use Fedora container (libkrun-devel
in repos). Matrix: amd64 + arm64. Existing scripts: build-neovex-crun.sh,
verify-crun-patch.sh, verify-neovex-crun-fedora-userspace.sh. Existing
verify-only CI: verify-neovex-crun-patch.yml (has bug: pins crun 1.22,
should be 1.27). Need new workflow neovex-crun.yml with verify → build →
publish jobs. Publish on crun/v* tags with attestation. Reference
machine-os workflow for patterns. Read docs/prompts/patched-crun-ci-build.md
for full context.
```

---

## Session prompt

Paste this after compact finishes:

```
Read docs/prompts/patched-crun-ci-build.md then implement the patched crun
CI build workflow. Start by reading the reference files listed at the bottom
of that doc. Do research if needed to verify Fedora 43 repos have
libkrun-devel for both x86_64 and aarch64 before writing the workflow.
```

---

## Full context

### Goal

Create a GitHub Actions workflow that builds the neovex-patched crun binary
(crun + libkrun TSI port mapping patch) and publishes it as a versioned
release artifact. The binary must be built for linux/amd64 and linux/arm64.

### What exists

**Patch:**
- `patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`
  Adds ~50 lines to `src/libcrun/handlers/krun.c` — reads `krun.port_map`
  OCI annotation and calls `krun_set_port_map()` for TSI port forwarding.

**Build scripts (all working):**
- `scripts/build-neovex-crun.sh` — builds patched binary from crun source
- `scripts/verify-crun-patch.sh` — dry-run patch verification
- `scripts/verify-neovex-crun-fedora-userspace.sh` — full build in Fedora container

**CI (verification only, no artifact publishing):**
- `.github/workflows/verify-neovex-crun-patch.yml` — verifies patch applies
  and does a Fedora userspace build. Does NOT publish artifacts.
  **Bug:** workflow pins `CRUN_VERSION: "1.22"` but scripts target 1.27.

**Documentation:**
- `docs/research/krun-ci-build-and-distribution.md` — full build process,
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
crun 1.27 + patch (C, autotools → /usr/libexec/neovex/crun)
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

**New workflow: `.github/workflows/neovex-crun.yml`**

Triggers:
- Push to main (path-filtered: patches/crun/**, scripts/*crun*)
- Tags: `crun/v*`
- workflow_dispatch

Strategy: Use Fedora container for the build (deps already in repos).
The existing `verify-neovex-crun-fedora-userspace.sh` already does this
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
   - Name convention: `neovex-crun-linux-amd64`, `neovex-crun-linux-arm64`

Caching: Cache the Fedora container image across runs (same pattern as
machine-os workflow). libkrun/libkrunfw come from Fedora repos so no source
build needed.

**Fix existing workflow:**
- Update `CRUN_VERSION` from `"1.22"` to `"1.27"` in
  `verify-neovex-crun-patch.yml`

### Key constraints
- The binary must link against libkrun — verify with `ldd` or `crun --version`
  showing `+LIBKRUN`
- Install to `/usr/libexec/neovex/crun` (not system path)
- Binary is statically usable once libkrun.so is on the target system's
  LD_LIBRARY_PATH
- Test: `./crun --version` should show `crun version 1.27-dirty ... +LIBKRUN`

### Reference files to read first
1. `CLAUDE.md` — repo rules and verification commands
2. `docs/research/krun-ci-build-and-distribution.md` — full build walkthrough
3. `scripts/build-neovex-crun.sh` — existing build script
4. `scripts/verify-neovex-crun-fedora-userspace.sh` — Fedora container build pattern
5. `.github/workflows/verify-neovex-crun-patch.yml` — existing CI (verify only)
6. `.github/workflows/neovex-machine-os.yml` — reference for workflow patterns
   (caching, attestation, parallel pulls, artifact upload)
