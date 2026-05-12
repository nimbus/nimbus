# Plan: CLI Command Surface - `dev`, `deploy`, `start`, `compose`

Canonical execution plan for the next Nimbus CLI surface wave: add a first-class
local `dev` loop, define a real `deploy` contract for pushing app artifacts to a
running instance, and rename the Compose-backed service namespace to
`nimbus compose` using Docker-native command semantics.

This plan intentionally uses `nimbus start` for the foreground server process
instead of `nimbus serve`, `nimbus run`, or `nimbus agent`. `dev` owns the
local development loop, `run` stays reserved for future Convex-style function
invocation, and `agent` stays reserved for a future clustered node role if
Nimbus gains true agent semantics.

---

## Status

- **Status:** `implemented in current worktree`
- **Primary owner:** this plan
- **Activation gate:** none
- **Parent plan:** none
- **Closeout:** all P1-P6 phases implemented; final verification completed
  locally. Post-review naming cleanup also replaced the public `serve` command
  with `start` and renamed the CLI implementation module to
  `crates/nimbus-bin/src/start/`. Keep this plan as the live CLI
  command-surface entrypoint until the work is merged or archived.
- **Reviewed against:**
  - `crates/nimbus-bin/src/main.rs`
  - `crates/nimbus-bin/src/codegen.rs`
  - `crates/nimbus-bin/src/start/mod.rs`
  - `crates/nimbus-bin/src/start/boot.rs`
  - `crates/nimbus-bin/src/compose/mod.rs`
  - `crates/nimbus-bin/src/compose/commands.rs`
  - `crates/nimbus-bin/src/compose/render.rs`
  - `docs/reference/cli.md`
  - `docs/reference/current-capabilities.md`
  - `docs/reference/microvm-service-baseline.md`
  - `docs/reference/macos-machine-flow.md`
  - `docs/plans/archive/codegen-cli-plan.md`
  - `docs/plans/archive/machine-cli-alignment-plan.md`
  - `docs/plans/archive/machine-cli-follow-on-plan.md`
  - official docs current on 2026-04-22:
    - Convex CLI
    - Convex local deployments
    - Docker Compose `ls`, `ps`, `top`, `up`, and `logs`

---

## Why This Plan Exists

Nimbus now has a coherent first-party `codegen` command and a much cleaner
`--app-dir` contract. Before P1, the overall developer journey was still split
awkwardly between:

```bash
nimbus codegen
nimbus start --app-dir ./my-app
nimbus service up
```

At plan start, the full wave tracked three DX gaps. P1-P6 have closed the
compose rename, local dev loop, server-side generation-swap deploy seam,
user-facing deploy CLI, and `start` startup polish in the current worktree.

1. **There was no first-class local dev loop.**
   Convex users expect `dev` to mean "watch my app, regenerate artifacts, keep
   the local backend running, and stream logs." P2/P3 now provide the local
   server, watched codegen, and local generation activation. Live runtime log
   multiplexing remains follow-on plumbing behind the accepted `--tail-logs`
   surface.
2. **There was no explicit push/update path for a running instance.**
   P4 now defines and implements the deploy/admin generation-swap contract, and
   P5 adds the public `nimbus deploy` CLI over it.
3. **The Compose-backed namespace needed a Docker-native name and subcommand
   mapping.**
   The current commands all operate on a `compose.yaml` file and a deterministic
   project identity. For Docker users, `compose` is the natural name, but the
   command mapping must follow Docker's actual surface:
   `compose ps` is a project status summary and `compose top` is the per-service
   process view.

The goal of this plan is not to chase a different product's branding. The goal
is to make Nimbus feel unsurprising:

- **Convex migrants** should discover `nimbus dev`, `nimbus deploy`, and
  `nimbus codegen` as the obvious path.
- **Docker migrants** should discover `nimbus compose ...` with familiar
  subcommand names where semantics actually match.
- **Operators** should use `nimbus start` for the long-running foreground
  server process until the product grows a real agent role.

### Design Principle

> Command names should follow the strongest nearby analogy, but only when the
> semantics actually match. Good taste comes from honest contracts, not from
> renaming alone.

---

## Current Assessed State

- `nimbus codegen` already ships and is the first-party artifact-generation
  command.
- `nimbus start --app-dir` already accepts both `nimbus/` and `convex/`
  application roots and already performs a one-shot codegen preflight unless
  `--skip-codegen` is set.
- `nimbus start` still owns the only server-start path. It is an explicit
  subcommand, not an implicit root action.
- P1 is implemented in the current worktree: the Compose-backed command surface
  is now `nimbus compose config`, `up`, `down`, `ps`, `inspect`, `logs`, and
  `top`.
- P1 intentionally removed the `nimbus service ...` command surface instead of
  keeping a compatibility alias. This is allowed by the repo's pre-launch
  breaking-change policy.
- The existing service lifecycle implementation still has a useful
  human/structured output contract from the completed machine/service CLI
  alignment wave:
  action summaries for mutation commands, human tables for summary surfaces, and
  structured JSON/YAML for inspect-style surfaces.
- The P1 command mapping is now Docker-correct for the supported subset:
  `compose ps` is the project summary surface and `compose top` is the
  per-service process surface.
- The codegen integration plan is complete and archived; this plan must treat
  `codegen`, `--app-dir`, and start-side one-shot preflight as shipped reality,
  not future prerequisites.
- P4 is implemented in the current worktree: the server exposes an
  authenticated deploy/admin API for staged artifact validation, diffing,
  generation activation, and activation-time rollback behavior. P5 now owns the
  user-facing `nimbus deploy` CLI over that contract.

---

## Control Plan Rules

1. **Use `start` for the foreground server process.**
   Do not keep the weaker public `serve` verb, do not spend `run` on server
   startup, and do not rename the primary server-start command to `agent` in
   this wave. Reserve `run` for future Convex-style function invocation and
   reserve `agent` for a future clustered node role if Nimbus actually gains
   one.
2. **Docker command names must match Docker semantics.**
   If the namespace becomes `compose`, then:
   - project/service status summary belongs on `compose ps`
   - process snapshots belong on `compose top`
   - `compose ls` is reserved for project-level listing, not repurposed as
     `ps`
3. **Convex migration taste matters for app workflows.**
   `dev`, `deploy`, and `codegen` should be the primary app-facing verbs.
   Avoid making Convex migrants learn server-internal vocabulary on the happy
   path.
4. **Human output and machine-readable output are different products.**
   Tables, spinners, progress, colors, and hints are human surfaces. JSON/YAML
   stay boring and scriptable.
5. **Do not promise hot-push behavior without an activation model.**
   `deploy` must define authentication, staging, diffing, activation, in-flight
   request behavior, and rollback before implementation starts.
6. **Do not advertise routes or dashboards that do not exist.**
   A `dev` banner may print the local server URL and app directory. It must not
   promise a dashboard URL until Nimbus actually ships one.
7. **Keep shared UX helpers reusable, but keep precedence rules local.**
   Reuse `cli_ux`, but continue documenting per-command interactions among
   `--quiet`, `--format`, `--follow`, and any future style flags.
8. **Prefer staged rollout over a giant rename-and-feature bundle.**
   The command-surface cleanup should land in phases that each leave the CLI
   internally coherent.

---

## Target CLI Surface

```text
nimbus dev              # Local development loop
nimbus deploy           # Push app artifacts to a running Nimbus instance
nimbus start            # Long-running server
nimbus codegen          # One-shot artifact generation
nimbus compose ...      # Compose-backed local service lifecycle
nimbus machine ...      # Existing machine lifecycle namespace
nimbus encryption ...   # Existing encryption admin namespace
```

### Explicit non-goals for this wave

- no `nimbus agent` rename
- no implicit server start from the root command
- no fake Docker parity where Nimbus semantics differ materially
- no cloud-control-plane assumptions copied from hosted Convex

### Reserved future names

These names stay available for later, but are **not** part of this plan:

```text
nimbus agent ...        # If Nimbus gains real node/cluster agent semantics
nimbus service ...      # If Nimbus gains cluster-scoped service management
```

---

## Command Specifications

### 1. `nimbus dev`

**Audience:** developer iterating locally on a Nimbus or Convex-compatible app.

**DX model:** Convex `dev` for the watch-and-sync loop, plus a minimal local
server banner so the developer immediately knows where the backend is running.

**Why this is the right analogy**

- Convex users already understand `dev` as "watch my functions, regenerate
  generated code, keep the backend available, and show logs."
- The current Convex docs explicitly describe `npx convex dev` as the watched
  development loop and local deployments as a subprocess that is available only
  while `convex dev` is running.
- Nimbus is self-hosted and local-first, so the command must also print the
  local URL clearly.

**Behavior**

- detects `nimbus/` or `convex/` under the selected app directory
- performs one initial codegen pass unless `--skip-codegen` is set
- starts a local Nimbus server with development defaults
- watches source files and re-runs codegen on changes
- activates new artifacts locally without requiring a full process restart
  only after the deploy-generation contract below exists
- tails function/runtime logs in the terminal
- exits cleanly on Ctrl-C and tears down the local dev server

This behavior block describes the full target-state `dev` contract. The first
landed slice is the smaller bootstrap-only subset recorded in the implementation
phases below.

**Default local shape**

These defaults intentionally favor the "clone repo and start coding" path:

| Setting | Default |
| --- | --- |
| port | `3210` |
| app directory | auto-detect from current directory |
| tenant provider | embedded SQLite |
| data root | `./.nimbus/dev/` (shared v1 root for both tenant data and control state) |
| codegen | enabled |
| log tailing | enabled, pause during sync by default |

**Flags**

| Flag | Default | Notes |
| --- | --- | --- |
| `--port` | `3210` | separate from `start`'s `8080` default |
| `--app-dir` | auto-detected | current directory fallback |
| `--compose-file` | unset | optional local service dependencies |
| `--once` | `false` | one-shot codegen + startup, no watch loop |
| `--skip-codegen` | `false` | use already-generated artifacts |
| `--tail-logs` | `pause-on-sync` | mirror Convex taste: `always`, `pause-on-sync`, `disable` |
| `--data-dir` | `./.nimbus/dev/` | explicit override for the shared v1 dev persistence root |

**Terminal output contract**

- sync/progress/errors go to `stderr`
- function/runtime logs go to `stdout`
- the startup banner prints only values Nimbus actually knows today

Example:

```text
  NIMBUS v0.1.0  ready in 342 ms

  Local:    http://localhost:3210/
  App dir:  /path/to/my-app

  Functions ready. Watching for changes in nimbus/ ...
```

On change:

```text
  Preparing Nimbus functions...
  10:42:15 Nimbus functions ready (0.3s)
```

On error:

```text
  Failed to update Nimbus functions
  Error: nimbus/messages.ts(12): Type 'string' is not assignable to type 'number'
```

**Non-TTY behavior**

- no spinner animation
- no ANSI color
- plain line-oriented progress on `stderr`
- logs remain on `stdout`

**Important scope rule**

`dev` is the command that should feel familiar to Convex users. If Nimbus later
needs a "full server with development defaults" mode for operators or internal
testing, that belongs on `start --dev` only if a concrete need emerges. This
plan does not require a separate `agent --dev` surface.

**Phase resolution before implementation**

- P2 is bootstrap-only: initial codegen, local server startup, and a clear
  development banner.
- P2 does **not** watch files, hot-activate new artifacts, or pretend edits are
  live without a restart.
- `--once` belongs to the full watched `dev` surface and should land together
  with P3, not as a redundant flag in the bootstrap slice.

---

### 2. `nimbus deploy`

**Audience:** developer or CI pipeline pushing app artifacts to a running
Nimbus instance.

**DX model:** Convex `deploy` for the user-facing workflow, but with Nimbus's
self-hosted reality made explicit instead of hidden behind a cloud deployment
concept.

**User-facing goal**

The command should read like:

```bash
nimbus deploy --url http://my-nimbus.example.com --app-dir ./my-app
```

or in CI:

```bash
NIMBUS_DEPLOY_URL=http://my-nimbus.example.com \
nimbus deploy --app-dir ./my-app
```

**Required server-side contract**

Before the CLI lands, the server side must define and document:

1. **Authentication and authorization**
   - how an operator or CI job proves it can deploy
   - whether local dev deploys can use a simpler trust path
2. **Artifact payload**
   - required files such as `functions.json`
   - optional files such as `bundle.mjs`, `bundle.sha256`, routes, schema, and
     auth config
3. **Staging**
   - artifacts are written into a versioned staging location, not over the live
     generation in place
4. **Validation**
   - manifest readability
   - bundle integrity when a runtime bundle is present
   - schema/index validation before activation
5. **Diff generation**
   - changed functions
   - added/removed HTTP routes
   - schema and index changes
6. **Activation**
   - once validation succeeds, the server atomically switches the active app
     generation
7. **In-flight request behavior**
   - in-flight requests continue against the previous generation
   - new requests observe the new generation only after activation
8. **Rollback**
   - activation failure leaves the previous generation live
   - v1 requires activation-time internal rollback only
   - an explicit rollback command stays out of scope until Nimbus defines
     retained generation history, operator intent, and admin identity

Without this contract, `deploy` would be a UI over undefined behavior.

**CLI behavior**

- runs codegen first unless `--skip-codegen`
- packages the selected app artifacts
- uploads them to a deploy/admin endpoint
- requests server-side diff + validation
- prints a clear summary of what changed
- exits nonzero on validation or activation failure

**Flags**

| Flag | Default | Notes |
| --- | --- | --- |
| `--url` | `NIMBUS_DEPLOY_URL` or required | explicit target in v1 |
| `--app-dir` | auto-detected | same source-root rules as `codegen` and `dev` |
| `--dry-run` | `false` | validate and diff, but do not activate |
| `--skip-codegen` | `false` | use existing generated artifacts |
| `--verbose` | `false` | show upload and phase detail |

**Terminal output contract**

- phase and status output on `stderr`
- structured diff output may remain on `stdout` only if an explicit machine
  format is added later
- default human output should stay short and trustworthy

Example:

```text
  Preparing Nimbus functions...
  Uploading app artifacts to http://my-nimbus.example.com ...
  Validating schema and indexes...
  Deployed Nimbus app to http://my-nimbus.example.com

  Changes:
    + api.billing.charge        mutation
    ~ api.messages.send         mutation
    - api.legacy.oldEndpoint    query
```

---

### 3. `nimbus start`

**Audience:** operator running a long-lived Nimbus server in development,
staging, or production.

**DX model:** explicit foreground process startup, with the developer happy path
handled by `nimbus dev`.

**Why `start` is the public verb**

- modern platform CLIs tend to reserve `dev` for local iteration and `deploy`
  for artifact rollout; `serve` reads more like a framework preview server than
  a self-hosted backend process
- `start` clearly means "start the server in the foreground" without claiming
  daemon, cluster, or hosted-control-plane semantics
- `run` is better reserved for a future `nimbus run <function>` command that
  mirrors Convex's function-invocation vocabulary
- `agent` would create more confusion than clarity until Nimbus has real
  multi-node agent semantics

**Behavior**

- all former server-start flags stay on `start`
- `--app-dir` and `--skip-codegen` remain the app-loading contract
- `--compose-file` remains the server-side declared-service hook
- the command may gain a cleaner startup summary, but that is polish on top of
  `start`, not a rename justification
- no legacy `nimbus serve` alias is retained

**Possible follow-on polish in this plan**

- concise startup summary on `stderr`
- clearer warnings around codegen preflight and missing app artifacts
- grouped config output when helpful for operators

**Explicit non-goal**

This plan does not add a fake Nomad-style `agent` role just to borrow a banner
format.

---

### 4. `nimbus compose`

**Audience:** developer or operator managing Compose-declared local service
dependencies for one Nimbus project.

**DX model:** Docker Compose command naming, combined with the already-landed
Nimbus output rules for action, summary, and inspect surfaces.

The key rule is simple: if we rename the namespace to `compose`, the subcommand
names must match Docker semantics instead of inventing a new mapping.

#### Rename: `nimbus service` -> `nimbus compose`

This rename is justified because the current namespace is already file-driven
and Compose-backed in implementation. The rename should be a clean pre-launch
break with no compatibility alias.

#### Subcommand mapping

| Current | Target | Analogue |
| --- | --- | --- |
| `nimbus service config` | `nimbus compose config` | `docker compose config` |
| `nimbus service up` | `nimbus compose up` | `docker compose up` |
| `nimbus service down` | `nimbus compose down` | `docker compose down` |
| `nimbus service list` | `nimbus compose ps` | `docker compose ps` |
| `nimbus service logs` | `nimbus compose logs` | `docker compose logs` |
| `nimbus service ps` | `nimbus compose top` | `docker compose top` |
| `nimbus service inspect` | `nimbus compose inspect` | explicit Nimbus extension |

#### Why there is no `compose ls` in this wave

Docker's `compose ls` lists Compose projects, not services within one project.
Nimbus does not yet expose a project-scoped multi-project listing surface for
Compose state, so `ls` should stay unused until there is a real project-list
concept.

#### Output contract

This plan keeps the already-landed distinction from the CLI alignment wave:

- `compose up` / `down`: concise action summaries by default
- `compose ps`: human table by default, explicit structured formats if retained
- `compose inspect`: structured output by default
- `compose logs`: direct log stream passthrough
- `compose top`: human process summary by default

That is already a better fit for current Nimbus architecture than pretending we
have Docker Compose's full live event model and attached log multiplexing on day
one.

#### `compose logs` stays single-service in v1

The current persisted-log path requires one service name up front and resolves
one log source at a time. The first rename wave should therefore keep
`compose logs <service>` required. Omitted-service behavior is follow-on work
only after Nimbus has a real multi-stream log mux instead of a single-service
poll loop.

#### Follow-on ergonomics worth adding

To feel more native to Docker users, this wave should also evaluate:

- `-f` as a short alias for `--file`
- `--timestamps` on logs
- explicit `--format` alignment with the settled machine/service output rules

#### Deferred richer Compose TTY output

The flashy Docker Compose v2 `[+] Running X/Y` TTY renderer is attractive, but
it should be treated as a follow-on slice only if Nimbus has a real event stream
or progress model to drive it. The current architecture supports concise action
summaries today; it does not yet have Docker's full resource-progress event
surface.

---

## Migration Taste

### Convex-leaning developer path

The happy path should feel like this:

```bash
nimbus dev
nimbus deploy --url http://my-nimbus.example.com
```

and, when needed:

```bash
nimbus codegen
nimbus start --app-dir ./my-app
```

Important honesty notes:

- generated `_generated/*` files should still be checked into version control,
  matching current Convex guidance
- the first `dev` milestone is bootstrap-only and must not pretend it is a live
  watched edit loop
- `deploy` must target an explicit self-hosted server URL, not an implicit
  cloud deployment identity

### Docker-leaning local services path

The local-service path should feel like this:

```bash
nimbus compose config
nimbus compose up
nimbus compose ps
nimbus compose logs api --follow
nimbus compose top api
nimbus compose down
```

That is much easier to learn than teaching Docker users that Nimbus `compose ls`
means status and Nimbus `compose ps` means process snapshots.

---

## Implementation Phases

### P1: Rename `service` -> `compose` with correct Docker-style subcommand names

**Status:** implemented in the current worktree; final workspace closeout
passed locally.

**Scope**

- rename the top-level namespace from `service` to `compose`
- rename current `list` to `ps`
- rename current `ps` to `top`
- keep `inspect` as an explicit Nimbus extension
- update help, examples, docs, and proofs

**Key rule**

Do not introduce `compose ls` in this phase.

**Verification**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- focused `cargo test -p nimbus-bin service_ -- --nocapture` updated for new
  command names
- direct help proof for:
  - `nimbus compose --help`
  - `nimbus compose ps --help`
  - `nimbus compose top --help`
  - `nimbus compose logs --help`

**Verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin compose::tests::parse_help -- --nocapture`
- `cargo test -p nimbus-bin compose::tests -- --nocapture`
- `cargo test -p nimbus-bin`

**Additional closeout notes**

- Current reference docs now use `nimbus compose ...` instead of the retired
  `nimbus service ...` public command surface:
  `docs/README.md`, `docs/reference/microvm-service-baseline.md`, and
  `docs/reference/macos-machine-flow.md`.
- The CLI implementation module was also renamed from
  `crates/nimbus-bin/src/service/` to `crates/nimbus-bin/src/compose/` so the
  code layout matches the public command surface. The Compose-file parser and
  lowerer live under `crates/nimbus-bin/src/compose/file/`, matching Docker's
  "Compose file" terminology while avoiding confusion with the
  `nimbus compose config` command. Domain references to individual Compose
  services remain where they describe Compose units or sandbox service
  internals.

**Post-review naming cleanup verification**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin compose::tests::parse_help -- --nocapture`
- `cargo test -p nimbus-bin compose::tests -- --nocapture`
- `cargo test -p nimbus-bin`
- `cargo run -p nimbus-bin -- service --help` rejects the legacy namespace
- `cargo run -p nimbus-bin -- compose --help` renders the Compose namespace

### P2: `nimbus dev` bootstrap slice

**Status:** implemented in the current worktree.

**Scope**

- add `DevCommand`
- auto-detect app dir
- initial codegen pass
- launch local server with development defaults and a project-local
  `./.nimbus/dev/` persistence root
- print local URL and app directory

**Important constraint**

This slice is startup-only. It does not watch files, multiplex live runtime
logs, or hot-activate new artifacts. The milestone description and help text
must say that clearly.

**Implementation notes**

- Added a first-class `DevCommand` on the root CLI.
- `nimbus dev` auto-detects an app root by walking upward from the current
  directory for `nimbus/`, `convex/`, or generated `.nimbus/convex/`
  artifacts, with current-directory fallback.
- The command wraps the existing `start` startup path instead of creating a
  second server lifecycle. It sets port `3210`, passes through `--app-dir`,
  runs the existing one-shot start codegen preflight unless `--skip-codegen`
  is set, and starts the server through `run_start_command`.
- The default dev persistence root is the resolved app directory's
  `.nimbus/dev/`, passed as both `data_dir` and `control_data_dir` for the
  shared v1 root. `--data-dir` explicitly overrides that shared root.
- `nimbus dev` forces embedded SQLite tenant persistence for the local dev
  wrapper, while preserving the existing start/runtime implementation beneath
  it.
- Startup output prints the local URL, app directory, and dev data root on
  `stderr`, and explicitly says the current slice has no watching, hot
  activation, or live log multiplexing.
- `docs/reference/cli.md` now documents the shipped bootstrap behavior and the
  current non-goals.

**Verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin dev::tests -- --nocapture`
- `cargo test -p nimbus-bin`
- `cargo test -p nimbus-server`
- `cargo test -p nimbus-engine`
- `cargo run -p nimbus-bin -- dev --help`

### P3: `nimbus dev` watch loop

**Status:** implemented in the current worktree.

**Scope**

- file watching for `nimbus/` or `convex/`
- debounced codegen reruns
- `--once`
- local activation of new artifacts after the generation-swap contract exists
- Convex-like log tailing controls

**Depends on**

- P2
- the activation semantics defined for deploy generations

**Implementation notes**

- Added `--once` to keep the P2 startup-only behavior available.
- Added `--tail-logs always|pause-on-sync|disable` as the Convex-like log-tail
  control surface, with help/output explicitly noting that live runtime log
  multiplexing is pending runtime log plumbing.
- Default `nimbus dev` now detects the selected `nimbus/` or `convex/` source
  root and runs a polling, debounced codegen watcher without adding a new
  dependency. The watcher ignores generated and build-output directories such
  as `_generated`, `node_modules`, `.nimbus`, `.next`, `dist`, and `build` so
  codegen output does not trigger itself.
- After P4 introduced the generation-swap seam, watched codegen now packages
  generated artifacts and activates them locally through the same deploy/admin
  API that `nimbus deploy` will use. `nimbus dev` injects an internal
  per-process deploy token into the local server and never asks developers to
  configure deploy credentials for the local loop.
- Activation output stays on `stderr`, reports the new generation, and may
  show a short human diff. Old in-flight requests keep their captured
  generation; new requests observe the activated generation after the swap.
- `docs/reference/cli.md` now documents the watched-codegen behavior, local
  activation, `--once`, `--tail-logs`, and the current non-goals.

**Verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin dev::tests -- --nocapture`
- `cargo test -p nimbus-bin deploy::tests -- --nocapture`
- `cargo test -p nimbus-bin`
- `cargo test -p nimbus-server`
- `cargo test -p nimbus-engine`
- `cargo run -p nimbus-bin -- dev --help`

### P4: Deploy control-plane design and server implementation

**Status:** implemented in the current worktree.

**Scope**

- document the deploy/admin API
- implement staging, validation, diffing, activation, and activation-time
  rollback behavior
- add server-side tests for generation swap and failure handling

**Depends on**

- none, but must complete before the CLI is considered implementation-ready

**Implementation notes**

- Started after P3. The immediate design target is a server-owned generation
  activation seam so deploy and future dev hot activation can update the app
  registry without restarting the process.
- Added `POST /api/admin/deploy`, disabled unless the server starts with
  `NIMBUS_DEPLOY_TOKEN` or a local dev workflow supplies an internal token.
  Requests authenticate with `Authorization: Bearer <token>`.
- Deploy requests stage `functions_json`, optional `http_routes_json`,
  optional `schema_json`, optional `auth_config_json`, and optional runtime
  bundle files into a temporary generated-app layout. Runtime bundles require
  both `bundle_mjs` and `bundle_sha256`.
- Staging validates manifest readability, HTTP route readability,
  schema/index definitions, auth config readability, and runtime bundle
  integrity before activation.
- The server computes function, HTTP route, schema, index, and runtime-bundle
  diffs. Dry-runs validate and diff without activation.
- Non-dry-run deploys atomically swap the active registry generation only after
  validation succeeds. Handlers capture an `Arc<ConvexRegistry>` at request or
  websocket entry, so in-flight work continues on the previous generation and
  new work observes the new generation.
- Validation or staging failure leaves the previous generation live. There is
  still no user-facing rollback command in v1.
- Added `docs/reference/deploy-admin-api.md` and updated
  `docs/reference/http-api.md` / `docs/reference/cli.md` for the server-side
  deploy contract.

**Verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-server`
- `cargo test -p nimbus-server deploy -- --nocapture`
- `cargo test -p nimbus-server`
- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-runtime`
- Exact reruns for transient parallel runtime failures:
  `cargo test -p nimbus-runtime runtime::tests::cooperative::runtime_cooperative_locker_slot_completes_immediate_async_host_work_without_parking_subprocess -- --ignored --exact --nocapture`
  and
  `cargo test -p nimbus-runtime runtime::tests::cooperative::warm_pool_cooperative_async_host_two_cycles_subprocess -- --ignored --exact --nocapture`
- `npm run docs:validate-refs:strict` was attempted, but this checkout has no
  such npm script.

### P5: `nimbus deploy` CLI

**Status:** implemented in the current worktree.

**Scope**

- add `DeployCommand`
- package app artifacts
- call the deploy/admin API
- render human diff output
- support `--dry-run`

**Depends on**

- P4

**Implementation notes**

- Added first-class `DeployCommand` on the root CLI.
- `nimbus deploy` requires an explicit target URL through `--url` or
  `NIMBUS_DEPLOY_URL`, and requires deploy credentials through `--token` or
  `NIMBUS_DEPLOY_TOKEN`.
- The command auto-detects the app directory with the same `nimbus/`,
  `convex/`, or generated `.nimbus/convex/functions.json` ancestor rules used
  by `nimbus dev`, with current-directory fallback.
- Deploy runs codegen first unless `--skip-codegen` is set, packages generated
  artifacts from `.nimbus/convex/`, and calls `POST /api/admin/deploy`.
- `--dry-run` validates and diffs without activation.
- Default output keeps phase/status lines on `stderr` and renders a concise
  human diff on `stdout` for functions, HTTP routes, schema/index changes, and
  runtime-bundle changes.
- `docs/reference/cli.md` now documents the shipped deploy command and flags.

**Verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin deploy::tests -- --nocapture`
- `cargo test -p nimbus-bin`
- `cargo run -p nimbus-bin -- deploy --help`

### P6: `start` startup polish

**Status:** implemented in the current worktree.

**Scope**

- improve `start` startup messaging without renaming the command
- keep the current operator contract intact

**Depends on**

- none

**Implementation notes**

- Post-review naming cleanup replaced the public `nimbus serve` command with
  `nimbus start` without retaining a legacy alias.
- The CLI implementation module moved from `crates/nimbus-bin/src/serve/` to
  `crates/nimbus-bin/src/start/`, with `StartCommand` and
  `run_start_command` naming so the code layout matches the public surface.
- `nimbus start` now emits a concise startup summary after the listener binds:
  local URL, server-owned scope, app directory/codegen state, optional Compose
  file, and deploy-admin API status when enabled.
- The command name and contract are unchanged. All existing `start` flags,
  including `--app-dir`, `--skip-codegen`, and `--compose-file`, remain in
  place.
- The startup summary does not print dashboard URLs.
- `docs/reference/cli.md` now documents the startup summary.

**Verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin start::tests::start_startup_summary_mentions_url_app_codegen_and_deploy_api -- --nocapture`
- `cargo run -p nimbus-bin -- serve --help` rejects the retired command
- `cargo run -p nimbus-bin -- start --help`

---

## Final Closeout

**Status:** complete in the current worktree.

**Implementation summary**

- `nimbus compose` is the public Compose-backed namespace; the legacy
  `nimbus service` public surface is not retained.
- `nimbus dev` now owns the Convex-migration local loop: app detection,
  project-local `.nimbus/dev/` persistence, codegen, local server startup,
  watched codegen, and local activation through the generation-swap seam.
- `POST /api/admin/deploy` now provides authenticated artifact staging,
  validation, diffing, activation, and activation-time rollback behavior.
- `nimbus deploy` packages generated artifacts, requires an explicit target and
  deploy token, supports `--dry-run`, and renders a clear human diff.
- `nimbus start` keeps its existing contract while printing a concise startup
  summary with URL, app/codegen state, optional Compose file, and deploy-admin
  API status.
- The retired public `nimbus serve` command is not retained as an alias.
- `docs/reference/cli.md`, `docs/reference/http-api.md`, and
  `docs/reference/deploy-admin-api.md` describe the shipped command and API
  behavior.
- Final naming review removed stale current-doc references to the retired
  `serve` surface from reference docs and active/future plans, while keeping
  historical proof rows explicitly labeled as historical evidence.
- Final implementation review aligned helper names with the public CLI surface:
  `start` now exposes `persistence_config_from_*` helpers, Compose command
  dispatch uses `run_compose_*` names, and engine-level `Service`/
  `ServicePersistenceConfig` naming remains only where it describes the core
  service domain type.
- Root help now describes Nimbus as a Convex-compatible reactive backend and
  gives `machine` a user-facing summary, so every top-level command has
  enterprise-grade first-touch copy.
- Archive closeout moved this completed control plan to
  `docs/plans/archive/cli-command-surface-plan.md`; `AGENTS.md` and
  `docs/plans/README.md` now point to it only for historical context.

**Full-wave verification completed before post-review `start` rename**

- `cargo fmt --all --check`
- `cargo check --workspace`
- `make clippy`
- `make test`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci`

**Post-review `start` rename verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin start::tests -- --nocapture`
- `cargo test -p nimbus-bin dev::tests -- --nocapture`
- `cargo test -p nimbus-bin`
- `cargo run -p nimbus-bin -- start --help`
- `cargo run -p nimbus-bin -- serve --help` rejects the retired command
- `cargo run -p nimbus-bin -- --help`

**Final naming/DX review verification completed in this worktree**

- `cargo fmt --all --check`
- `cargo check -p nimbus-bin`
- `cargo test -p nimbus-bin start::tests -- --nocapture`
- `cargo test -p nimbus-bin compose::tests -- --nocapture`
- `cargo test -p nimbus-bin`
- `cargo run -p nimbus-bin -- --help`
- `cargo run -p nimbus-bin -- start --help`
- `cargo run -p nimbus-bin -- compose --help`
- `cargo run -p nimbus-bin -- serve --help` rejects the retired command
- Archive closeout reran `cargo fmt --all --check`, `cargo check -p
  nimbus-bin`, and `cargo test -p nimbus-bin` after moving the plan and
  updating `AGENTS.md` / `docs/plans/README.md`.
- Current reference docs and active/future plans no longer present `nimbus
  serve` or `nimbus service` as current public commands; remaining hits are
  retired-command notes or labeled historical proof rows.
- `crates/nimbus-bin/src/start/` no longer exposes stale
  `service_persistence_config_*` helper names.
- `crates/nimbus-bin/src/compose/` command dispatch now uses
  `run_compose_*` helper names for public command handling, while service
  lifecycle names remain only for Compose-managed workload concepts.

**Verification notes**

- The first `make ci` attempt hit an environment lock under the Cargo advisory
  database outside the workspace. It was rerun with approved escalation and
  passed.
- The post-review `serve` -> `start` rename is covered by focused CLI gates and
  the full `nimbus-bin` test crate. Workspace-wide `make ci` was not rerun
  after this naming-only follow-up.
- `npm run docs:validate-refs:strict` was attempted during P4 documentation
  work, but this checkout has no such npm script. The limitation is
  environmental/package-script availability, not a code or docs failure.

---

## Verification Contract

Every landed phase must pass:

1. `cargo fmt --all --check`
2. `make clippy`
3. `make test`
4. targeted direct CLI help/output proof for the affected commands
5. docs update in `docs/reference/cli.md`
6. when command behavior changes materially on the shipped macOS flow, capture an
   isolated-root proof bundle similar to the completed machine/service follow-on
   wave

---

## Decisions And Constraints

1. **`start` is the server-start verb.**
   Do not retain `serve`, do not spend `run` on server startup, and do not
   rename it to `agent` in this plan.
2. **`compose ps` is the status summary surface.**
   `compose top` is the process-detail surface.
3. **There is no `compose ls` in v1.**
   Save that name for a true project-list concept if one lands later.
4. **`dev` is the Convex-migration happy path.**
   The command must feel simple and local-first.
5. **`deploy` is explicit-target in v1.**
   Self-hosted Nimbus should not pretend there is a cloud deployment identity
   when there is really a server URL plus admin credentials.
6. **Do not print a dashboard URL until one exists.**
7. **Formatting is part of the deliverable, but semantics come first.**
   Native-feeling command names matter more than copying another tool's spinner
   frame-for-frame.
8. **`dev` uses a project-local `./.nimbus/dev/` root in v1.**
   Do not reuse `start`'s default `./data` root for the watched local workflow.
9. **The first `dev` slice was bootstrap-only.**
   P2 did not pretend edits were live. P3 now uses P4's generation-swap
   contract for local activation after watched codegen succeeds.
10. **`compose logs` remains single-service in the rename wave.**
    Do not fake Docker-style multiplexing with the current one-service polling
    implementation.
11. **Deploy v1 exposes activation-time rollback behavior, not a rollback
    command.**
    Keep the previous generation live on activation failure, but defer
    user-facing rollback verbs until the control plane has real generation
    history semantics.

---

## Resolved Before Implementation

Reviewing the current codebase resolves the four questions that were still open
in the first draft of this plan:

1. **`dev` defaults to a project-local `./.nimbus/dev/` root.**
   The current `start` path already couples `data_dir` and `control_data_dir`
   by default, so the clean v1 move is to keep that coupling but relocate the
   local dev workflow under a project-local hidden directory instead of sharing
   `start`'s `./data` default.
2. **The first `dev` milestone did not include live artifact activation.**
   At P2 time the server startup path built a static `ConvexRegistry` from
   `.nimbus/convex/` at boot and did not expose a generation-swap seam, so P2
   could not honestly promise watched edits were live. P3 now uses the P4
   deploy/admin generation-swap seam for local activation.
3. **`compose logs` stays single-service in v1.**
   The current CLI parser and persisted-log implementation both require one
   service name up front and stream one log source at a time.
4. **Deploy v1 includes internal rollback at activation time only.**
   The current repo does not yet expose retained generation history, so an
   explicit rollback command would be a promise ahead of architecture.
