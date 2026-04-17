# CLI Reference

Neovex serves HTTP/WebSocket traffic by default and also exposes service
management subcommands.

Current shipped CLI shape:

```bash
neovex serve [flags]
```

`neovex serve` is the current server-start path.

Current shipped service-management commands:

```bash
neovex service config [--file compose.yaml] [--services]
```

```bash
neovex service up [service] [--file compose.yaml] [--tenant <tenant-id>]
```

```bash
neovex service down [service] [--file compose.yaml] [--tenant <tenant-id>]
```

```bash
neovex service list [--file compose.yaml] [--all-tenants]
```

```bash
neovex service inspect <service> [--file compose.yaml] [--tenant <tenant-id>]
```

```bash
neovex service logs <service> [--file compose.yaml] [--tenant <tenant-id>] [--follow]
```

```bash
neovex service ps <service> [--file compose.yaml] [--tenant <tenant-id>]
```

Current shipped machine commands:

```bash
neovex machine init [--cpus N] [--memory-mib N] [--disk-gib N] [--image SOURCE] [--ssh-identity PATH] [--ignition-file PATH] [--efi-store PATH] [--volume HOST:GUEST]
```

```bash
neovex machine start
```

```bash
neovex machine stop
```

```bash
neovex machine status
```

```bash
neovex machine ssh [COMMAND...]
```

```bash
neovex machine rm
```

```bash
neovex machine os apply <oci-ref-or-digest> [--restart]
```

```bash
neovex machine os upgrade [--dry-run] [--restart]
```

```bash
neovex serve --compose-file ./compose.yaml [--convex-app-dir ./app]
```

For local development from source:

```bash
cargo run -p neovex-bin -- serve [flags]
```

Current command taxonomy:

- `neovex serve`
  shipped explicit server-start verb
- `neovex service ...`
  shipped managed-service lifecycle namespace
- `neovex machine ...`
  shipped macOS machine lifecycle namespace

## Core Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--port` | `8080` | port to listen on |
| `--data-dir` | `./data` | data directory for tenant databases |
| `--convex-app-dir` | unset | optional app directory containing a generated `.neovex/convex/functions.json` manifest |
| `--license-file` | unset | optional explicit path to a Neovex license file |

If `--license-file` is not provided, Neovex next checks
`NEOVEX_LICENSE_FILE`, then `./.neovex/license.json`, and otherwise falls back
to the built-in community license.

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

## Startup Behavior

When no subcommand is provided, Neovex starts the server. On startup, it:

- initializes tracing
- loads the service with the configured data directory
- loads tenants with scheduled work
- starts the scheduler loop
- optionally loads the Convex registry from `--convex-app-dir`
- optionally loads declared sandbox-backed services from `--compose-file`
- loads license state from the explicit path, environment, default path, or
  built-in community defaults

When `--compose-file` is present, Neovex validates the Compose file through the
same adapter used by `neovex service config`, lowers it into a typed
declared-service catalog, and wires that catalog into the server-owned sandbox
manager. With `--convex-app-dir`, `ctx.services.*` can activate those declared
services on first use. The explicit `neovex service up/down/...` commands share
that same Compose lowering, deterministic project identity, and project-scoped
backend root instead of inventing a second lifecycle control plane.

This is why `serve` and `service` are not the same concept:

- server startup owns the Neovex API surface itself: HTTP, WebSocket,
  Convex-compatible routes, runtime execution, and `ctx.services.*` activation
- `service` commands manage declared backing workloads that Neovex may start,
  stop, inspect, or log

`machine` is a third concept:

- `machine` commands manage the macOS Linux-guest envelope that Neovex will use
  for developer workflows on Apple Silicon
- the shipped MAC2 surface owns persisted machine config, typed path layout,
  CLI/state-model wiring, and the initial direct `krunkit` + `gvproxy`
  machine-manager seam
- guest-image/bootstrap completion and transparent developer UX remain owned by
  the active macOS machine-support plan

## Service Commands

The current landed service-control surface exposes Compose validation plus
explicit lifecycle control:

| Command | Meaning |
| --- | --- |
| `neovex service config` | parse `compose.yaml`, validate the supported subset, and print the resolved service plan as YAML |
| `neovex service config --file ./stack.yml` | validate a specific Compose file |
| `neovex service config --services` | list only service names, one per line |
| `neovex service up` | start all declared services for the deterministic local project tenant; active current services report `already_running` |
| `neovex service up db --tenant tenant-a` | start only service `db` for an explicit tenant |
| `neovex service down` | stop one current persisted sandbox per service identity for the deterministic local project tenant |
| `neovex service down db` | stop the current persisted sandbox for service `db` in the deterministic local project tenant |
| `neovex service list` | list persisted sandbox state for the deterministic local project tenant as YAML |
| `neovex service list --all-tenants` | list all persisted sandboxes under the project-scoped backend root |
| `neovex service inspect db` | inspect persisted sandbox details for service `db` in the deterministic local project tenant |
| `neovex service inspect db --tenant tenant-a` | inspect persisted sandbox details for service `db` in an explicit tenant |
| `neovex service logs db` | print the persisted `ctr.log` for service `db` in the deterministic local project tenant |
| `neovex service logs db --follow` | keep polling the persisted `ctr.log` for appended output |
| `neovex service ps db` | show the persisted PID snapshot and matching host `ps` rows for service `db` |

Current scope:

- validates `image` and `build` sources
- resolves `environment` plus `env_file`
- validates lowerable `command`, `entrypoint`, `working_dir`, and `user`
  process overrides
- validates lowerable lifecycle settings such as `restart` and
  `stop_grace_period` against the generic sandbox lifecycle seam
- validates the declared-service catalog handoff used by both the server-owned
  service manager and the explicit `service up` launch path
- lowers `ports`, restart policy, and CPU/memory limits into the resolved plan
- preserves `depends_on`, `healthcheck`, `volumes`, labels, and `x-neovex`
  metadata for lifecycle commands and recovery drills
- warns on ignored fields such as `networks`, `privileged`, and `logging`
- resolves lifecycle commands against backend-owned persisted sandbox state
  under the project-scoped `control_data_dir`, not a separate CLI-owned
  service database: krun manifests/logs locally for krun-backed projects, and
  forwarded guest container manifests/logs on macOS for container-backed
  projects
- on macOS, container-backed `neovex service up/down/list/inspect/logs/ps`
  commands now route through the forwarded guest machine API and guest
  container-manifest state instead of assuming host-local krun state
- mixed-backend project-wide Compose operations still reject because
  `neovex service ...` requires one backend family per project-wide command
- the server startup path can now load container-backed Compose managers on
  macOS through the forwarded guest machine API when the default machine API
  is reachable and reports `service_execution_ready`
- derives a deterministic local project tenant id from the Compose project key,
  and uses that tenant by default for `service up` / `service down` /
  `service list` / `service inspect` / `service logs` / `service ps`

## Machine Commands

The current landed machine surface is the checked-in macOS machine contract:

| Command | Meaning |
| --- | --- |
| `neovex machine init` | write the default machine config and state files, create the typed config/state/runtime roots, and record future guest resource settings |
| `neovex machine start` | launch `krunkit` + `gvproxy`, wait for machine-ready plus guest SSH reachability, and report guest machine-API reachability separately |
| `neovex machine stop` | stop the current machine helpers, including stale-helper recovery, and persist the stopped machine state |
| `neovex machine status` | print the current machine config, lifecycle state, derived runtime/socket/log paths, the configured machine-API forwarding contract, and guest machine-API reachability |
| `neovex machine ssh [COMMAND...]` | run a command through the configured guest SSH user and identity once the machine is running |
| `neovex machine rm` | remove the persisted machine config, state, and short runtime-root layout when the machine is not running |
| `neovex machine os apply <oci-ref-or-digest>` | record an explicit immutable OCI machine-image rollout and invalidate boot artifacts so the next boot recreates from that image |
| `neovex machine os upgrade` | move back to the host-supported machine-image stream for this `neovex` version, with `--dry-run` for status-only checks |

Current scope:

- records a typed XDG-style config root and state root for the default machine
- records a short `/tmp/neovex/...` runtime root with typed socket, pid,
  and log paths
- persists the machine provider, typed guest image source, guest SSH user,
  guest resources, and future virtiofs volume mappings
- on macOS `krunkit`, defaults the guest image source to the pinned Podman
  machine-image digest owned by the host release
  (`docker://quay.io/podman/machine-os@sha256:...`) instead of a floating tag
- auto-generates a Neovex-owned Ignition file when no explicit
  `--ignition-file` override is configured, carrying the machine ready signal,
  guest `neovex.socket` plus `neovex.service`, and virtiofs mount-unit wiring
  into the guest
- makes `neovex machine start` the primary convergence path on macOS: cache
  missing machine-image and guest-Linux-`neovex` artifacts, rebuild boot
  artifacts when the recorded base image drifts from the desired digest,
  hash-sync `/usr/local/bin/neovex` inside the guest, and only then report
  success after the forwarded machine API is reachable
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

Related references:

- [MicroVM and service-control baseline](microvm-service-baseline.md)
- [HTTP and WebSocket API](http-api.md)
- [Convex compatibility](../convex/compatibility.md)
