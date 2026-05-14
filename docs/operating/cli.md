# CLI Reference

Nimbus serves HTTP/WebSocket traffic through an explicit `start` subcommand and
also exposes compose-backed service and machine management subcommands.

Current shipped CLI shape:

```bash
nimbus dev [--app-dir PATH] [--port 3210] [--data-dir ./.nimbus/dev] [--once] [--skip-codegen] [--tail-logs always|pause-on-sync|disable] [--compose-file PATH]...
```

```bash
nimbus deploy [--url URL] [--token TOKEN] [--app-dir PATH] [--dry-run] [--skip-codegen] [--verbose]
```

```bash
nimbus codegen [--app PATH]
```

```bash
nimbus init <ADAPTER> [DIRECTORY] [--source-root convex] [--install]
```

```bash
nimbus token rotate
```

```bash
nimbus start [flags]
```

`nimbus dev` starts the local development path with dev defaults, watched
codegen reruns, and local activation. `nimbus deploy` pushes generated app
artifacts to an explicit self-hosted target. `nimbus codegen` is the
first-party artifact-generation command. `nimbus start` is the foreground
server-start path. The retired `nimbus serve` command is not retained as a
compatibility alias.

Current shipped compose-management commands:

```bash
nimbus compose config [--file PATH]... [--services]
```

```bash
nimbus compose up [service] [--file PATH]... [--tenant <tenant-id>]
```

```bash
nimbus compose down [service] [--file PATH]... [--tenant <tenant-id>]
```

```bash
nimbus compose ps [-f json|yaml|table] [--noheading] [--file PATH]... [--all-tenants]
```

```bash
nimbus compose inspect <service> [-f json|yaml] [--file PATH]... [--tenant <tenant-id>]
```

```bash
nimbus compose logs <service> [--file PATH]... [--tenant <tenant-id>] [--follow]
```

```bash
nimbus compose top <service> [-f json|yaml|table] [--noheading] [--file PATH]... [--tenant <tenant-id>]
```

Current shipped machine commands:

```bash
nimbus machine init [--cpus N] [--memory N] [--disk-size N] [--image SOURCE] [--identity PATH] [--ignition-path PATH] [--firmware PATH] [--volume HOST:GUEST] [--now] [NAME]
```

```bash
nimbus machine start [--cpus N] [--memory N] [--disk-size N] [--image SOURCE] [--identity PATH] [--ignition-path PATH] [--firmware PATH] [--volume HOST:GUEST] [--quiet] [--no-info] [NAME]
```

```bash
nimbus machine stop [NAME]
```

```bash
nimbus machine status [-f json|yaml|table] [--noheading] [--quiet] [NAME]
```

```bash
nimbus machine list [-f json|table] [--noheading] [--quiet]
```

```bash
nimbus machine ls [-f json|table] [--noheading] [--quiet]
```

```bash
nimbus machine info [-f json|yaml] [--format json|yaml]
```

```bash
nimbus machine inspect [-f json|yaml] [NAME]
```

```bash
nimbus machine set [--cpus N] [--memory N] [--disk-size N] [NAME]
```

```bash
nimbus machine cp [--quiet] SRC_PATH DEST_PATH
```

```bash
nimbus machine ssh [NAME] [COMMAND...]
```

```bash
nimbus machine rm [NAME]
```

```bash
nimbus machine os apply <oci-ref-or-digest> [--restart]
```

```bash
nimbus machine os upgrade [--dry-run] [--restart]
```

```bash
nimbus machine os rollback [--restart]
```

```bash
nimbus start [--host 127.0.0.1] [--port 8080] [--app-dir ./app] [--skip-codegen] [--compose-file PATH]...
```

For local development from source:

```bash
cargo run -p nimbus-bin -- start [flags]
```

Current command taxonomy:

- `nimbus dev`
  shipped local development server with Node.js-backed codegen, auto
  `npm install` when declared packages are missing locally, auto-tenant
  creation (`demo`), one-shot startup codegen, debounced watched codegen
  reruns, local generation activation, and development persistence defaults
- `nimbus deploy`
  shipped explicit-target deploy command for validating, diffing, and
  activating generated app artifacts on a running server
- `nimbus codegen`
  shipped first-party code generation for `nimbus/` or `convex/` source roots
- `nimbus init <adapter>`
  shipped project scaffold command; requires an adapter argument (e.g.
  `convex`). Creates a starter project with schema, example functions,
  `package.json`, `tsconfig.json`, and `.gitignore` in the target directory.
  Skips existing files without overwriting and stops after scaffolding by
  default. Pass `--install` to bootstrap adapter dependencies immediately.
- `nimbus token rotate`
  shipped local admin token lifecycle command for rotating the localhost server
  access token using live-server semantics when a server is discoverable and
  offline semantics otherwise
- `nimbus start`
  shipped explicit server-start verb
- `nimbus compose ...`
  shipped Compose-backed service lifecycle namespace
- `nimbus machine ...`
  shipped macOS machine lifecycle namespace

## Bootc Default Promotion And Rollback

The current macOS default is the Nimbus-owned bootc image promoted in BMD6:

```text
docker://ghcr.io/nimbus/machine-os:v0.1.30@sha256:f56553e212d2e077d8bedc1db902283f6e12315a621d6046b03d1cb43a0eb08d
```

Future direct bootc artifacts are default-promotable only when all of these
are true:

1. A paired `nimbus/nimbus` release and `nimbus/machine-os` release exist for
   the same tag.
2. The machine-os release assets include the AppleHV OCI layout, checksums,
   SBOM, digest evidence, bootc build summary, and artifact attestations.
3. A real macOS release-candidate VM has been booted from that artifact and
   captured with `scripts/collect-nimbus-machine-guest-proof.sh`, including
   package context, SELinux context, and AVC evidence.
4. The captured proof uses the host-side AVC checker from
   `/Users/jack/src/github.com/nimbus/machine-os/scripts/check-selinux-avcs.sh`.
5. The final composed gate passes:

```bash
bash scripts/verify-bootc-default-promotion-gate.sh \
  --release-dir <downloaded-machine-os-release-assets> \
  --guest-proof-dir <macos-proof-dir> \
  --expected-tag vX.Y.Z
```

Only after that gate passes should the checked-in macOS default image digest
move to a new Nimbus-owned bootc artifact.

Rollback has two supported shapes. For a healthy bootc-native machine, use
`nimbus machine os rollback --restart` so the guest stages the previous bootc
deployment and reboots through the normal readiness path. If the guest machine
API cannot answer, or if the fleet default itself must move back to a prior
artifact, use `nimbus machine os apply <previous-digest> --restart` or an
explicit repair/recreate flow. The Podman-compatible path stays
available only as an explicit compatibility/repair override until the
legacy-removal phase deliberately removes or further demotes it.

## Core Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--host` | `127.0.0.1` | host interface to listen on; use an explicit non-loopback value only when intentionally exposing the local server beyond localhost |
| `--port` | `8080` | port to listen on |
| `--data-dir` | `./data` | data directory for tenant databases |
| `--app-dir` | unset | optional app directory whose user source root may be `nimbus/` or `convex/`; generated runtime artifacts still live under `.nimbus/convex/` |
| `--skip-codegen` | `false` | skip the one-shot startup-time codegen preflight when `--app-dir` is set |
| `--license-file` | unset | optional explicit path to a Nimbus license file |

If `--license-file` is not provided, Nimbus next checks
`NIMBUS_LICENSE_FILE`, then `~/.config/nimbus/license.json`, and otherwise
falls back to the built-in community license.

## Encryption Flags

Local encryption for `nimbus start` is controlled with
`--encryption-key-provider`:

- `master-key-file`
  single 32-byte key file that wraps per-subject DEKs
- `key-dir`
  directory of per-subject wrapping keys
- `aws-kms`
  enterprise-managed wrapping provider using AWS KMS over the shared
  manifest-backed DEK envelope

Provider-specific flags:

| Flag | Meaning |
| --- | --- |
| `--encryption-master-key-file` | path to the 32-byte master key file |
| `--encryption-key-dir` | path to the key directory containing per-subject wrapping keys |
| `--encryption-aws-kms-key-id` | AWS KMS key ID or alias (e.g., `alias/nimbus`) |
| `--encryption-aws-region` | AWS region for KMS |
| `--encryption-aws-endpoint-url` | optional KMS endpoint URL (for LocalStack) |

Environment variables: `NIMBUS_ENCRYPTION_KEY_PROVIDER`,
`NIMBUS_ENCRYPTION_MASTER_KEY_FILE`, `NIMBUS_ENCRYPTION_KEY_DIR`,
`NIMBUS_ENCRYPTION_AWS_KMS_KEY_ID`, `NIMBUS_ENCRYPTION_AWS_REGION`,
`NIMBUS_ENCRYPTION_AWS_ENDPOINT_URL`.

`nimbus encryption ...` admin commands read the current provider and
persistence settings from environment variables and config-file resolution.
`rotate-kek` additionally accepts replacement-provider flags on the
subcommand itself.

See [Encryption at rest reference](encryption.md) for operational guidance.

## Persistence Flags

The tenant persistence mode is selected with `--tenant-provider`:

- `sqlite`
  embedded SQLite with one tenant file per tenant
- `redb`
  retained embedded redb provider
- `libsql-replica`
  libsql remote-primary provider family with a provider-owned local SQLite
  derivative cache
- `postgres`
  Postgres-backed external tenant persistence
- `mysql`
  MySQL-backed external tenant persistence

Provider-specific flags:

| Flag | Meaning |
| --- | --- |
| `--control-data-dir` | optional override for the local redb control-plane directory |
| `--libsql-url` | canonical libsql primary URL for `--tenant-provider=libsql-replica` |
| `--libsql-auth-token` | optional auth token for the libsql primary |
| `--libsql-admin-url` | required libsql admin/provisioning API URL for namespace lifecycle when `--tenant-provider=libsql-replica` |
| `--libsql-admin-auth-header` | optional `Authorization` header value for the libsql admin API |
| `--libsql-metadata-namespace` | provider metadata namespace for libsql replica routing |
| `--libsql-tenant-namespace-prefix` | prefix used when deriving per-tenant libsql namespaces |
| `--libsql-replica-cache-dir` | provider-owned local cache root for embedded derivative replica files |
| `--postgres-url` | canonical Postgres resource URL |
| `--postgres-metadata-schema` | provider metadata schema for Postgres routing |
| `--postgres-tenant-schema-prefix` | prefix used when deriving per-tenant Postgres schemas |
| `--postgres-min-connections` | optional minimum Postgres pool size |
| `--postgres-max-connections` | optional maximum Postgres pool size |
| `--mysql-url` | canonical MySQL resource URL |
| `--mysql-metadata-database` | provider metadata database for MySQL routing |
| `--mysql-tenant-database-prefix` | prefix used when deriving per-tenant MySQL databases |
| `--mysql-min-connections` | optional minimum MySQL pool size |
| `--mysql-max-connections` | optional maximum MySQL pool size |

The same values can also be supplied through the corresponding `NIMBUS_*`
environment variables or the JSON config file referenced by `--config` or
`NIMBUS_CONFIG`.

## Runtime Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--runtime-heap-mb` | `128` | V8 heap limit per isolate in megabytes |
| `--runtime-initial-heap-mb` | `8` | initial V8 heap size per isolate in megabytes |
| `--runtime-timeout-secs` | `30` | maximum wall-clock runtime invocation time |
| `--runtime-max-instances` | available hardware parallelism | maximum concurrent top-level runtime instances |
| `--runtime-max-nested-calls` | `64` | maximum nested `ctx.run*` invocations per request tree |

## Shared Compose Discovery

`nimbus compose ...`, `nimbus dev`, and `nimbus start` now share one Compose
discovery rule:

- explicit repeated `--file` or `--compose-file` flags win and load exactly
  those files in the order provided
- when explicit flags are absent, `COMPOSE_FILE` provides an ordered explicit
  file list; `COMPOSE_PATH_SEPARATOR` overrides the platform default separator
  (`:` on macOS/Linux, `;` on Windows)
- without explicit flags or `COMPOSE_FILE`, Nimbus searches the current
  directory first and
  then parent directories for `compose.yaml`, `compose.yml`,
  `docker-compose.yaml`, or `docker-compose.yml`
- when auto-discovery selects canonical `compose.yaml` and
  `compose.override.yaml` exists beside it, both files are loaded in that
  order as one logical Compose project
- if both docker-compose filenames exist in the same directory, Nimbus fails with an
  actionable error instead of guessing
- relative paths in explicit flag lists and `COMPOSE_FILE` entries resolve from
  the current working directory, and `files[0]` remains the project identity
  plus relative-path anchor for merged Compose semantics
- `--app-dir` is separate: it selects the app/codegen context and does not
  redefine Compose discovery

## Dev Command

`nimbus dev` is the local development happy path. In the current watch-loop
slice it:

- requires Node.js 22 with `npm` for Convex and Cloud Functions authoring
  because startup codegen still runs through external `node` by default, and
  the external authoring path verifies the `node --version` baseline before it
  executes
- auto-detects the app directory from the current directory by looking for a
  `nimbus/` or `convex/` source root, `firebase.json`, or
  `@google-cloud/functions-framework` in `package.json`, falling back to the
  current directory
- when no compatible adapter is detected and `--skip-codegen` is not set, exits
  with guidance to run `nimbus init convex` or `nimbus init cloud-functions`
- when `package.json` exists and declared dependencies or devDependencies are
  missing from `node_modules/`, or when the recorded dependency fingerprint no
  longer matches `package.json` plus the npm lockfile (`package-lock.json` or
  `npm-shrinkwrap.json` when present), automatically runs `npm install` before
  codegen (for Cloud Functions, this runs in the `functions/` subdirectory or,
  for Firebase multi-codebase projects, in each declared `functions[].source`
  package root)
- records the current tooling dependency fingerprint in
  `.nimbus/cache/node/dependency-state.json` inside the app or functions
  directory so future runs can distinguish “already installed and current” from
  “installed but stale after manifest/lockfile changes”
- auto-creates a `demo` tenant on startup so a Convex client can connect to
  `http://localhost:3210/convex/demo` immediately; silently reuses the tenant
  on subsequent runs
- runs one initial codegen pass unless `--skip-codegen` is set
- accepts `--debug-node-apis` to diagnose Convex modules that import Node.js
  builtins without `"use node"`; this mirrors Convex's debug flow and recognizes
  both bare and `node:` builtin specifiers such as `fs` and `node:fs`
- validates Convex Node action package imports against `node.externalPackages`;
  explicit package names and `["*"]` are supported, while package imports that
  would require unimplemented bundling fail before startup with install/config
  guidance
- starts the same local server path as `nimbus start`

The embedded codegen runner exists only as an experimental pilot behind
`NIMBUS_EXPERIMENTAL_EMBEDDED_CODEGEN`. Convex-compatible Node action runtime
execution supports configured Node20, Node22, and Node24 targets through
`convex.json`, with Node22 as the default. Firebase / Cloud Functions package
layouts still fall back to the external Node.js runner; the embedded pilot does
not yet support that structure.
- watches the selected `nimbus/` or `convex/` source root for source changes
  and reruns codegen after a short debounce
- validates and locally activates regenerated artifacts through the deploy
  generation-swap path after watched codegen succeeds
- listens on port `3210` by default
- uses a shared project-local persistence root at `./.nimbus/dev/` by default
  for both tenant data and local control state
- writes `NIMBUS_DEPLOYMENT=local:<slug>` to `.env.local` in the app directory
  on startup, where `<slug>` is derived from the directory name and a hash of
  the canonical path; respects existing `.env.local` content and only updates
  the `NIMBUS_DEPLOYMENT` line
- prints the local URL, deployment identity, app directory, persistence root,
  watched source root, and resolved Compose selection when one is active

Current watch-loop scope:

- watched codegen activates generated artifacts locally after validation; old
  in-flight requests keep their captured generation and new requests see the
  new generation after activation
- it does not multiplex live runtime logs
- it does not print a dashboard URL

Flags:

| Flag | Default | Meaning |
| --- | --- | --- |
| `--port` | `3210` | local development server port |
| `--app-dir` | auto-detected | app directory whose user source root may be `nimbus/` or `convex/`; generated runtime artifacts still live under `.nimbus/convex/` |
| `--data-dir` | `./.nimbus/dev/` under the resolved app directory | shared local dev persistence root for tenant data and control state |
| `--once` | `false` | run startup only and disable the watched codegen loop |
| `--skip-codegen` | `false` | skip the initial codegen pass and use already-generated artifacts |
| `--tail-logs` | `pause-on-sync` | accepted log-tail mode (`always`, `pause-on-sync`, or `disable`); live runtime log multiplexing remains pending runtime log plumbing |
| `--compose-file` | unset | optional explicit ordered Compose path list for local service dependencies; repeat the flag to merge overlays in order. When omitted, `dev` uses `COMPOSE_FILE` if set, then the shared cwd/parent discovery rule |

## Init Command

`nimbus init` scaffolds a new Nimbus project in the target directory. The
adapter argument is required — it selects which project template to create.
Starter files are written without overwriting anything that already exists. By
default, `init` stops after scaffolding. Pass `--install` to bootstrap adapter
dependencies immediately.

**Convex adapter** (`nimbus init convex`):

- `convex/schema.ts` — messages table with author/body fields and a `by_author` index
- `convex/messages.ts` — list query and send mutation
- `package.json` — project dependencies (`convex`, `@nimbus/codegen`)
- `tsconfig.json` — TypeScript configuration for ESNext/bundler
- `.gitignore` — ignores `.nimbus/` and `node_modules/`

**Cloud Functions adapter** (`nimbus init cloud-functions`):

- `firebase.json` — Firebase project config pointing to `functions/`
- `functions/package.json` — dependencies (`firebase-functions`, `firebase-admin`, `@nimbus/codegen`)
- `functions/tsconfig.json` — TypeScript configuration for Node.js
- `functions/src/index.ts` — starter HTTP and Firestore trigger handlers
- `.gitignore` — ignores `.nimbus/`, `node_modules/`, and `lib/`

With `--install`, both adapters run `npm install` after scaffolding when
declared packages are missing locally. For Cloud Functions, npm install runs in
the `functions/` subdirectory. If the optional install step fails, the
scaffolded project is preserved and you can recover by running `nimbus dev` or
`npm install` manually from the project directory.

If the target directory already has the adapter's marker files (`convex/` or
`nimbus/` for Convex, `firebase.json` for Cloud Functions), `init` exits with
an error and suggests `nimbus dev` instead.

Arguments and flags:

| Argument / Flag | Default | Meaning |
| --- | --- | --- |
| `ADAPTER` | *(required)* | adapter to scaffold (`convex`, `cloud-functions`) |
| `DIRECTORY` | `.` | target directory (created if it does not exist) |
| `--source-root` | `convex` | source root directory name (convex adapter only); `nimbus` is experimental and not yet supported |
| `--install` | `false` | install adapter dependencies after scaffolding |

Examples:

```bash
nimbus init convex my-app
cd my-app
nimbus dev
```

```bash
nimbus init cloud-functions my-functions-app
cd my-functions-app
nimbus dev
```

```bash
nimbus init convex
nimbus dev
```

## Codegen Behavior

`nimbus codegen` generates two classes of artifacts from the selected app
directory:

- `_generated/*` files under the detected `nimbus/` or `convex/` source root
- runtime manifests and bundle files under `.nimbus/convex/`
- Convex Node external package evidence at
  `.nimbus/convex/node_external_packages.json` plus staged package roots under
  `.nimbus/convex/node_modules/` when `node.externalPackages` is used

Equivalent entrypoints:

- `nimbus codegen --app ./my-app`
- `npx convex codegen --app ./my-app`
- `npx nimbus-codegen --app ./my-app`

The shared pipeline still expects generated files to be checked into version
control for stable typechecking and frontend workflows. The start-side
preflight described below is a startup convenience, not a watched `dev` loop.

From the repo root, the canonical JS verification entrypoints are now:

- `npm run typecheck`
- `npm run test`
- `npm run build`

Those commands fan out to workspace-owned package scripts where present, so the
root command surface stays stable even when individual package internals
change. The `@nimbus/codegen` typecheck lane is a JS parser and
codegen-boundary guardrail check because the package is implemented as `.mjs`
rather than TypeScript.

## Deploy Command

`nimbus deploy` packages generated app artifacts and sends them to a running
Nimbus server. The target is explicit in v1: use `--url` or
`NIMBUS_DEPLOY_URL`. Authentication uses `--token` or `NIMBUS_DEPLOY_TOKEN`,
matching the token configured on the server.

Default behavior:

- auto-detects the app directory from the current directory by looking for a
  `nimbus/` or `convex/` source root, falling back to the current directory
- runs codegen before packaging unless `--skip-codegen` is set
- packages generated artifacts from `.nimbus/convex/`
- uploads to `POST /api/admin/deploy`
- prints a concise human diff for functions, HTTP routes, schema/index
  changes, and runtime-bundle changes

Flags:

| Flag | Default | Meaning |
| --- | --- | --- |
| `--url` | `NIMBUS_DEPLOY_URL` or required | explicit target Nimbus server URL |
| `--token` | `NIMBUS_DEPLOY_TOKEN` or required | deploy admin bearer token |
| `--app-dir` | auto-detected | app directory whose generated artifacts live under `.nimbus/convex/` |
| `--dry-run` | `false` | validate and diff without activating a new generation |
| `--skip-codegen` | `false` | package existing generated artifacts without running codegen |
| `--verbose` | `false` | show extra packaging phase detail |

## Deploy Admin API

Start the server with `NIMBUS_DEPLOY_TOKEN` to enable
`POST /api/admin/deploy`; callers authenticate with
`Authorization: Bearer <token>`.

The endpoint accepts generated app artifacts, stages them into a temporary app
directory, validates manifests, schema/index definitions, auth config, and
runtime bundle integrity, then returns a human-renderable diff. Dry-runs
validate and diff without activation. Non-dry-run requests atomically activate
the new app generation only after validation succeeds, so in-flight requests
continue on the generation they already captured while new requests observe the
new generation after activation.

See [deploy-admin-api.md](deploy-admin-api.md) for the request and response schema.

## Startup Behavior

Nimbus requires an explicit subcommand. `nimbus start` starts the server. On
startup, it:

- initializes tracing
- prints a concise startup summary with the local URL, server-owned scope,
  app directory/codegen state, optional Compose selection, and deploy-admin
  status
- when `--app-dir` is set and `--skip-codegen` is not, runs one codegen
  preflight pass before loading manifests
- loads the service with the configured data directory
- loads tenants with scheduled work
- starts the scheduler loop
- optionally loads the Convex-compatible registry from `--app-dir`
- optionally loads declared sandbox-backed services from the shared Compose
  discovery contract
- loads license state from the explicit path, environment, default path, or
  built-in community defaults

When a Compose selection is present, Nimbus validates it through the same
adapter used by `nimbus compose config`, lowers it into a typed
declared-service catalog, and wires that catalog into the server-owned sandbox
manager. With `--app-dir`, ready declared-service bindings appear in
`ctx.services.<name>`, and missing bindings can be activated explicitly through
`await ctx.services.get("<name>")`. The app directory may use `nimbus/` as the
native user source root or `convex/` as the compatibility root; in both cases
the runtime registry still loads the generated manifests from
`.nimbus/convex/`. The
explicit `nimbus compose up/down/...` commands share that same Compose
discovery, lowering, deterministic project identity, and project-scoped
backend root instead of inventing a second lifecycle control plane.

The `start` preflight is intentionally one-shot. After startup, Nimbus does not
watch the filesystem or regenerate `_generated/*` as functions change. If you
want watched edit-loop behavior, that remains follow-on work.

On macOS, if that Compose project resolves to the forwarded guest container
backend and the default machine is initialized but stopped, `nimbus start`
now starts the machine first and only then wires the forwarded guest manager.

This is why `start` and `compose` are not the same concept:

- server startup owns the Nimbus API surface itself: HTTP, WebSocket,
  Convex-compatible routes, runtime execution, and the `ctx.services` snapshot
  plus activation surface
- `compose` commands manage declared backing workloads that Nimbus may start,
  stop, inspect, or log

`machine` is a third concept:

- `machine` commands manage the macOS Linux-guest envelope that Nimbus will use
  for developer workflows on Apple Silicon
- the shipped MAC2 surface owns persisted machine config, typed path layout,
  CLI/state-model wiring, and the initial direct `krunkit` + `gvproxy`
  machine-manager seam
- guest-image/bootstrap completion, packaging behavior, and transparent
  developer UX are documented in [macos-machine-flow.md](../architecture/sandbox/macos-machine-flow.md)

Output-shaping contract:

- human summary commands use compact tables by default
- `-f` and `--format` are equivalent where structured output is supported
- `--noheading` only affects table-producing human output
- `nimbus machine list` marks the default machine with `*` in human table
  output and sorts active machines ahead of inactive ones
- Go-template output is intentionally not supported in the current shipped CLI;
  `json` and `yaml` remain the stable machine-readable formats

## Compose Commands

The current landed compose-control surface exposes shared-discovery Compose
validation plus explicit lifecycle control:

| Command | Meaning |
| --- | --- |
| `nimbus compose config` | validate the discovered Compose project, the `COMPOSE_FILE` selection when present, or an explicit ordered `--file` list, and print the resolved service plan as YAML |
| `nimbus compose config --file ./stack.yml --file ./stack.dev.yml` | validate a specific ordered Compose file list |
| `nimbus compose config --services` | list only service names, one per line |
| `nimbus compose up` | start all declared services for the deterministic local project tenant and print a concise action summary instead of a YAML lifecycle dump; active current services are reported inline as `already_running` |
| `nimbus compose up db --tenant tenant-a` | start only service `db` for an explicit tenant and print the same action-summary contract |
| `nimbus compose down` | stop one current persisted sandbox per service identity for the deterministic local project tenant and print a concise action summary instead of a YAML lifecycle dump |
| `nimbus compose down db` | stop the current persisted sandbox for service `db` in the deterministic local project tenant with the same action-summary contract |
| `nimbus compose ps` | list persisted sandbox state for the deterministic local project tenant in a human table by default; use `-f json` or `-f yaml` for structured output, or `--noheading` to suppress table headers |
| `nimbus compose ps --all-tenants` | list all persisted sandboxes under the project-scoped backend root with the same table-vs-structured contract |
| `nimbus compose inspect db` | inspect persisted sandbox details for service `db` in the deterministic local project tenant as JSON by default; `-f yaml` is the explicit structured alternative |
| `nimbus compose inspect db --tenant tenant-a` | inspect persisted sandbox details for service `db` in an explicit tenant with the same structured contract |
| `nimbus compose logs db` | print the persisted `ctr.log` for service `db` in the deterministic local project tenant |
| `nimbus compose logs db --follow` | keep polling the persisted `ctr.log` for appended output |
| `nimbus compose top db` | show the persisted PID snapshot and matching host `ps` rows for service `db` as a human summary by default; use `-f json` or `-f yaml` for structured output, or `--noheading` to suppress the process table headings |

Current scope:

- validates `image` and `build` sources
- resolves `environment` plus `env_file`
- validates lowerable `command`, `entrypoint`, `working_dir`, and `user`
  process overrides
- validates lowerable lifecycle settings such as `restart` and
  `stop_grace_period` against the generic sandbox lifecycle seam
- validates the declared-service catalog handoff used by both the server-owned
  service manager and the explicit `compose up` launch path
- uses the same cwd/parent discovery contract for `compose`, `dev`, and
  `start`, while keeping project identity anchored on the primary Compose file
- lowers `ports`, restart policy, and CPU/memory limits into the resolved plan
- preserves `depends_on`, `healthcheck`, `volumes`, labels, and `x-nimbus`
  metadata for lifecycle commands and recovery drills
- warns on ignored fields such as `networks`, `privileged`, and `logging`
- resolves lifecycle commands against backend-owned persisted sandbox state
  under the project-scoped `control_data_dir`, not a separate CLI-owned
  service database: krun manifests/logs locally for krun-backed projects, and
  forwarded guest container manifests/logs on macOS for container-backed
  projects
- on macOS, container-backed `nimbus compose up/down/ps/inspect/logs/top`
  commands now route through the forwarded guest machine API and guest
  container-manifest state instead of assuming host-local krun state
- mixed-backend project-wide Compose operations still reject because
  `nimbus compose ...` requires one backend family per project-wide command
- the server startup path can now load container-backed Compose managers on
  macOS through the forwarded guest machine API; if the default machine is
  initialized but stopped, `nimbus start` now starts it first under the same
  machine convergence contract, then requires `service_execution_ready`
- derives a deterministic local project tenant id from the Compose project key,
  and uses that tenant by default for `compose up` / `compose down` /
  `compose ps` / `compose inspect` / `compose logs` / `compose top`

## Machine Commands

The current landed machine surface is the checked-in macOS machine contract:

| Command | Meaning |
| --- | --- |
| `nimbus machine init [NAME]` | write the named machine config and state files, record the guest resource contract, and optionally start immediately with `--now`; omitting `NAME` still targets `default` and action commands print a concise success summary instead of the full structured status view |
| `nimbus machine start [NAME]` | start the named machine, creating it with defaults first when it does not already exist, then wait for machine-ready, guest SSH, and forwarded machine-API reachability while emitting human progress updates for slow first-start convergence on stderr; `--quiet` suppresses the phase chatter but still prints the final success summary, while `--no-info` suppresses advisory `info:` notices without hiding the normal phase banners |
| `nimbus machine stop [NAME]` | stop the named machine helpers, including stale-helper recovery, and persist the stopped machine state before printing a concise success summary |
| `nimbus machine status [NAME]` | print the current named machine status in table form by default, emit the full structured view with `-f json` / `-f yaml`, suppress table headings with `--noheading`, or print the machine name only with `--quiet` |
| `nimbus machine list` / `nimbus machine ls` | list initialized machines from the Nimbus config root in a compact table by default; active machines sort ahead of inactive ones, the default machine is marked with `*`, `--quiet` stays names-only when no explicit `-f` is requested, `--noheading` suppresses table headers, and `-f json` exposes the same `default` bit structurally |
| `nimbus machine info` | print host-level machine information in YAML by default, or JSON with `-f json`; this is the operator-facing summary for the machine roots, cache locations, current `nimbus` machine release, and the default-machine summary contract |
| `nimbus machine inspect [NAME]` | print the persisted config plus refreshed state record for the named machine as JSON by default, or YAML with `-f yaml` |
| `nimbus machine set [NAME]` | update the recorded CPU, memory, or disk contract for a stopped named machine without recreating it |
| `nimbus machine cp [--quiet] SRC_PATH DEST_PATH` | recursively copy files or directories between the host and a running machine using Podman-style `NAME:/path` guest endpoints and the machine's configured SSH contract |
| `nimbus machine ssh [NAME] [COMMAND...]` | run a command through the configured guest SSH user and identity once the machine is running; as with Podman, if the first argument names an existing machine it is treated as `NAME`, otherwise it is passed through as the guest command on `default` |
| `nimbus machine rm [NAME]` | remove the persisted config, state, and short runtime-root layout for the named machine when it is not running |
| `nimbus machine os apply <oci-ref-or-digest>` | apply an explicit immutable OCI machine-image rollout; current host-managed machines recreate boot artifacts from the recorded image, while bootc-native machines ask the guest machine API to stage `bootc switch` and then require a restart into the staged deployment |
| `nimbus machine os upgrade` | move back to the host-supported machine-image stream for this `nimbus` version; on bootc-native machines this stages `bootc upgrade` through the guest machine API, and `--dry-run` reports the current/target image pair as a concise action summary instead of a structured status dump |
| `nimbus machine os rollback` | for bootc-native machines, ask the guest machine API to stage `bootc rollback`; restart the machine to boot the previous deployment |

Current scope:

- records a typed XDG-style config root and state root for the default machine
- records a short `/tmp/nimbus/...` runtime root with typed socket, pid,
  and log paths
- persists the machine provider, typed guest image source, guest SSH user,
  guest resources, and future virtiofs volume mappings
- on macOS `krunkit`, defaults the guest image source to the pinned
  Nimbus-owned bootc machine-image digest owned by the host release
  (`docker://ghcr.io/nimbus/machine-os:v0.1.30@sha256:...`) instead of a
  floating tag
- for the supported Apple Silicon Homebrew install surface, expects `krunkit`
  as the explicit formula dependency and prefers a bundled `libexec/gvproxy`
  that ships beside the packaged `nimbus` binary; helper lookup now mirrors
  Podman's darwin `helper_binaries_dir` model by honoring
  `NIMBUS_MACHINE_HELPER_BINARY_DIR`, then known packaged and Podman helper
  locations, without relying on ambient `PATH` lookup for machine helpers
- manual macOS tarball installs must preserve that same relative
  `prefix/bin/nimbus` plus `prefix/libexec/gvproxy` layout, or set
  `NIMBUS_MACHINE_HELPER_BINARY_DIR` explicitly; moving only the `nimbus`
  binary is not a supported machine install shape
- for the default Nimbus bootc image, generates and attaches a
  bootc-native machine-config bundle; explicit Podman-image overrides remain
  on the legacy Ignition contract
- auto-generates a machine-owned SSH identity under the Nimbus machine data
  root when no explicit `--identity` override was recorded, so the default
  `machine start` path does not require a separate SSH-key setup step
- makes `nimbus machine start` the primary convergence path on macOS: cache
  missing machine-image artifacts, rebuild boot artifacts when the recorded
  base image drifts from the desired digest, hash-sync
  `/usr/local/bin/nimbus` only for explicit Podman/FCOS host-managed
  fallback machines, and report success only after the forwarded machine API is
  reachable. Bootc-native machines instead boot with the matching Linux
  `nimbus` binary already baked into the image.
- treats mutating machine commands as action-oriented UX surfaces: `init`,
  `start`, `stop`, `set`, and `rm` now print concise human summaries, while
  `machine status` and `machine inspect` remain the structured diagnostic
  surfaces
- emits step-oriented progress to stderr during long-running `machine start`
  convergence phases such as image pull/materialization, guest-binary fetch,
  VM boot, SSH readiness, and forwarded machine-API readiness, so first-start
  waits look active instead of silent
- keeps `machine start` Podman-aligned on output controls: `--quiet` suppresses
  phase/progress chatter but still prints the final success summary on stdout,
  while `--no-info` only suppresses advisory `info:` notices and leaves the
  normal phase banners intact
- auto-materializes OCI machine-image references plus `http(s)` image sources
  into the reserved machine-state raw disk path, with OCI artifact selection
  based on linux/arch plus the provider disk type (`applehv` on macOS),
  digest verification, gzip/zstd decompression for OCI blobs, and gzip
  decompression for direct URL downloads
- launches direct `krunkit` + `gvproxy` orchestration on macOS and waits for
  the machine-ready signal plus guest SSH reachability before reporting the
  machine manager as ready
- keeps guest machine-API readiness separate from machine readiness, so a
  booted guest with a missing guest `nimbus` binary does not get misreported
  as a working control plane
- renders the configured machine-API forwarding contract plus socket
  existence and actual API reachability separately, so host helpers, machine
  readiness, and guest control readiness do not get collapsed into one status
  bit
- keeps `nimbus machine os apply`, `nimbus machine os upgrade`, and
  `nimbus machine os rollback` as explicit machine-image lifecycle surfaces:
  current host-managed machines use controlled disk recreation, while
  bootc-native machines use the guest machine API to stage `bootc`
  switch/upgrade/rollback and keep disk replacement as a repair/recreate path
- treats `nimbus machine rm` as a full config/state-root removal, including the
  per-machine image and guest-binary caches under the state root, so a clean
  recreate path intentionally repulls or rehydrates artifacts on the next boot
- threads an optional `[NAME]` positional through the machine lifecycle
  surface (`init`, `start`, `stop`, `status`, `inspect`, `set`, `ssh`, `rm`),
  defaulting to `default` so multi-machine targeting does not require a second
  command family; `machine cp` follows Podman's embedded `NAME:/path` machine
  targeting instead of adding a second positional name
- defaults `nimbus machine status` to a condensed operator table while keeping
  the full serialized status view available through `-f json` and `-f yaml`
  for scripts and diagnostics
- lets `nimbus machine status --quiet` short-circuit those richer renderers and
  print only the selected machine name, matching the precedence rule that
  `machine list --quiet` already uses
- adds `nimbus machine list` / `nimbus machine ls` as the multi-machine
  summary surface, scanning initialized machine records from the config root
  and refreshing each machine's persisted state before rendering
- adds `nimbus machine inspect` as the raw machine-record surface for scripts
  and debugging, returning the persisted config contract plus the refreshed
  state record without the extra derived status fields
- keeps `nimbus machine set` intentionally narrow and Podman-aligned for the
  current contract: it updates stopped-machine CPU, memory, and disk settings
  in `config.json`, then requires the next `machine start` to apply them
- adds `nimbus machine cp` as the host↔guest transfer surface, using
  machine-prefixed guest paths (`default:/tmp/file`), recursive `scp`, the
  same localhost SSH safety options as `machine ssh`, and `--quiet` to suppress
  the success message when scripts want silence

## Encryption Commands

The encryption admin surface provides migration, rotation, and status operations:

| Command | Meaning |
| --- | --- |
| `nimbus encryption status` | print the current encryption status |
| `nimbus encryption migrate` | migrate a plaintext database to encrypted |
| `nimbus encryption export` | export an encrypted database to plaintext for recovery |
| `nimbus encryption rotate-kek` | rotate the wrapping key (rewraps metadata only) |
| `nimbus encryption rotate-dek` | rotate the data encryption key (rewrites pages) |

Common flags:

| Flag | Meaning |
| --- | --- |
| `--source` | source database path |
| `--target` | target database path |
| `--provider` | storage provider (`sqlite`, `redb`, `libsql-cache`) |
| `--tenant-id` | tenant id for tenant-owned databases or caches |
| `--path` | protected database path for `rotate-kek` and `rotate-dek` |
| `--new-key-provider` | replacement provider for `rotate-kek` (`master-key-file`, `key-dir`, `aws-kms`) |
| `--new-master-key-file` | replacement KEK for `rotate-kek` |
| `--new-key-dir` | replacement key directory for `rotate-kek` |
| `--new-aws-kms-key-id` | replacement AWS KMS key id or alias for `rotate-kek` |
| `--new-aws-region` | replacement AWS region for `rotate-kek` when targeting `aws-kms` |
| `--new-aws-endpoint-url` | replacement AWS endpoint override for `rotate-kek` |
| `--all` | rotate every manifest in a directory during `rotate-kek` |
| `--skip-validation` | skip SQLite migration validation |
| `--retire-source` | remove predecessor plaintext artifacts after successful migrate |
| `--skip-backup` | skip DEK-rotation backups |

Example workflows:

```bash
# Migrate plaintext SQLite to encrypted
NIMBUS_ENCRYPTION_KEY_PROVIDER=master-key-file \
NIMBUS_ENCRYPTION_MASTER_KEY_FILE=/secure/master.key \
nimbus encryption migrate \
  --source ./data/tenant.sqlite3 \
  --target ./data/tenant-encrypted.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a

# Export encrypted database for recovery
NIMBUS_ENCRYPTION_KEY_PROVIDER=master-key-file \
NIMBUS_ENCRYPTION_MASTER_KEY_FILE=/secure/master.key \
nimbus encryption export \
  --source ./data/tenant-encrypted.sqlite3 \
  --target ./data/tenant-recovery.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a

# Rotate the KEK for one manifest-backed SQLite database
NIMBUS_ENCRYPTION_KEY_PROVIDER=master-key-file \
NIMBUS_ENCRYPTION_MASTER_KEY_FILE=/secure/old.key \
nimbus encryption rotate-kek \
  --path ./data/tenant.sqlite3 \
  --new-master-key-file /secure/new.key

# Rotate the KEK onto AWS KMS without rewriting database pages
NIMBUS_ENCRYPTION_KEY_PROVIDER=master-key-file \
NIMBUS_ENCRYPTION_MASTER_KEY_FILE=/secure/old.key \
nimbus encryption rotate-kek \
  --path ./data/tenant.sqlite3 \
  --new-key-provider aws-kms \
  --new-aws-kms-key-id alias/nimbus-production \
  --new-aws-region us-east-1
```

See [Encryption at rest reference](encryption.md) for full operational guidance.

Related references:

- [Encryption at rest reference](encryption.md)
- [MicroVM and service-control baseline](microvm-service-baseline.md)
- [HTTP and WebSocket API](http-api.md)
- [Convex compatibility](../convex/compatibility.md)
