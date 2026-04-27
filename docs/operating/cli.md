# CLI Reference

Neovex serves HTTP/WebSocket traffic through an explicit `start` subcommand and
also exposes compose-backed service and machine management subcommands.

Current shipped CLI shape:

```bash
neovex dev [--app-dir PATH] [--port 3210] [--data-dir ./.neovex/dev] [--once] [--skip-codegen] [--tail-logs always|pause-on-sync|disable] [--compose-file PATH]...
```

```bash
neovex deploy [--url URL] [--token TOKEN] [--app-dir PATH] [--dry-run] [--skip-codegen] [--verbose]
```

```bash
neovex codegen [--app PATH]
```

```bash
neovex token rotate
```

```bash
neovex start [flags]
```

`neovex dev` starts the local development path with dev defaults, watched
codegen reruns, and local activation. `neovex deploy` pushes generated app
artifacts to an explicit self-hosted target. `neovex codegen` is the
first-party artifact-generation command. `neovex start` is the foreground
server-start path. The retired `neovex serve` command is not retained as a
legacy alias.

Current shipped compose-management commands:

```bash
neovex compose config [--file PATH]... [--services]
```

```bash
neovex compose up [service] [--file PATH]... [--tenant <tenant-id>]
```

```bash
neovex compose down [service] [--file PATH]... [--tenant <tenant-id>]
```

```bash
neovex compose ps [-f json|yaml|table] [--noheading] [--file PATH]... [--all-tenants]
```

```bash
neovex compose inspect <service> [-f json|yaml] [--file PATH]... [--tenant <tenant-id>]
```

```bash
neovex compose logs <service> [--file PATH]... [--tenant <tenant-id>] [--follow]
```

```bash
neovex compose top <service> [-f json|yaml|table] [--noheading] [--file PATH]... [--tenant <tenant-id>]
```

Current shipped machine commands:

```bash
neovex machine init [--cpus N] [--memory N] [--disk-size N] [--image SOURCE] [--identity PATH] [--ignition-path PATH] [--firmware PATH] [--volume HOST:GUEST] [--now] [NAME]
```

```bash
neovex machine start [--cpus N] [--memory N] [--disk-size N] [--image SOURCE] [--identity PATH] [--ignition-path PATH] [--firmware PATH] [--volume HOST:GUEST] [--quiet] [--no-info] [NAME]
```

```bash
neovex machine stop [NAME]
```

```bash
neovex machine status [-f json|yaml|table] [--noheading] [--quiet] [NAME]
```

```bash
neovex machine list [-f json|table] [--noheading] [--quiet]
```

```bash
neovex machine ls [-f json|table] [--noheading] [--quiet]
```

```bash
neovex machine info [-f json|yaml] [--format json|yaml]
```

```bash
neovex machine inspect [-f json|yaml] [NAME]
```

```bash
neovex machine set [--cpus N] [--memory N] [--disk-size N] [NAME]
```

```bash
neovex machine cp [--quiet] SRC_PATH DEST_PATH
```

```bash
neovex machine ssh [NAME] [COMMAND...]
```

```bash
neovex machine rm [NAME]
```

```bash
neovex machine os apply <oci-ref-or-digest> [--restart]
```

```bash
neovex machine os upgrade [--dry-run] [--restart]
```

```bash
neovex start [--host 127.0.0.1] [--port 8080] [--app-dir ./app] [--skip-codegen] [--compose-file PATH]...
```

For local development from source:

```bash
cargo run -p neovex-bin -- start [flags]
```

Current command taxonomy:

- `neovex dev`
  shipped local development server with one-shot startup codegen, debounced
  watched codegen reruns, local generation activation, and development
  persistence defaults
- `neovex deploy`
  shipped explicit-target deploy command for validating, diffing, and
  activating generated app artifacts on a running server
- `neovex codegen`
  shipped first-party code generation for `neovex/` or `convex/` source roots
- `neovex token rotate`
  shipped local admin token lifecycle command for rotating the localhost server
  access token using live-server semantics when a server is discoverable and
  offline semantics otherwise
- `neovex start`
  shipped explicit server-start verb
- `neovex compose ...`
  shipped Compose-backed service lifecycle namespace
- `neovex machine ...`
  shipped macOS machine lifecycle namespace

## Core Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--host` | `127.0.0.1` | host interface to listen on; use an explicit non-loopback value only when intentionally exposing the local server beyond localhost |
| `--port` | `8080` | port to listen on |
| `--data-dir` | `./data` | data directory for tenant databases |
| `--app-dir` | unset | optional app directory whose user source root may be `neovex/` or `convex/`; generated runtime artifacts still live under `.neovex/convex/` |
| `--skip-codegen` | `false` | skip the one-shot startup-time codegen preflight when `--app-dir` is set |
| `--license-file` | unset | optional explicit path to a Neovex license file |

If `--license-file` is not provided, Neovex next checks
`NEOVEX_LICENSE_FILE`, then `./.neovex/license.json`, and otherwise falls back
to the built-in community license.

## Encryption Flags

Local encryption for `neovex start` is controlled with
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
| `--encryption-aws-kms-key-id` | AWS KMS key ID or alias (e.g., `alias/neovex`) |
| `--encryption-aws-region` | AWS region for KMS |
| `--encryption-aws-endpoint-url` | optional KMS endpoint URL (for LocalStack) |

Environment variables: `NEOVEX_ENCRYPTION_KEY_PROVIDER`,
`NEOVEX_ENCRYPTION_MASTER_KEY_FILE`, `NEOVEX_ENCRYPTION_KEY_DIR`,
`NEOVEX_ENCRYPTION_AWS_KMS_KEY_ID`, `NEOVEX_ENCRYPTION_AWS_REGION`,
`NEOVEX_ENCRYPTION_AWS_ENDPOINT_URL`.

`neovex encryption ...` admin commands read the current provider and
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

The same values can also be supplied through the corresponding `NEOVEX_*`
environment variables or the JSON config file referenced by `--config` or
`NEOVEX_CONFIG`.

## Runtime Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--runtime-heap-mb` | `128` | V8 heap limit per isolate in megabytes |
| `--runtime-initial-heap-mb` | `8` | initial V8 heap size per isolate in megabytes |
| `--runtime-timeout-secs` | `30` | maximum wall-clock runtime invocation time |
| `--runtime-max-instances` | available hardware parallelism | maximum concurrent top-level runtime instances |
| `--runtime-max-nested-calls` | `64` | maximum nested `ctx.run*` invocations per request tree |

## Shared Compose Discovery

`neovex compose ...`, `neovex dev`, and `neovex start` now share one Compose
discovery rule:

- explicit repeated `--file` or `--compose-file` flags win and load exactly
  those files in the order provided
- when explicit flags are absent, `COMPOSE_FILE` provides an ordered explicit
  file list; `COMPOSE_PATH_SEPARATOR` overrides the platform default separator
  (`:` on macOS/Linux, `;` on Windows)
- without explicit flags or `COMPOSE_FILE`, Neovex searches the current
  directory first and
  then parent directories for `compose.yaml`, `compose.yml`,
  `docker-compose.yaml`, or `docker-compose.yml`
- when auto-discovery selects canonical `compose.yaml` and
  `compose.override.yaml` exists beside it, both files are loaded in that
  order as one logical Compose project
- if both legacy names exist in the same directory, Neovex fails with an
  actionable error instead of guessing
- relative paths in explicit flag lists and `COMPOSE_FILE` entries resolve from
  the current working directory, and `files[0]` remains the project identity
  plus relative-path anchor for merged Compose semantics
- `--app-dir` is separate: it selects the app/codegen context and does not
  redefine Compose discovery

## Dev Command

`neovex dev` is the Convex-migration happy path for local work. In the current
watch-loop slice it:

- auto-detects the app directory from the current directory by looking for a
  `neovex/` or `convex/` source root, falling back to the current directory
- runs one initial codegen pass unless `--skip-codegen` is set
- starts the same local server path as `neovex start`
- watches the selected `neovex/` or `convex/` source root for source changes
  and reruns codegen after a short debounce
- validates and locally activates regenerated artifacts through the deploy
  generation-swap path after watched codegen succeeds
- listens on port `3210` by default
- uses a shared project-local persistence root at `./.neovex/dev/` by default
  for both tenant data and local control state
- prints the local URL, app directory, persistence root, watched source root,
  and resolved Compose selection when one is active

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
| `--app-dir` | auto-detected | app directory whose user source root may be `neovex/` or `convex/`; generated runtime artifacts still live under `.neovex/convex/` |
| `--data-dir` | `./.neovex/dev/` under the resolved app directory | shared local dev persistence root for tenant data and control state |
| `--once` | `false` | run startup only and disable the watched codegen loop |
| `--skip-codegen` | `false` | skip the initial codegen pass and use already-generated artifacts |
| `--tail-logs` | `pause-on-sync` | accepted log-tail mode (`always`, `pause-on-sync`, or `disable`); live runtime log multiplexing remains pending runtime log plumbing |
| `--compose-file` | unset | optional explicit ordered Compose path list for local service dependencies; repeat the flag to merge overlays in order. When omitted, `dev` uses `COMPOSE_FILE` if set, then the shared cwd/parent discovery rule |

## Codegen Behavior

`neovex codegen` generates two classes of artifacts from the selected app
directory:

- `_generated/*` files under the detected `neovex/` or `convex/` source root
- runtime manifests and bundle files under `.neovex/convex/`

Equivalent entrypoints:

- `neovex codegen --app ./my-app`
- `npx convex codegen --app ./my-app`
- `npx neovex-codegen --app ./my-app`

The shared pipeline still expects generated files to be checked into version
control for stable typechecking and frontend workflows. The start-side
preflight described below is a startup convenience, not a watched `dev` loop.

From the repo root, the canonical JS verification entrypoints are now:

- `npm run typecheck`
- `npm run test`
- `npm run build`

Those commands fan out to workspace-owned package scripts where present, so the
root command surface stays stable even when individual package internals
change. The `@neovex/codegen` typecheck lane is a JS parser and
codegen-boundary guardrail check because the package is implemented as `.mjs`
rather than TypeScript.

## Deploy Command

`neovex deploy` packages generated app artifacts and sends them to a running
Neovex server. The target is explicit in v1: use `--url` or
`NEOVEX_DEPLOY_URL`. Authentication uses `--token` or `NEOVEX_DEPLOY_TOKEN`,
matching the token configured on the server.

Default behavior:

- auto-detects the app directory from the current directory by looking for a
  `neovex/` or `convex/` source root, falling back to the current directory
- runs codegen before packaging unless `--skip-codegen` is set
- packages generated artifacts from `.neovex/convex/`
- uploads to `POST /api/admin/deploy`
- prints a concise human diff for functions, HTTP routes, schema/index
  changes, and runtime-bundle changes

Flags:

| Flag | Default | Meaning |
| --- | --- | --- |
| `--url` | `NEOVEX_DEPLOY_URL` or required | explicit target Neovex server URL |
| `--token` | `NEOVEX_DEPLOY_TOKEN` or required | deploy admin bearer token |
| `--app-dir` | auto-detected | app directory whose generated artifacts live under `.neovex/convex/` |
| `--dry-run` | `false` | validate and diff without activating a new generation |
| `--skip-codegen` | `false` | package existing generated artifacts without running codegen |
| `--verbose` | `false` | show extra packaging phase detail |

## Deploy Admin API

Start the server with `NEOVEX_DEPLOY_TOKEN` to enable
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

Neovex requires an explicit subcommand. `neovex start` starts the server. On
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

When a Compose selection is present, Neovex validates it through the same
adapter used by `neovex compose config`, lowers it into a typed
declared-service catalog, and wires that catalog into the server-owned sandbox
manager. With `--app-dir`, ready declared-service bindings appear in
`ctx.services.<name>`, and missing bindings can be activated explicitly through
`await ctx.services.get("<name>")`. The app directory may use `neovex/` as the
native user source root or `convex/` as the compatibility root; in both cases
the runtime registry still loads the generated manifests from
`.neovex/convex/`. The
explicit `neovex compose up/down/...` commands share that same Compose
discovery, lowering, deterministic project identity, and project-scoped
backend root instead of inventing a second lifecycle control plane.

The `start` preflight is intentionally one-shot. After startup, Neovex does not
watch the filesystem or regenerate `_generated/*` as functions change. If you
want watched edit-loop behavior, that remains follow-on work.

On macOS, if that Compose project resolves to the forwarded guest container
backend and the default machine is initialized but stopped, `neovex start`
now starts the machine first and only then wires the forwarded guest manager.

This is why `start` and `compose` are not the same concept:

- server startup owns the Neovex API surface itself: HTTP, WebSocket,
  Convex-compatible routes, runtime execution, and the `ctx.services` snapshot
  plus activation surface
- `compose` commands manage declared backing workloads that Neovex may start,
  stop, inspect, or log

`machine` is a third concept:

- `machine` commands manage the macOS Linux-guest envelope that Neovex will use
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
- `neovex machine list` marks the default machine with `*` in human table
  output and sorts active machines ahead of inactive ones
- Go-template output is intentionally not supported in the current shipped CLI;
  `json` and `yaml` remain the stable machine-readable formats

## Compose Commands

The current landed compose-control surface exposes shared-discovery Compose
validation plus explicit lifecycle control:

| Command | Meaning |
| --- | --- |
| `neovex compose config` | validate the discovered Compose project, the `COMPOSE_FILE` selection when present, or an explicit ordered `--file` list, and print the resolved service plan as YAML |
| `neovex compose config --file ./stack.yml --file ./stack.dev.yml` | validate a specific ordered Compose file list |
| `neovex compose config --services` | list only service names, one per line |
| `neovex compose up` | start all declared services for the deterministic local project tenant and print a concise action summary instead of a YAML lifecycle dump; active current services are reported inline as `already_running` |
| `neovex compose up db --tenant tenant-a` | start only service `db` for an explicit tenant and print the same action-summary contract |
| `neovex compose down` | stop one current persisted sandbox per service identity for the deterministic local project tenant and print a concise action summary instead of a YAML lifecycle dump |
| `neovex compose down db` | stop the current persisted sandbox for service `db` in the deterministic local project tenant with the same action-summary contract |
| `neovex compose ps` | list persisted sandbox state for the deterministic local project tenant in a human table by default; use `-f json` or `-f yaml` for structured output, or `--noheading` to suppress table headers |
| `neovex compose ps --all-tenants` | list all persisted sandboxes under the project-scoped backend root with the same table-vs-structured contract |
| `neovex compose inspect db` | inspect persisted sandbox details for service `db` in the deterministic local project tenant as JSON by default; `-f yaml` is the explicit structured alternative |
| `neovex compose inspect db --tenant tenant-a` | inspect persisted sandbox details for service `db` in an explicit tenant with the same structured contract |
| `neovex compose logs db` | print the persisted `ctr.log` for service `db` in the deterministic local project tenant |
| `neovex compose logs db --follow` | keep polling the persisted `ctr.log` for appended output |
| `neovex compose top db` | show the persisted PID snapshot and matching host `ps` rows for service `db` as a human summary by default; use `-f json` or `-f yaml` for structured output, or `--noheading` to suppress the process table headings |

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
- preserves `depends_on`, `healthcheck`, `volumes`, labels, and `x-neovex`
  metadata for lifecycle commands and recovery drills
- warns on ignored fields such as `networks`, `privileged`, and `logging`
- resolves lifecycle commands against backend-owned persisted sandbox state
  under the project-scoped `control_data_dir`, not a separate CLI-owned
  service database: krun manifests/logs locally for krun-backed projects, and
  forwarded guest container manifests/logs on macOS for container-backed
  projects
- on macOS, container-backed `neovex compose up/down/ps/inspect/logs/top`
  commands now route through the forwarded guest machine API and guest
  container-manifest state instead of assuming host-local krun state
- mixed-backend project-wide Compose operations still reject because
  `neovex compose ...` requires one backend family per project-wide command
- the server startup path can now load container-backed Compose managers on
  macOS through the forwarded guest machine API; if the default machine is
  initialized but stopped, `neovex start` now starts it first under the same
  machine convergence contract, then requires `service_execution_ready`
- derives a deterministic local project tenant id from the Compose project key,
  and uses that tenant by default for `compose up` / `compose down` /
  `compose ps` / `compose inspect` / `compose logs` / `compose top`

## Machine Commands

The current landed machine surface is the checked-in macOS machine contract:

| Command | Meaning |
| --- | --- |
| `neovex machine init [NAME]` | write the named machine config and state files, record the guest resource contract, and optionally start immediately with `--now`; omitting `NAME` still targets `default` and action commands print a concise success summary instead of the full structured status view |
| `neovex machine start [NAME]` | start the named machine, creating it with defaults first when it does not already exist, then wait for machine-ready, guest SSH, and forwarded machine-API reachability while emitting human progress updates for slow first-start convergence on stderr; `--quiet` suppresses the phase chatter but still prints the final success summary, while `--no-info` suppresses advisory `info:` notices without hiding the normal phase banners |
| `neovex machine stop [NAME]` | stop the named machine helpers, including stale-helper recovery, and persist the stopped machine state before printing a concise success summary |
| `neovex machine status [NAME]` | print the current named machine status in table form by default, emit the full structured view with `-f json` / `-f yaml`, suppress table headings with `--noheading`, or print the machine name only with `--quiet` |
| `neovex machine list` / `neovex machine ls` | list initialized machines from the Neovex config root in a compact table by default; active machines sort ahead of inactive ones, the default machine is marked with `*`, `--quiet` stays names-only when no explicit `-f` is requested, `--noheading` suppresses table headers, and `-f json` exposes the same `default` bit structurally |
| `neovex machine info` | print host-level machine information in YAML by default, or JSON with `-f json`; this is the operator-facing summary for the machine roots, cache locations, current `neovex` machine release, and the default-machine summary contract |
| `neovex machine inspect [NAME]` | print the persisted config plus refreshed state record for the named machine as JSON by default, or YAML with `-f yaml` |
| `neovex machine set [NAME]` | update the recorded CPU, memory, or disk contract for a stopped named machine without recreating it |
| `neovex machine cp [--quiet] SRC_PATH DEST_PATH` | recursively copy files or directories between the host and a running machine using Podman-style `NAME:/path` guest endpoints and the machine's configured SSH contract |
| `neovex machine ssh [NAME] [COMMAND...]` | run a command through the configured guest SSH user and identity once the machine is running; as with Podman, if the first argument names an existing machine it is treated as `NAME`, otherwise it is passed through as the guest command on `default` |
| `neovex machine rm [NAME]` | remove the persisted config, state, and short runtime-root layout for the named machine when it is not running |
| `neovex machine os apply <oci-ref-or-digest>` | record an explicit immutable OCI machine-image rollout, invalidate boot artifacts so the next boot recreates from that image, and print a concise action summary with restart/start guidance when relevant |
| `neovex machine os upgrade` | move back to the host-supported machine-image stream for this `neovex` version; `--dry-run` now reports the current/target image pair as a concise action summary instead of a structured status dump |

Current scope:

- records a typed XDG-style config root and state root for the default machine
- records a short `/tmp/neovex/...` runtime root with typed socket, pid,
  and log paths
- persists the machine provider, typed guest image source, guest SSH user,
  guest resources, and future virtiofs volume mappings
- on macOS `krunkit`, defaults the guest image source to the pinned Podman
  machine-image digest owned by the host release
  (`docker://quay.io/podman/machine-os@sha256:...`) instead of a floating tag
- for the supported Apple Silicon Homebrew install surface, expects `krunkit`
  as the explicit formula dependency and prefers a bundled `libexec/gvproxy`
  that ships beside the packaged `neovex` binary; helper lookup now mirrors
  Podman's darwin `helper_binaries_dir` model by honoring
  `NEOVEX_MACHINE_HELPER_BINARY_DIR`, then known packaged and Podman helper
  locations, without relying on ambient `PATH` lookup for machine helpers
- manual macOS tarball installs must preserve that same relative
  `prefix/bin/neovex` plus `prefix/libexec/gvproxy` layout, or set
  `NEOVEX_MACHINE_HELPER_BINARY_DIR` explicitly; moving only the `neovex`
  binary is not a supported machine install shape
- auto-generates a Neovex-owned Ignition file when no explicit
  `--ignition-path` override is configured, carrying the machine ready signal,
  guest `neovex.socket` plus `neovex.service`, and virtiofs mount-unit wiring
  into the guest
- auto-generates a machine-owned SSH identity under the Neovex machine data
  root for the host-managed macOS contract when no explicit `--identity`
  override was recorded, so the default `machine start` path does not require
  a separate SSH-key setup step
- makes `neovex machine start` the primary convergence path on macOS: cache
  missing machine-image and guest-Linux-`neovex` artifacts, rebuild boot
  artifacts when the recorded base image drifts from the desired digest,
  hash-sync `/usr/local/bin/neovex` inside the guest, and only then report
  success after the forwarded machine API is reachable
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
  booted guest with a missing guest `neovex` binary does not get misreported
  as a working control plane
- renders the configured machine-API forwarding contract plus socket
  existence and actual API reachability separately, so host helpers, machine
  readiness, and guest control readiness do not get collapsed into one status
  bit
- keeps `neovex machine os apply` and `neovex machine os upgrade` as explicit,
  host-managed machine-image rollout surfaces instead of ad hoc guest mutation
- treats `neovex machine rm` as a full config/state-root removal, including the
  per-machine image and guest-binary caches under the state root, so a clean
  recreate path intentionally repulls or rehydrates artifacts on the next boot
- threads an optional `[NAME]` positional through the machine lifecycle
  surface (`init`, `start`, `stop`, `status`, `inspect`, `set`, `ssh`, `rm`),
  defaulting to `default` so multi-machine targeting does not require a second
  command family; `machine cp` follows Podman's embedded `NAME:/path` machine
  targeting instead of adding a second positional name
- defaults `neovex machine status` to a condensed operator table while keeping
  the full serialized status view available through `-f json` and `-f yaml`
  for scripts and diagnostics
- lets `neovex machine status --quiet` short-circuit those richer renderers and
  print only the selected machine name, matching the precedence rule that
  `machine list --quiet` already uses
- adds `neovex machine list` / `neovex machine ls` as the multi-machine
  summary surface, scanning initialized machine records from the config root
  and refreshing each machine's persisted state before rendering
- adds `neovex machine inspect` as the raw machine-record surface for scripts
  and debugging, returning the persisted config contract plus the refreshed
  state record without the extra derived status fields
- keeps `neovex machine set` intentionally narrow and Podman-aligned for the
  current contract: it updates stopped-machine CPU, memory, and disk settings
  in `config.json`, then requires the next `machine start` to apply them
- adds `neovex machine cp` as the host↔guest transfer surface, using
  machine-prefixed guest paths (`default:/tmp/file`), recursive `scp`, the
  same localhost SSH safety options as `machine ssh`, and `--quiet` to suppress
  the success message when scripts want silence

## Encryption Commands

The encryption admin surface provides migration, rotation, and status operations:

| Command | Meaning |
| --- | --- |
| `neovex encryption status` | print the current encryption status |
| `neovex encryption migrate` | migrate a plaintext database to encrypted |
| `neovex encryption export` | export an encrypted database to plaintext for recovery |
| `neovex encryption rotate-kek` | rotate the wrapping key (rewraps metadata only) |
| `neovex encryption rotate-dek` | rotate the data encryption key (rewrites pages) |

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
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/master.key \
neovex encryption migrate \
  --source ./data/tenant.sqlite3 \
  --target ./data/tenant-encrypted.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a

# Export encrypted database for recovery
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/master.key \
neovex encryption export \
  --source ./data/tenant-encrypted.sqlite3 \
  --target ./data/tenant-recovery.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a

# Rotate the KEK for one manifest-backed SQLite database
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/old.key \
neovex encryption rotate-kek \
  --path ./data/tenant.sqlite3 \
  --new-master-key-file /secure/new.key

# Rotate the KEK onto AWS KMS without rewriting database pages
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/old.key \
neovex encryption rotate-kek \
  --path ./data/tenant.sqlite3 \
  --new-key-provider aws-kms \
  --new-aws-kms-key-id alias/neovex-production \
  --new-aws-region us-east-1
```

See [Encryption at rest reference](encryption.md) for full operational guidance.

Related references:

- [Encryption at rest reference](encryption.md)
- [MicroVM and service-control baseline](microvm-service-baseline.md)
- [HTTP and WebSocket API](http-api.md)
- [Convex compatibility](../convex/compatibility.md)
