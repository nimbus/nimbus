# Convex Node Runtime Configuration Plan

Status: `done`

Owner: Convex adapter / Node-compatible runtime

## Purpose

Make Neovex's Convex-compatible Node runtime surface match the developer
contract Convex documents today, while keeping support claims evidence-backed.
The target experience is familiar to Convex developers:

- `"use node"` at the top of a function module selects the Node.js runtime for
  actions in that module.
- `convex.json` can declare `node.nodeVersion` as `"20"`, `"22"`, or `"24"`.
- Neovex keeps Node22 as its default until a deliberate Node24-default
  migration is planned and verified.
- `neovex dev --once --debug-node-apis` and the codegen CLI equivalent explain
  Node builtin usage that should move behind `"use node"`.
- Node builtin diagnostics recognize both bare and `node:` specifiers such as
  `fs` and `node:fs`.

## Source Contract

Reviewed source docs:

- Convex runtimes:
  https://docs.convex.dev/functions/runtimes
- Convex project configuration:
  https://docs.convex.dev/production/project-configuration#configuring-the-nodejs-version
- Convex bundling:
  https://docs.convex.dev/functions/bundling

Important Convex behaviors to mirror:

- The default Convex runtime is browser/Workers-like, while Node.js is opt-in
  for actions through a file-level `"use node"` directive.
- Files using `"use node"` should not contain queries or mutations, and
  non-Node files should not import Node files.
- Node API bundling errors should point developers toward the slower
  `--debug-node-apis` diagnostic path.
- Node action dependencies may be bundled or configured as external packages.
- `convex.json` supports `node.nodeVersion` values `"20"`, `"22"`, and `"24"`.

## Guardrails

- Do not weaken runtime tests or classify compatibility as green without
  measured evidence.
- Keep product support distinct from test-lane evidence. Node20 and Node24 are
  supported lanes only when the runtime selection path actually exercises
  those compatibility targets.
- Preserve the runtime crate's zero-workspace-dependency invariant.
- Keep Convex compatibility behavior clean rather than layered behind legacy
  shims; the repo is pre-launch.
- If a Node action feature is not implemented yet, fail with a precise,
  owner-backed diagnostic instead of silently falling back to the default
  runtime.

## Work Queue

### CNR1 Project Config Contract

Status: `done`

Add a small, schema-conscious `convex.json` reader in the codegen/authoring
path. It should preserve the existing `functions` root behavior and add
validation for `node.nodeVersion` with an internal normalized target:
`node20`, `node22`, or `node24`. Default remains `node22`.

Close criteria:

- Valid `"20"`, `"22"`, and `"24"` values are accepted.
- Invalid versions fail fast with a clear message.
- The generated Convex artifact set records the selected Node action target.
- Codegen fixtures cover default and configured versions.

### CNR2 `"use node"` Module Classification

Status: `done`

Detect the directive prologue in each Convex module. Node modules may define
actions and helper-only code; they may not define queries or mutations. The
manifest should record runtime selection per function so server/runtime
loading does not infer it from source text later.

Close criteria:

- `"use node"` actions are represented explicitly in `functions.json`.
- `"use node"` queries/mutations fail with Convex-compatible diagnostics.
- Non-Node functions keep using the default web-standard compatibility target.
- Tests cover directive prologue placement and mixed module rejection.

### CNR3 Node Runtime Target Plumbing

Status: `done`

Promote `RuntimeCompatibilityTarget` from a Node22-only product enum to a
Node20/Node22/Node24 selection surface while retaining the same V8/Deno-family
backend. Node22 remains the default. Tooling/subprocess constraints must be
explicit per target and backed by the existing lane evidence.

Close criteria:

- Runtime limits expose application Node20, Node22, and Node24 constructors.
- Product docs no longer describe Node20/Node24 as preview-only once the
  runtime path can select them.
- Focused runtime tests prove selected targets serialize and normalize
  correctly.

### CNR4 `--debug-node-apis` Diagnostics

Status: `done`

Add `neovex dev --once --debug-node-apis` and the codegen CLI equivalent. The
diagnostic pass should identify Node builtin imports/requires in files that do
not opt into `"use node"` and explain how to fix the owning module.

Close criteria:

- Diagnostics include import chain context when available.
- Both `fs` and `node:fs` forms are recognized, along with other Node builtins
  and subpaths already tracked by the compatibility matrix.
- Normal codegen stays fast; the debug path is opt-in.
- CLI/help/docs match the supported command surface.

### CNR5 Bundling And External Package Parity

Status: `done`

Align Neovex docs and artifact metadata with Convex bundling behavior for Node
actions, including `node.externalPackages`. Keep unresolved package behavior
truthful if server-side installation is not yet supported.

Close criteria:

- `convex.json` `node.externalPackages` is parsed or rejected with a precise
  unsupported-feature diagnostic.
- Docs explain bundled versus external Node action dependencies.
- Tests cover the current behavior so package-handling claims stay honest.

### CNR6 Docs, Evidence, And Closeout

Status: `done`

Update README, CLI docs, Convex adapter docs, runtime surface matrix, and
plan index. Close only after verification shows Node20/22/24 configuration,
`"use node"`, debug diagnostics, and builtin specifier parity are synchronized
across code, docs, and tests.

Close criteria:

- Plan status is `done` and indexed as a stable baseline.
- `npm run test --workspace @neovex/codegen` passes.
- Focused Rust tests for runtime target/config loading pass.
- `cargo fmt --all --check` and `git diff --check` pass.

## Progress Log

- 2026-05-12: Created the active plan from Convex runtime, project
  configuration, and bundling docs. Started CNR1 with codegen-side project
  configuration and diagnostics as the first safe slice.
- 2026-05-12: Completed CNR1-CNR6. Codegen now reads `convex.json`
  `node.nodeVersion`, records Node action metadata, enforces `"use node"`
  action-only modules, diagnoses default-runtime Node builtin imports with
  `--debug-node-apis`, binds bare and `node:` builtin imports into generated
  Node action bundles, parses `node.externalPackages` into metadata, and the
  server selects per-function runtime lanes for Node20, Node22, and Node24.
