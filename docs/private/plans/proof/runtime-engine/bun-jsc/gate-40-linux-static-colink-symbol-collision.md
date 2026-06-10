# Gate 40: Linux Static Co-Link Symbol Collision

Date: 2026-05-24

## Purpose

This gate records the Debian 13 `minicloud` result for the BJA4 linked pure
invocation proof. The local macOS static link passed, but Linux found a native
symbol collision between the existing Deno/V8 runtime stack and Bun/WebKit.

This is product-relevant evidence: Nimbus wants Bun/JSC as an in-process
runtime backend beside the existing Deno/V8/Node backend. A proof that only
works when V8 is not present would not satisfy that architecture.

## Environment

Host:

```text
nimbus@192.168.4.29
hostname: minicloud
OS: Debian 13
kernel: Linux 6.12.88+deb13-amd64
arch: x86_64
```

Source:

```text
Nimbus proof worktree:
/home/nimbus/src/github.com/nimbus/nimbus-worktrees/bun-jsc-linked-adapter-proof
commit: f1158cd1

Bun worktree:
/home/nimbus/src/github.com/oven-sh/bun
commit: a409f596e8e1394d8860e2cd8b2bb558ff1afcac
```

Tooling:

```text
Rust: /home/nimbus/.cargo/bin/rustc 1.95.0
Bun CLI: /home/nimbus/.bun/bin/bun
```

Home-backed proof paths:

```sh
TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp
NIMBUS_BUN_PROOF_ROOT=$HOME/.cache/nimbus-bun-proof
NIMBUS_BUN_BUILD_DIR=$HOME/.cache/nimbus-bun-proof/linked-adapter-release
NIMBUS_BUN_CACHE_DIR=$HOME/.cache/nimbus-bun-proof/cache
NIMBUS_BUN_CARGO_TARGET_DIR=$HOME/.cache/nimbus-bun-proof/bun-cargo-target-release
```

The remote Nimbus main worktree had an unrelated dirty file:

```text
scripts/verify-bun-jsc-in-process-lockdown.sh
```

The proof used a detached Nimbus worktree so that local state was preserved.

## Passing Evidence

The full linked gate was started with:

```sh
PATH=$HOME/.cargo/bin:$HOME/.bun/bin:$PATH \
TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp \
NIMBUS_BUN_PROOF_ROOT=$HOME/.cache/nimbus-bun-proof \
NIMBUS_BUN_BUILD_DIR=$HOME/.cache/nimbus-bun-proof/linked-adapter-release \
NIMBUS_BUN_CACHE_DIR=$HOME/.cache/nimbus-bun-proof/cache \
NIMBUS_BUN_CARGO_TARGET_DIR=$HOME/.cache/nimbus-bun-proof/bun-cargo-target-release \
bash scripts/verify-bun-jsc-linked-adapter.sh
```

The gate proved these stages before failing at the final linked invocation:

- Default no-link contract passed:
  - 11 runtime policy and memory semantics tests
  - 7 Bun/JSC pool scaffold tests
  - 13 Convex runtime lane registry tests
  - 2 runtime diagnostics API tests
  - 1 tenant admission test
  - 2 operator UI test files / 5 tests
- Linked adapter feature/no-manifest unit contract passed: 10 tests.
- Required Bun proof exports were present, including
  `nimbus_bun_embed_invoke_program_wrapper_json`.
- Bun Rust format passed.
- The release-profile Bun native embed probe passed and generated
  `nimbus-bun-embed-link-args.txt`.
- The native probe emitted cancellation, permission surface, package/resolver,
  memory behavior, and lifecycle evidence.

## Failing Evidence

The failure happened in step 6, while linking the Nimbus `nimbus-runtime` test
binary against the Bun/WebKit manifest:

```text
error: linking with `clang++-21` failed: exit status: 1
```

The actionable linker diagnostics were duplicate `simdutf` symbols from V8 and
Bun/WebKit:

```text
ld.lld: error: duplicate symbol: simdutf::BOM::check_bom(char const*, unsigned long)
>>> defined at simdutf.cpp in libv8-*.rlib
>>> defined at simdutf_impl.cpp.h in libWTF.a

ld.lld: error: duplicate symbol: simdutf::implementation::supported_by_runtime_system() const
>>> defined at simdutf.cpp in libv8-*.rlib
>>> defined at simdutf_impl.cpp.h in libWTF.a
```

The duplicate family continued through `simdutf::get_active_implementation`,
`simdutf::validate_utf8`, and related `simdutf` runtime functions.

## Unsafe Workaround Rejected

I also ran a diagnostic-only retry with:

```sh
RUSTFLAGS="-C link-arg=-Wl,--allow-multiple-definition"
```

That allowed the link to complete, but the test binary crashed:

```text
process did not exit successfully
signal: 11, SIGSEGV: invalid memory reference
```

Therefore `--allow-multiple-definition` is not an acceptable product or proof
fix. The duplicate symbols are not harmless; allowing the first definition to
win can select V8's `simdutf` implementation for Bun/WebKit code and crash the
process.

## Decision

BJA4 is locally proven on macOS, but it is not platform-complete. Static
co-linking the current V8 and Bun/WebKit archives into one Linux binary is
unsafe until Nimbus owns a real symbol-isolation strategy.

Do not start BJA5 HostBridge product work on top of the current static Linux
co-link. Add a symbol-isolation subgate first.

Acceptable next directions:

- Namespace or hide the colliding `simdutf` symbols in a Nimbus-owned
  Bun/WebKit source fork or in the Nimbus `rusty_v8` fork, then prove both V8
  and Bun/JSC can run in the same binary.
- Build a Bun embedder dynamic library with hidden/local native symbols and an
  explicit Nimbus C ABI, then load it in-process with a controlled loader
  contract.
- If neither path is maintainable, keep Bun as an external sandbox workload
  rather than claiming a same-binary in-process backend.

The third option is not the goal of this plan; Nimbus already has OCI/microVM
execution for external sandbox workloads. For this plan, the next step is to
prove an in-process symbol-isolated Bun/JSC adapter.
