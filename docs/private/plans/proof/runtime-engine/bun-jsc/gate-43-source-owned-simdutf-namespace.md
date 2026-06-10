# Gate 43: BJA4L Source-Owned Simdutf Namespace

Date: 2026-05-24

## Purpose

Gate 42 rejected the current dynamic/PIC lane because the available
Bun/WebKit release objects contain non-PIC bmalloc/libpas TLS relocations.
This gate starts the selected source-owned static lane: isolate Bun/WebKit's
`simdutf` symbols at build time while preserving Nimbus' single-binary
in-process runtime shape.

## Bun Patch

The local and `minicloud` Bun worktrees were patched from Bun proof head
`a409f596e8` with patch:

```text
eb9d7d047b44fdc7921a3447c6d155ee1c9f8a1cef9892a79d60280f6e589c85
```

The patch changes only:

```text
scripts/build.ts
scripts/build/config.ts
scripts/build/deps/webkit.ts
scripts/build/flags.ts
scripts/build/rust.ts
src/simdutf_sys/bun-simdutf.cpp
src/simdutf_sys/simdutf.rs
```

The new build option is `--simdutf-namespace=nimbus_bun_simdutf`.

The option is intentionally fail-closed:

- it must be a valid C++ identifier;
- it currently supports only `nimbus_bun_simdutf`;
- it requires `--webkit=local`, because prebuilt WebKit archives already
  contain public `simdutf` symbols.

## Local Hygiene

In `/Users/jack/src/github.com/oven-sh/bun`:

```sh
git diff --check
cargo fmt --all --check
```

Both passed.

## Debian Configure Proof

On Debian 13 `minicloud`, the patch applied cleanly to a clean Bun worktree at
`a409f596e8` and the same hygiene checks passed:

```sh
git diff --check
cargo fmt --all --check
```

The local-WebKit configure proof passed:

```sh
export PATH=$HOME/.cargo/bin:$HOME/.bun/bin:$PATH
export BUN_WEBKIT_PATH=$HOME/src/github.com/oven-sh/WebKit

bun scripts/build.ts \
  --profile=release-local \
  --build-dir=$HOME/.cache/nimbus-bun-proof/configure-namespaced \
  --cache-dir=$HOME/.cache/nimbus-bun-proof/cache-namespaced \
  --simdutf-namespace=nimbus_bun_simdutf \
  --target=check-bun-embed-probe \
  --configure-only
```

Result:

```text
[configured] bun-profile -> bun (stripped)
  target       linux-x64-gnu
  build type   Release
  build dir    ./../../../../.cache/nimbus-bun-proof/configure-namespaced
  revision     a409f596e8
  features     webkit:local, simdutf:nimbus_bun_simdutf

22 deps, 95 codegen, 1130 objects in 7437ms
```

Generated-build audit:

```text
bun_cpp_private_flags=594
bun_cpp_namespace_flags=594
webkit_cmake_namespace_lines=1
rust_cfg_lines=2
```

The generated `build.ninja` passes
`-Dsimdutf=nimbus_bun_simdutf` to local WebKit CMake and passes both
`-DBUN_PRIVATE_SIMDUTF_NAMESPACE` and
`-Dsimdutf=nimbus_bun_simdutf` to Bun C++ objects. Rust receives
`--cfg=bun_private_simdutf_namespace` so the Rust FFI binds Bun's wrapper ABI
as `nimbus_bun_simdutf__*`.

## Fail-Closed Guards

Prebuilt WebKit denial:

```sh
bun scripts/build.ts \
  --profile=release \
  --build-dir=$HOME/.cache/nimbus-bun-proof/configure-namespaced-prebuilt-deny \
  --cache-dir=$HOME/.cache/nimbus-bun-proof/cache-namespaced \
  --simdutf-namespace=nimbus_bun_simdutf \
  --configure-only
```

Result:

```text
status=1
error: --simdutf-namespace requires --webkit=local
  hint: Prebuilt WebKit archives already contain public simdutf symbols; use a local WebKit source build so the namespace is applied to WebKit and Bun together.
```

Invalid identifier denial:

```text
status=1
error: --simdutf-namespace must be a valid C++ identifier
  hint: Use a namespace such as nimbus_bun_simdutf.
```

Unsupported namespace denial:

```text
status=1
error: --simdutf-namespace currently supports only nimbus_bun_simdutf
  hint: The Bun simdutf C wrapper ABI is prefixed to nimbus_bun_simdutf__* for the Nimbus linked-adapter proof.
```

## Source Build Probe

A narrow Ninja target for Bun's simdutf wrapper was attempted:

```sh
ninja -C $HOME/.cache/nimbus-bun-proof/configure-namespaced \
  obj/src/simdutf_sys/bun-simdutf.cpp.o
```

That target pulls in the local static WebKit/JSC target. This is useful: it
proves this is a real source-owned JSC build, not a binary post-processing
lane.

The first run failed because the minimal Debian machine lacked Ruby. After
installing `ruby`, the second run failed because WebKit/JSC needed ICU
development files. After installing `libicu-dev`, WebKit CMake configured
successfully and began compiling JSC/WTF source with the namespace flag:

```text
-Dsimdutf=nimbus_bun_simdutf
```

Observed compile examples included:

```text
Source/WTF/wtf/SIMDUTF.cpp.o
Source/JavaScriptCore/.../UnifiedSource-bytecode-7.cpp.o
Source/JavaScriptCore/dfg/DFGSpeculativeJIT.cpp.o
```

The build was intentionally stopped after about 19 minutes to avoid hiding a
full source-owned JSC build inside a "single object" proof. The partial cache
was preserved:

```text
/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced  470M
/home/nimbus/src/github.com/oven-sh/WebKit                  7.6G
/dev/sda2                                                   250G available
```

No proof build processes remained running after interruption.

## Decision

The source-owned static namespace path is viable enough to continue. This gate
proves:

- the Bun build seam can express a private simdutf namespace;
- the seam is fail-closed for prebuilt WebKit and unsupported namespaces;
- Bun C++, Bun Rust FFI, and local WebKit CMake receive matching namespace
  settings;
- Debian `minicloud` can configure local WebKit/JSC with the namespace after
  installing normal WebKit build prerequisites.

This gate does not close BJA4L2 or BJA4L3. The next gate must complete the
source-owned JSC/Bun build, audit actual archive/object symbols, and prove the
same-process V8 plus Bun/JSC link without `--allow-multiple-definition`.

## Next

- Promote or preserve the Bun patch in a reproducible source revision.
- Run a dedicated long source-build gate on `minicloud` using the existing
  partial cache.
- Audit built archives/objects for:
  - Bun wrapper symbols as `nimbus_bun_simdutf__*`;
  - WebKit/WTF C++ symbols as `nimbus_bun_simdutf::`;
  - no unsafe duplicate `simdutf::` or `simdutf__` families against V8.
- Update `scripts/verify-bun-jsc-linked-adapter.sh` only after the archive
  and link evidence exists.
