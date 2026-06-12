---
title: CLI reference
description: Command and flag reference for the nimbus binary — server, dev loop, deploys, machines, Compose services, policy, and encryption admin.
sidebar:
  label: CLI
  order: 2
---

The `nimbus` binary is the entire product surface: server, dev loop, deploy
client, codegen, and operator tooling ship as one executable. Every command
requires an explicit subcommand — there is no default action.

```bash
nimbus <command> [subcommand] [flags]
```

| Command | What it does |
| --- | --- |
| [`start`](#nimbus-start) | Start a Nimbus server in the foreground |
| [`dev`](#nimbus-dev) | Start a local development server with watched codegen |
| [`deploy`](#nimbus-deploy) | Push app artifacts to a self-hosted Nimbus instance |
| [`codegen`](#nimbus-codegen) | Generate app artifacts from `nimbus/` or `convex/` source |
| [`init`](#nimbus-init) | Scaffold a new Nimbus project |
| [`token`](#nimbus-token) | Local admin token management |
| [`backup`](#nimbus-backup) | Offline backup and restore of the local data directory |
| [`auth`](#nimbus-auth) | Console sign-in URLs and remote deploy credentials |
| [`ui`](#nimbus-ui) | Open the operator console in a browser |
| [`machine`](#nimbus-machine) | Manage local developer machines (macOS Linux guest) |
| [`node`](#nimbus-node) | Install and manage node service-manager artifacts |
| [`compose`](#nimbus-compose) | Compose-backed local service lifecycle |
| [`policy`](#nimbus-policy) | Validate and explain operator policy files |
| [`encryption`](#nimbus-encryption) | At-rest encryption admin operations |
| [`packages`](#nimbus-packages) | Provision embedded Nimbus JS packages into an app |

For what each surface currently supports, see
[Current capabilities](/reference/current-capabilities/).

## nimbus start

```bash
nimbus start [flags]
```

Starts a Nimbus server in the foreground. Binds loopback by default; pass
`--allow-network` (after rotating the local admin token) to bind a
non-loopback interface. With `--app-dir` it runs one codegen preflight and
serves that app; without it, the server starts empty and waits for deploys
through the admin API. See [Self-host](/get-started/self-host/) for the
deployment walkthrough.

Most-used flags:

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--port` | — | `8080` | Port to listen on. |
| `--host` | — | `127.0.0.1` | Host interface to listen on; defaults to loopback for local safety. |
| `--allow-network` | — | `false` | Opt in to binding a non-loopback interface; requires an explicitly rotated local admin token (`nimbus auth rotate-admin`). |
| `--cors-allow-origin` | `NIMBUS_CORS_ALLOW_ORIGINS` | loopback only | Additional allowed browser origin for CORS (repeatable; env var is comma-separated). Exact match; wildcards rejected. |
| `--data-dir` | `NIMBUS_DATA_DIR` | `./data` | Local data directory for embedded tenant databases and, by default, the control plane. |
| `--tenant-provider` | `NIMBUS_TENANT_PROVIDER` | `sqlite` | Tenant persistence provider: `sqlite`, `libsql-replica`, `redb`, `postgres`, or `mysql`. |
| `--config` | `NIMBUS_CONFIG` | unset | Optional JSON config file; CLI flags override env vars, which override file values. |

`nimbus start` accepts further flag families — TLS termination
(`--tls-cert` + `--tls-key`), protocol adapters
(`--firestore`; `--mongodb-port` with `--mongodb-username` and the env-only
`NIMBUS_MONGODB_PASSWORD`; `--dynamodb-port` with repeatable
`--dynamodb-access-key KEY_ID:SECRET:TENANT` or
`NIMBUS_DYNAMODB_ACCESS_KEYS`), app loading (`--app-dir`,
`--skip-codegen`, `--debug-node-apis`), Compose services (`--compose-file`),
systemd socket activation (`--systemd-socket-activation`), licensing
(`--license-file`, falling back to `NIMBUS_LICENSE_FILE` then
`~/.config/nimbus/license.json`), per-provider storage settings
(`--libsql-*`, `--postgres-*`, `--mysql-*`), runtime limits
(`--runtime-heap-mb`, `--runtime-timeout-secs`, and the other
`--runtime-*` flags), and at-rest encryption (`--encryption-key-provider`
and the other `--encryption-*` flags). The full flag ↔ environment variable ↔
config-key cross-reference lives in [Configuration](/reference/configuration/).

## nimbus dev

```bash
nimbus dev [flags]
```

Starts a local development server with dev defaults: watched codegen reruns,
local generation activation, an auto-created `demo` tenant, and automatic
browser launch of the operator console. When `--app-dir` is omitted, the app
directory is auto-detected by walking up from the current directory to the
nearest `.git` boundary. See [Quickstart](/get-started/quickstart/).

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--port` | — | `3210` | Port to listen on. |
| `--app-dir` | — | auto-detected | App directory containing an adapter source root. |
| `--data-dir` | — | `<app-dir>/.nimbus/dev` | Shared local dev persistence root for tenant data and control state. |
| `--compose-file` | `COMPOSE_FILE` | discovered | Ordered Compose file list for local service dependencies; repeat to merge overlays. |
| `--once` | — | `false` | Run startup only, without the watched codegen loop. |
| `--skip-codegen` | — | `false` | Skip initial codegen before starting; watched reruns still use codegen. |
| `--debug-node-apis` | — | `false` | Diagnose Node.js builtin imports that should move behind `"use node"`. |
| `--tail-logs` | — | `pause-on-sync` | Runtime log tailing mode: `always`, `pause-on-sync`, or `disable`. |
| `--no-open` | — | `false` | Suppress the default browser auto-open and print a launch-URL banner instead. |

Browser auto-open is also suppressed automatically in non-interactive
environments (`$CI` or `$NO_BROWSER` set, or stdout is not a TTY).

## nimbus deploy

```bash
nimbus deploy [flags]
```

Packages generated app artifacts and pushes them to an explicit self-hosted
Nimbus instance, printing a diff of functions, HTTP routes, schema, and
runtime-bundle changes. The target URL and token resolve from flags first,
then environment variables; tokens stored by `nimbus auth login` are used
when `NIMBUS_DEPLOY_TOKEN` is unset. The deploy route also requires the
server's local admin token: when the target URL is a loopback address on
the same machine, `nimbus deploy` reads it from the local token file
automatically; for a remote server, pass `--admin-token` or set
`NIMBUS_ADMIN_TOKEN`. The server-side contract is documented in
[Deploy & admin API](/reference/deploy-admin-api/).

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--url` | `NIMBUS_DEPLOY_URL` | required | Target Nimbus server URL. |
| `--token` | `NIMBUS_DEPLOY_TOKEN` | credentials file | Deploy admin bearer token. |
| `--admin-token` | `NIMBUS_ADMIN_TOKEN` | local token file (loopback targets) | Local admin token sent as `X-Nimbus-Admin-Token`. |
| `--app-dir` | — | auto-detected | App directory containing a `nimbus/` or `convex/` source root. |
| `--dry-run` | — | `false` | Validate and diff without activating the new generation. |
| `--skip-codegen` | — | `false` | Skip codegen and package already-generated artifacts. |
| `--verbose` | — | `false` | Show packaging and deploy phase detail. |

## nimbus codegen

```bash
nimbus codegen [--app <path>]
```

Generates `_generated/*` files and the runtime bundle from a `nimbus/` or
`convex/` source root. Codegen is embedded in the binary — there is no
separate npm codegen step. See [Developers](/developers/) for the authoring
workflow.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--app` | — | `.` | App directory containing a `nimbus/` or `convex/` source root. |
| `--debug-node-apis` | — | `false` | Diagnose Node.js builtin imports that should move behind `"use node"`. |

## nimbus init

```bash
nimbus init <adapter> [directory] [flags]
```

Scaffolds a new Nimbus project. The adapter argument is required and selects
the project template; existing files are never overwritten.

| Argument / flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `<adapter>` | — | required | Adapter to scaffold: `convex` or `cloud-functions`. |
| `[directory]` | — | `.` | Target directory, created if it does not exist. |
| `--source-root` | — | `convex` | Source root directory name (convex adapter only). |
| `--install` | — | `false` | Install adapter dependencies after scaffolding. |

## nimbus token

Local admin token management.

### nimbus token rotate

```bash
nimbus token rotate
```

Rotates the local admin token used for localhost server access. When a live
server is discoverable, rotation goes through the running server; otherwise
it rotates the on-disk token file offline. Takes no flags.

## nimbus backup

```bash
nimbus backup create --data-dir ./data --out <file>
nimbus backup restore --in <file> --data-dir ./data
```

Offline whole-deployment backup and restore for the embedded providers
(`--provider sqlite|redb`): one point-in-time archive per tenant in a
single file, fingerprint-verified on restore. Run with the server
stopped; restore requires a fresh data directory. See
[Backup & restore](/operators/backup-restore/) for procedures, encrypted
deployments, and external backends.

## nimbus auth

Sign-in URLs for the local console and credentials for remote deploys.

### nimbus auth url

```bash
nimbus auth url [--copy] [--open]
```

Mints a single-use launch URL for the local operator console. Requires a
running server (`nimbus start` or `nimbus dev`).

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--copy` | — | `false` | Copy the launch URL to the OS clipboard in addition to printing it. |
| `--open` | — | `false` | Open the launch URL in the default browser in addition to printing it. |

### nimbus auth token

```bash
nimbus auth token [--copy]
```

Prints the local admin token from the on-disk token file.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--copy` | — | `false` | Copy the token to the OS clipboard in addition to printing it. |

### nimbus auth login

```bash
nimbus auth login --url <daemon-url> [--bearer <token>]
```

Stores a deploy bearer token for a remote Nimbus daemon in the local
credentials file. `nimbus deploy` falls back to this store when
`NIMBUS_DEPLOY_TOKEN` is unset.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--url` | — | required | Daemon URL to authenticate against (e.g. `https://nimbus.example.com`). |
| `--bearer` | — | stdin | Deploy bearer token; if omitted, read from stdin. |

### nimbus auth status

```bash
nimbus auth status
```

Lists configured deploy connections with masked bearers and metadata. Takes
no flags.

### nimbus auth logout

```bash
nimbus auth logout --url <daemon-url>
```

Removes a stored deploy bearer.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--url` | — | required | Daemon URL whose stored bearer should be removed. |

### nimbus auth rotate-admin

```bash
nimbus auth rotate-admin
```

Rotates the local admin token offline. Required before
`nimbus start --allow-network` when the token has gone stale. A running
daemon keeps its in-memory token until restart. Takes no flags.

## nimbus ui

```bash
nimbus ui
```

Discovers the running local daemon and opens the operator console in a
browser. It does not spawn a daemon — start one with `nimbus start` or
`nimbus dev` first. Takes no flags.

## nimbus machine

Manages local developer machines — the Linux guest VM that backs container
workloads on macOS. Most subcommands take an optional machine `[name]`
positional that defaults to `default`.

### nimbus machine init

```bash
nimbus machine init [flags] [name]
```

Initializes a new machine: writes its config and state records and records
the guest resource contract.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-c, --cpus` | — | `2` | Number of CPUs. |
| `-m, --memory` | — | `2048` | Memory in MiB. |
| `-d, --disk-size` | — | `20` | Disk size in GiB. |
| `--image` | — | release-pinned image | Machine OS image source. |
| `--identity` | — | auto-generated | Path to the SSH identity for guest access. |
| `--ignition-path` | — | unset | Legacy Ignition config file for explicit non-bootc image overrides. |
| `--firmware` | — | unset | Path to the EFI variable store. |
| `-v, --volume` | — | none | `HOST:GUEST` volume mount; repeatable. |
| `--now` | — | `false` | Start the machine after initializing it. |

### nimbus machine start

```bash
nimbus machine start [flags] [name]
```

Starts a machine, creating it first when it does not exist. Accepts the same
creation flags as `machine init` (`--cpus`, `--memory`, `--disk-size`,
`--image`, `--identity`, `--ignition-path`, `--firmware`, `--volume`), which
apply only when start creates the machine, plus:

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-q, --quiet` | — | `false` | Suppress machine starting status output. |
| `--no-info` | — | `false` | Suppress informational tips. |

### nimbus machine stop

```bash
nimbus machine stop [name]
```

Stops a running machine and persists the stopped state. No flags beyond the
optional name.

### nimbus machine status

```bash
nimbus machine status [flags] [name]
```

Displays machine status.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-f, --format` | — | `table` | Output format: `json`, `yaml`, or `table`. |
| `-q, --quiet` | — | `false` | Print the machine name only. |
| `-n, --noheading` | — | `false` | Omit table headings from table output. |

### nimbus machine list

```bash
nimbus machine list [flags]
```

Lists initialized machines (alias: `nimbus machine ls`). The default machine
is marked with `*` in table output.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-f, --format` | — | `table` | Output format: `json` or `table`. |
| `-q, --quiet` | — | `false` | Print machine names only. |
| `-n, --noheading` | — | `false` | Omit table headings from table output. |

### nimbus machine info

```bash
nimbus machine info [-f <format>]
```

Displays machine host info: roots, cache locations, and the current machine
release.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-f, --format` | — | `yaml` | Output format: `json` or `yaml`. |

### nimbus machine inspect

```bash
nimbus machine inspect [-f <format>] [name]
```

Prints the persisted machine record (config plus refreshed state).

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-f, --format` | — | `json` | Output format: `json` or `yaml`. |

### nimbus machine set

```bash
nimbus machine set [flags] [name]
```

Updates a stopped machine's recorded resources; the next `machine start`
applies them.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-c, --cpus` | — | unchanged | Number of CPUs. |
| `-m, --memory` | — | unchanged | Memory in MiB. |
| `-d, --disk-size` | — | unchanged | Disk size in GiB. |

### nimbus machine cp

```bash
nimbus machine cp [-q] <src-path> <dest-path>
```

Securely copies files between the host and a machine. Guest endpoints use
`NAME:/path` notation (e.g. `default:/tmp/file`).

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `-q, --quiet` | — | `false` | Suppress copy status output. |

### nimbus machine ssh

```bash
nimbus machine ssh [name] [command...]
```

Logs in to a machine using SSH, optionally running a command. If the first
argument names an initialized machine it is treated as the machine name;
otherwise all arguments are passed through as the guest command on the
default machine.

### nimbus machine rm

```bash
nimbus machine rm [name]
```

Removes an existing machine's config, state, and runtime layout when it is
not running. No flags beyond the optional name.

### nimbus machine os

Manages machine OS images.

```bash
nimbus machine os apply <image> [--restart]
nimbus machine os upgrade [--dry-run] [--restart]
nimbus machine os rollback [--restart]
```

| Subcommand | Flag | Default | What it does |
| --- | --- | --- | --- |
| `apply <image>` | — | required | OCI image reference or digest to use on the next boot. |
| `apply` | `--restart` | `false` | Restart the machine immediately if it is running. |
| `upgrade` | `--dry-run` | `false` | Check whether an upgrade is available without applying it. |
| `upgrade` | `--restart` | `false` | Restart the machine immediately if an upgrade is applied. |
| `rollback` | `--restart` | `false` | Restart the machine immediately after queuing rollback. |

`upgrade` switches to the supported machine OS image for the current nimbus
release; `rollback` queues the previous bootc deployment for the next boot.

## nimbus node

Manages Nimbus node service-manager installation artifacts (systemd units
and Podman Quadlet files) on Linux hosts, and runs the node-side workload
reconciler. The artifact subcommands select a target with `--systemd`
(native unit) or `--container` (Quadlet), and a scope with `--user` or
`--system` (default: system). `node install` requires an explicit target;
the other artifact subcommands default to `--systemd`. See
[Run Nimbus as a service](/operators/node-lifecycle/) and
[Deploy on Linux](/operators/deploy-linux/).

### nimbus node run

```bash
nimbus node run --tenant <id> --workload <name> --exec </abs/path> \
  [--arg <v>]... [--bus system|session] --status-path <file.jsonl> \
  [--interval-secs 10] [--once] [--stop]
```

Reconciles one tenant workload to its desired state as a systemd
transient unit (Linux only). The workload is admitted through the tenant
isolation authority, then a converge loop starts, observes, or stops the
unit (`--stop` converges to stopped; `--once` runs a single pass) and
appends status evidence to the JSONL file after every pass. Production
units drive the system bus; `--bus session` targets `systemctl --user`.

### nimbus node install

```bash
nimbus node install --systemd|--container [flags]
```

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--systemd` | — | — | Install native systemd units for a host binary. |
| `--container` | — | — | Install a Quadlet `.container` file for the Nimbus OCI image. |
| `--user` / `--system` | — | system | Service-manager scope to install into. |
| `--binary` | — | `/usr/local/bin/nimbus` | Trusted Nimbus binary path (native installs only). |
| `--image` | — | required with `--container` | Nimbus OCI image reference (Quadlet installs only). |
| `--socket-activation` | — | `false` | Render a matching `nimbus.socket` and start from systemd's inherited TCP listener (native only). |
| `--enable` | — | `false` | Enable the generated service after writing artifacts. |
| `--now` | — | `false` | Start the generated service after writing artifacts. |
| `--overwrite` | — | `false` | Replace existing generated artifacts. |
| `--dry-run` | — | `false` | Print generated artifacts without writing files or calling systemctl. |

### nimbus node status

```bash
nimbus node status [--systemd|--container] [--user|--system]
```

Shows the Nimbus node service status through systemd. Target and scope flags
only.

### nimbus node logs

```bash
nimbus node logs [--systemd|--container] [--user|--system] [--follow]
```

Prints Nimbus node service logs through journalctl.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--follow` | — | `false` | Follow appended logs. |

### nimbus node doctor

```bash
nimbus node doctor [--systemd|--container] [--user|--system]
```

Diagnoses host support for the selected node service mode. Target and scope
flags only.

### nimbus node uninstall

```bash
nimbus node uninstall [--systemd|--container] [--user|--system] [--dry-run]
```

Removes Nimbus node service-manager artifacts.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--dry-run` | — | `false` | Print the files and commands without mutating the host. |

## nimbus compose

Compose-backed local service lifecycle commands. All subcommands share one
discovery rule for `--file`: explicit repeated `--file` flags win; otherwise
the `COMPOSE_FILE` environment variable provides an ordered list (separator
overridable with `COMPOSE_PATH_SEPARATOR`); otherwise Nimbus discovers
Compose files from the current directory and parents. Lifecycle subcommands
default to a deterministic per-project tenant, overridable with `--tenant`.

### nimbus compose config

```bash
nimbus compose config [--file <path>]... [--services]
```

Validates and prints the resolved service plan from a Compose file.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | `COMPOSE_FILE` | discovered | Compose files to read in order; repeat to merge overlays. |
| `--services` | — | `false` | Print only service names, one per line. |

### nimbus compose up

```bash
nimbus compose up [service] [--file <path>]... [--tenant <id>]
```

Starts one or more declared services for the current Compose project. When
the service name is omitted, starts all declared services.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | `COMPOSE_FILE` | discovered | Compose files to read in order; repeat to merge overlays. |
| `--tenant` | — | project tenant | Tenant override. |

### nimbus compose down

```bash
nimbus compose down [service] [--file <path>]... [--tenant <id>]
```

Stops one or more persisted services for the current Compose project. When
the service name is omitted, stops all persisted services in the tenant.
Same flags as `compose up`.

### nimbus compose ps

```bash
nimbus compose ps [-f <format>] [-n] [--file <path>]... [--all-tenants]
```

Shows persisted sandbox state for the current Compose project.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | `COMPOSE_FILE` | discovered | Compose files to read in order; repeat to merge overlays. |
| `-f, --format` | — | `table` | Output format: `json`, `yaml`, or `table`. |
| `-n, --noheading` | — | `false` | Omit table headings from table output. |
| `--all-tenants` | — | `false` | Show all tenants under the project-scoped backend root. |

### nimbus compose inspect

```bash
nimbus compose inspect <service> [-f <format>] [--file <path>]... [--tenant <id>]
```

Shows persisted sandbox details for one service.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | `COMPOSE_FILE` | discovered | Compose files to read in order; repeat to merge overlays. |
| `--tenant` | — | project tenant | Tenant override. |
| `-f, --format` | — | `json` | Output format: `json` or `yaml`. |

### nimbus compose logs

```bash
nimbus compose logs <service> [--file <path>]... [--tenant <id>] [--follow]
```

Prints persisted service logs for one service.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | `COMPOSE_FILE` | discovered | Compose files to read in order; repeat to merge overlays. |
| `--tenant` | — | project tenant | Tenant override. |
| `--follow` | — | `false` | Keep polling the persisted log file for appended output. |

### nimbus compose top

```bash
nimbus compose top <service> [-f <format>] [-n] [--file <path>]... [--tenant <id>]
```

Shows the persisted PID snapshot for one service.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | `COMPOSE_FILE` | discovered | Compose files to read in order; repeat to merge overlays. |
| `--tenant` | — | project tenant | Tenant override. |
| `-f, --format` | — | `table` | Output format: `json`, `yaml`, or `table`. |
| `-n, --noheading` | — | `false` | Omit table headings from table output. |

### nimbus compose export quadlet

```bash
nimbus compose export quadlet [flags]
```

Renders Podman Quadlet artifacts from an admitted Compose plan for operator
review.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | `COMPOSE_FILE` | discovered | Compose files to read in order; repeat to merge overlays. |
| `--service` | — | all services | Export only the named service; repeatable. |
| `--mode` | — | `containers` | Quadlet export shape: `containers`, `pod`, or `kube`. |
| `--podman-version` | — | unset | Podman version the operator targets; recorded in provenance. |
| `--output-dir` | — | stdout | Write artifacts to a directory instead of printing them. |
| `--overwrite` | — | `false` | Replace existing artifact files under `--output-dir`. |
| `--strict` | — | `false` | Treat every export warning as an error. |

## nimbus policy

Validates and explains Nimbus operator policy files. See
[Manage tenants](/operators/tenant-isolation/) for the policy model.

```bash
nimbus policy validate --file nimbus.policy.yaml [-f text|json]
nimbus policy explain --file nimbus.policy.yaml [-f text|json]
nimbus policy prove --file nimbus.policy.yaml [-f text|json]
nimbus policy diff --from before.yaml --to after.yaml [-f text|json]
```

| Subcommand | What it does |
| --- | --- |
| `validate` | Validate a Nimbus operator policy file. |
| `explain` | Explain the tenant-isolation decisions produced by a policy file. |
| `prove` | Prove policy advisories and accepted-risk status. |
| `diff` | Show authority changes between two policy files. |

Flags for `validate`, `explain`, and `prove`:

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--file` | — | required | Path to a `nimbus.policy.yaml` file. |
| `-f, --format` | — | `text` | Output format: `text` or `json`. |

Flags for `diff`:

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--from` | — | required | Previous policy file. |
| `--to` | — | required | Next policy file. |
| `-f, --format` | — | `text` | Output format: `text` or `json`. |

## nimbus encryption

Encryption admin commands for local at-rest encryption. These commands read
the active key provider and persistence settings from the same environment
variables and config file as `nimbus start` (`NIMBUS_ENCRYPTION_KEY_PROVIDER`
and friends; see [Configuration](/reference/configuration/)). Operational
guidance lives in [Encryption](/operators/encryption/).

### nimbus encryption status

```bash
nimbus encryption status [--format text|json]
```

Inspects encryption coverage and status.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--format` | — | `text` | Output format: `text` or `json`. |

### nimbus encryption migrate

```bash
nimbus encryption migrate --source <path> --provider <family> [flags]
```

Migrates a plaintext database to encrypted.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--source` | — | required | Path to the plaintext database to migrate. |
| `--target` | — | source + `.encrypted` | Path to the encrypted output database. |
| `--provider` | — | required | Provider family: `sqlite`, `redb`, or `libsql-cache`. |
| `--tenant-id` | — | unset | Tenant ID for tenant databases. |
| `--skip-validation` | — | `false` | Skip validation after migration. |
| `--retire-source` | — | `false` | Remove the source after successful migration. |

### nimbus encryption export

```bash
nimbus encryption export --source <path> --target <path> --provider <family> [--tenant-id <id>]
```

Exports an encrypted database to plaintext for recovery.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--source` | — | required | Path to the encrypted database to export. |
| `--target` | — | required | Path to the plaintext output database. |
| `--provider` | — | required | Provider family: `sqlite`, `redb`, or `libsql-cache`. |
| `--tenant-id` | — | unset | Tenant ID for tenant databases. |

### nimbus encryption rotate-kek

```bash
nimbus encryption rotate-kek --path <path> [flags]
```

Rotates key-encryption keys: rewraps manifests without rewriting data.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--path` | — | required | Path to the database or data directory. |
| `--provider` | — | unset | Provider family: `sqlite`, `redb`, or `libsql-cache`. |
| `--new-key-provider` | — | current provider | Replacement key provider: `master-key-file`, `key-dir`, or `aws-kms`. |
| `--new-master-key-file` | — | unset | New master key file when rotating to `master-key-file`. |
| `--new-key-dir` | — | unset | New key directory when rotating to `key-dir`. |
| `--new-aws-kms-key-id` | — | unset | AWS KMS key ID or alias when rotating to `aws-kms`. |
| `--new-aws-region` | — | unset | AWS region override when rotating to `aws-kms`. |
| `--new-aws-endpoint-url` | — | unset | AWS endpoint override when rotating to `aws-kms`. |
| `--all` | — | `false` | Rotate all manifests in the directory. |

### nimbus encryption rotate-dek

```bash
nimbus encryption rotate-dek --path <path> --provider <family> [flags]
```

Rotates data-encryption keys; provider-specific and may rewrite data.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--path` | — | required | Path to the encrypted database. |
| `--provider` | — | required | Provider family: `sqlite`, `redb`, or `libsql-cache`. |
| `--tenant-id` | — | unset | Tenant ID for tenant databases. |
| `--skip-backup` | — | `false` | Skip backup before rotation. |

## nimbus packages

Provisions the embedded Nimbus JS packages into an app's
`.nimbus/packages/` directory so `file:` package specifiers resolve offline.

### nimbus packages provision

```bash
nimbus packages provision [target] [--app-dir <path>]
```

| Argument / flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `[target]` | — | `all` | What to provision: `all` or an adapter (`convex`, `firebase`, `mongodb`, `dynamodb`, `nimbus`); dependencies are included automatically. |
| `--app-dir` | — | `.` | App directory to provision into. |

### nimbus packages verify

```bash
nimbus packages verify [--app-dir <path>]
```

Verifies provisioned package bytes against the binary's embedded checksums.

| Flag | Env var | Default | What it does |
| --- | --- | --- | --- |
| `--app-dir` | — | `.` | App directory whose `.nimbus/packages/` is verified. |
