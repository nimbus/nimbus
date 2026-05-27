# Bun/JSC Gate 24: In-Process Lockdown Closeout

Date: 2026-05-23

Nimbus plan: `docs/plans/archive/bun-jsc-in-process-lockdown-plan.md`

## Final State

Status: proof-only, upstream-first, not selectable.

The in-process Bun/JSC lockdown plan is complete. Nimbus now has explicit
backend trust, lockdown, and lifecycle axes, including future Bun pool
semantics, but no production Bun selector, route, codegen target, or OCI
fallback was added.

The product decision is:

- Bun inside OCI/microVM remains an existing sandbox workload pattern.
- Bun/JSC as an in-process runtime backend remains blocked for untrusted tenant
  code.
- A future selectable Bun backend should have a dedicated Bun/JSC pool beside
  the existing V8/Deno/Node pool.
- That Bun pool must prove resolver, permission, memory, cancellation, reuse,
  and teardown isolation before retained or fresh/discard Bun/JSC execution can
  be promoted for untrusted tenants.
- Fork posture is upstream-first; no Nimbus Bun fork yet.

## Verification

Nimbus:

```sh
cargo fmt --all --check
cargo clippy -p nimbus-runtime -p nimbus-server -p nimbus-bin -- -D warnings
cargo test -p nimbus-runtime limits::tests --lib
cargo test -p nimbus-server registry_and_license::registry --lib
cargo test -p nimbus-runtime --test engine_proofs \
  bun_jsc_build_gate_reproduces_from_bun_build_graph \
  -- --ignored --nocapture
git diff --check
```

Results:

- `cargo fmt --all --check`: passed
- `cargo clippy -p nimbus-runtime -p nimbus-server -p nimbus-bin -- -D warnings`: passed
- `cargo test -p nimbus-runtime limits::tests --lib`: 9 passed
- `cargo test -p nimbus-server registry_and_license::registry --lib`: 10 passed
- ignored Bun source proof test: 1 passed
- `git diff --check`: passed

Bun:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Results:

- `cargo fmt --all --check`: passed
- `bun scripts/build.ts ... --target=check-bun-embed-probe`: passed and emitted `[build] check-bun-embed-probe done`
- `git diff --check`: passed

Reusable gate:

```sh
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

Result: passed all ten script steps.

Linux/minicloud:

```sh
NIMBUS_BUN_REPO=~/src/github.com/oven-sh/bun \
NIMBUS_BUN_BUILD_DIR=~/.cache/nimbus-proof/bun-embed-native \
NIMBUS_BUN_CACHE_DIR=~/.cache/nimbus-proof/bun-cache \
NIMBUS_BUN_RUST_ONLY_BUILD_DIR=~/.cache/nimbus-proof/bun-rust-only \
NIMBUS_BUN_CARGO_TARGET_DIR=~/.cache/nimbus-proof/bun-cargo-target \
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

Result: passed all ten script steps on Debian 13 `minicloud` with Bun proof
commit `ce5aa2a389`.

## Residual Work

The next implementation work is not "wire Bun into product routing." It is one
of:

- draft an upstream Bun embedder API proposal for global construction,
  resolver policy, native permission hooks, worker propagation, dynamic code,
  and lifecycle/pool behavior, or
- build a new Nimbus implementation plan only after Bun exposes those hooks or
  Nimbus explicitly chooses a maintained fork.

Linux/minicloud verification of `scripts/verify-bun-jsc-in-process-lockdown.sh`
has passed for the proof baseline. Product promotion or any fork dependency
still requires the follow-on embedder API and Bun pool plan to close the
resolver, native permission, memory, cancellation, and teardown gates.
