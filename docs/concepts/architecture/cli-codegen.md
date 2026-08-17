---
title: CLI and codegen
description: How the single nimbus binary structures its command tree, boots the server, runs the dev loop, and generates code with an embedded JavaScript toolchain.
sidebar:
  label: CLI & codegen
  order: 11
---

Everything user-facing ships in one binary. The CLI in
`crates/nimbus-cli` is not a thin wrapper around a separate server — it
*is* the server, plus the developer loop, plus the code generator, plus
the host-management surface. This page explains how those pieces
compose. The full flag-by-flag reference lives in
[CLI commands](/reference/cli/).

## One binary, one command tree

`crates/nimbus-cli/src/lib.rs` declares the clap command tree — `start`,
`dev`, `deploy`, `run`, `codegen`, `init`, `token`, `auth`, `ui`,
`machine`, `node`, `compose`, `policy`, `encryption`, `packages`, `kv`,
and `object-storage` among others — plus hidden internal entrypoints for
the validated container runner and sandbox-local supervisor. Each
subcommand owns a module under `crates/nimbus-cli/src/`; the top-level
`run_from_env` entry point stays a thin dispatcher, and the binary itself
(`crates/nimbus-bin/src/main.rs`) is a small shim that initializes tracing
and calls it. The practical consequence: a production node, a
laptop dev loop, and a CI codegen step all run the same binary with the
same embedded assets, so there is no version skew between "the CLI" and
"the server".

## How `start` composes the server

`nimbus start` is the composition root. The boot sequence in
`crates/nimbus-cli/src/start/boot.rs` resolves configuration, loads the
license, wires function registries, mints serve options, and only then
binds the listener.

Configuration resolution (`crates/nimbus-cli/src/start/config.rs`)
follows a strict precedence: **flag, then environment, then file**. The
implementation makes the order structural — inputs from the command line
are built first and each lower layer is applied only as a fallback for
fields still unset. The file layer is a JSON document pointed to by
`NIMBUS_CONFIG`, contains only the persistence section, and rejects
unknown fields outright rather than ignoring them. Defaults are minimal:
the data directory is `./data`, the control-plane directory defaults to
the data directory, and the tenant persistence provider is chosen from
sqlite, libsql-replica, redb, postgres, or mysql — with conflicting
cross-provider overrides rejected at resolve time. The full matrix is in
[Configuration](/reference/configuration/).

Two safety gates shape binding. First, a non-loopback bind requires an
explicit host opt-in, checked *before* any local state is initialized.
Second, after the admin token is loaded, a freshness check refuses to
serve on a non-loopback bind with a stale token. Under systemd socket
activation, boot verifies the activation contract — exactly one passed
file descriptor, addressed to this process — and adopts the inherited
listener instead of binding.

License resolution checks the flag, then the environment, then a
`license.json` in the global config directory. Function registries are
wired only when generated artifacts actually exist on disk
(`.nimbus/convex/functions.json`, `.nimbus/firebase/artifact.json`);
`nimbus start` without an app directory boots at generation zero and
waits for deploys.

## How `dev` differs

`nimbus dev` (`crates/nimbus-cli/src/dev.rs`) is `start` plus a watch
loop, racing in the same process: the server runs in-process on port
3210 while a poll-based watcher drives regeneration, and whichever
finishes first ends the session.

The dev plan (`crates/nimbus-cli/src/dev/plan.rs`) pins
development-friendly choices: state lives under `<app>/.nimbus/dev`, the
provider is sqlite, a `demo` tenant is auto-created, tenant isolation
runs in a local-development mode, and a fresh random deploy token is
generated per run.

The watch loop (`crates/nimbus-cli/src/dev/watch.rs`) polls every 500ms
with a 300ms debounce, fingerprinting files by modification time and
length and skipping generated and vendored directories (`_generated`,
`node_modules`, `.git`, `.nimbus`, and build outputs). On change it runs
codegen, then **deploys to itself**: an HTTP deploy request posted to
its own local URL, authenticated with that per-run token. Dev does not
have a privileged side door into the engine — a dev reload exercises the
same deploy path a remote client would.

App-directory detection walks up from the working directory looking for
a `nimbus/` or `convex/` directory, generated Convex functions, or a
`firebase.json`. The walk is bounded at the repository boundary:
`crates/nimbus-cli/src/path_boundary.rs` stops at the first ancestor
containing `.git` — checked by existence, so worktree and submodule
`.git` *files* count — ensuring a lookalike directory outside the repo
can never be adopted as the app.

## Codegen without a toolchain

Code generation (schema parsing, function manifest emission, generated
TypeScript) is implemented in JavaScript in `packages/codegen`, but
users do not install it. The package is bundled at build time and
embedded in the binary, and `crates/nimbus-cli/src/codegen.rs` chooses
between two runners:

- **Embedded runner (default).** The binary materializes its embedded
  tooling closure into `<app>/.nimbus/tmp` — inside the app directory,
  because the runtime's filesystem capability boundary denies temp paths
  outside it — writes a small bootstrap module, and executes the bundle
  on the in-binary V8 runtime with Node-22-shaped tooling limits, a
  run-to-completion policy, a startup snapshot cache, and a host bridge
  that rejects database and host calls. Codegen is pure input-to-output;
  it cannot touch the engine.
- **External Node runner.** Cloud Functions layouts route to a system
  Node automatically (their build contract needs a real Node), and
  `NIMBUS_CODEGEN_RUNNER=external-node` exists as a diagnostic opt-out
  elsewhere. Even this path runs the *embedded* tooling closure — the
  app never needs a `@nimbus/codegen` dependency.

When external Node is required, `crates/nimbus-cli/src/node_runtime.rs` enforces
a version floor: Node majors below 22 are rejected, newer majors are
allowed with a warning.

## Embedded assets and package provisioning

`crates/nimbus-assets` is the catalog of everything the binary carries:
built JS packages, project templates, and the embedded UI. Packages are
embedded with a manifest recording a SHA-256 digest per file, and
digests are re-verified when files are materialized to disk — a
corrupted or tampered payload fails closed instead of being written.
Project templates (schema, example functions, `tsconfig`, scaffold
`package.json` with project-name substitution) are compiled in as
strings.

SDK packages reach an app through provisioning
(`crates/nimbus-cli/src/provision.rs`): the binary writes its embedded
packages into `<app>/.nimbus/packages/<name>/` and scaffolded
`package.json` files reference them with `file:./.nimbus/packages/...`
specifiers — so `npm install` links the SDK that shipped *inside this
binary*, with no registry fetch and no version skew. Provisioning is
idempotent and stamp-guarded: a `.version` file holding the manifest
digest is written last, a matching stamp short-circuits the work, and a
binary upgrade (different digest) re-provisions automatically.
Provisioning runs implicitly on the `init`, `dev`, `codegen`, and
`deploy` paths, and explicitly via `nimbus packages`.

## Related pages

- [CLI commands](/reference/cli/) — the flag-level reference.
- [Configuration](/reference/configuration/) — the resolved settings
  matrix.
- [Node lifecycle](/concepts/architecture/node-lifecycle/) — how the
  binary becomes a supervised service.
- [SDKs and packages](/concepts/architecture/sdk-packages/) — what the
  provisioned packages contain.
