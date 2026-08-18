# Local Development

This runbook defines the contributor bootstrap and local build contract. The
root `Makefile`, `rust-toolchain.toml`, npm workspace files, and CLI help are
the command authorities.

## Prerequisites

- Rust `stable` with `rustfmt` and `clippy`, as specified by
  `rust-toolchain.toml`.
- Node.js 22 or 24 with npm for JavaScript work and Rust targets that embed
  the operator UI or JavaScript packages. These are the CI-supported Node
  lines. Do not use an unverified newer Node release for acceptance evidence.
- `cargo-nextest` 0.9.138 for nextest-backed repository lanes.
- Bash, Make, Git, and the ordinary native build tools for the host.

Run `npm ci` after a fresh checkout or a lockfile change. Do not hand-edit
`package-lock.json`.

## Fresh-checkout build

Use Make when the build starts from a fresh checkout:

```bash
make check
```

The Make dependency graph builds the generated operator UI and staged embedded
JavaScript package payloads before Cargo compiles consumers. A raw
`cargo check --workspace` or `cargo build -p nimbus-bin` does not own that
fresh-checkout prerequisite graph.

Useful full entry points are:

```bash
make build
make test
make clippy
make ci
```

`make ci` is the required local CI aggregate. It includes formatting, Clippy,
dependency policy, Rust tests, required verification harnesses, JavaScript
build/typecheck/tests, and proof helpers.

`make test` runs the same three Rust lanes as CI. The runtime lane owns V8
process-global isolation. The non-runtime workspace lane uses Nextest, which
runs each test in a separate process. This keeps the single Nimbus network
composition local to one test process. The third lane runs workspace doctests.

## Focused iteration

After the generated prerequisites exist, use the smallest command that proves
the changed seam. Examples:

```bash
cargo test -p nimbus-network <test-name>
cargo test -p nimbus-cli <test-name>
npm run test -w convex
```

Serialize focused Cargo work against the shared `target/` directory. Do not
create alternate target directories only to avoid a live build. Repository
Make targets use `scripts/single-flight.sh`. If one reports an active owner,
inspect that process before you clear a stale lock.

## Run a local server

Build the binary through the repository graph, then run it from the workspace:

```bash
make build
target/debug/nimbus start --data-dir .nimbus/local-data
```

For an application development loop, run `nimbus dev` from the application
directory or pass `--app-dir`. Public CLI behavior and defaults live in
[`../../reference/cli.md`](../../reference/cli.md) and
[`../../reference/configuration.md`](../../reference/configuration.md).

Keep data, control state, logs, and other operator state in explicit owned
directories during concurrent or destructive tests. The
`NIMBUS_NETWORK_STATE_DIR` value is host-global network lease authority. Keep
one shared root when processes can compete for the same host resources.

## Generated and temporary state

Generated UI, embedded-package, runtime-snapshot, and application-codegen
outputs are build inputs but are not a reason to accept tracked-source
changes. Before and after a verification run, inspect:

```bash
git status --short
git diff --check
```

Do not recover a test run with `git reset`, `git clean`, or checkout-based
restoration. A test owns and removes its temporary state. If it cannot clean
up, it must fail and retain a named diagnostic artifact.

The application verification lane enforces source-byte preservation, isolated
case state, product-assigned listener leases, complete cleanup, and structured
results. See [`verification.md`](verification.md) for worker, report, and
retained-artifact instructions. [`../plans/README.md`](../plans/README.md)
routes the active owner and acceptance criteria.
