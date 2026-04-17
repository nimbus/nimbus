# Plan: Machine CLI Developer Experience

Canonical execution plan for improving the neovex CLI developer experience to
match best-in-class tooling in the developer VM/machine space. Items were
identified through comparative review of Podman machine, Lima, Colima, and
OrbStack CLIs, and assessed against the current neovex CLI surface.

Reviewed against:

- `crates/neovex-bin/src/main.rs` — top-level `Cli` struct and `Command` enum
- `crates/neovex-bin/src/machine/mod.rs` — `MachineSubcommand` enum,
  `MachineInitCommand` args, `MachineStatusView`, `render_machine_view()`
- `crates/neovex-bin/src/machine/manager.rs` — `build_ssh_command()`,
  `ssh_identity_path` usage throughout
- `crates/neovex-bin/src/machine/bootstrap.rs` — ignition generation,
  `identity_path` / `public_key_path()` internals
- `crates/neovex-bin/src/service/mod.rs` — `ServiceSubcommand` enum for DX
  parity reference
- Podman CLI (`podman machine init --now`, `--identity`, `--ignition-path`,
  `podman --version`, `podman machine inspect --format`, `podman machine list`)
- Colima CLI (`colima start` as combined create+start)
- Lima CLI (`limactl --format`, `--quiet`, `--yes`)
- OrbStack CLI (`orb version`, `orb run`)

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** prompted by comparative DX audit on 2026-04-16
- **Related plans:**
  - `docs/reference/macos-machine-flow.md` — current macOS delivery contract;
    that reference owns architecture and workflow, this plan owns CLI surface
    polish
  - `docs/plans/archive/macos-machine-support-plan.md` — completed macOS
    closeout record with the historical execution and proof context behind the
    current contract
  - `docs/plans/machine-lifecycle-hardening-plan.md` — active hardening plan;
    that plan owns reliability, this plan owns ergonomics

## Current Assessed State

- The CLI surface (`neovex serve|machine|service`) is functional but lacks
  standard ergonomic features that users expect from modern developer tools.
- `neovex --version` does not work (`unexpected argument`).
- Machine subcommand help text uses implementation language instead of
  user-facing language (e.g., "Validate persisted machine state and prepare
  runtime roots for startup" for `start`).
- Flag names diverge from Podman conventions despite mirroring Podman's
  architecture (`--ssh-identity` vs Podman's `--identity`).
- `init` and `start` are separate commands with no shortcut, requiring two
  commands to go from zero to running.
- Machine status output is YAML-only with no structured output option.
- No `list` subcommand (needed before multi-machine support).
- No `inspect` subcommand for programmatic machine config access.
- No `set` subcommand for reconfiguring without `rm` + `init`.
- No `--quiet`/`-q` flag on output-producing subcommands for scripting.
- No `cp` subcommand for host↔guest file transfer.
- Machine name is hardcoded — no way to target a non-default machine.
- Init flag help text uses the same implementation jargon as subcommand help.
- No short flag aliases for common init options.
- Top-level about text ("Reactive document database") does not reflect the
  machine/service surface.

## Current Review Findings

### 1. No `--version` flag — missing

**Every comparable tool** supports version introspection at the top level:
`podman --version`, `limactl --version`, `colima version`, `orb version`.

**Neovex current:** `neovex --version` produces `error: unexpected argument`.

**Risk:** Users cannot verify which version is installed, cannot include version
in bug reports, and cannot script upgrade verification. The default machine
image is version-pinned to `v{CARGO_PKG_VERSION}`, making version visibility
especially important for troubleshooting image mismatches.

**Fix:** Add `#[command(version)]` to the `Cli` struct in `main.rs`. Clap
derives the version from `Cargo.toml` automatically.

### 2. Help text uses implementation language — confusing

**Podman/Lima/Colima** use terse, user-facing descriptions:
- `podman machine start` → "Start an existing machine"
- `podman machine stop` → "Stop an existing machine"
- `podman machine init` → "Initialize a new virtual machine"

**Neovex current:**
- `start` → "Validate persisted machine state and prepare runtime roots for
  startup"
- `stop` → "Validate persisted machine state before a future graceful stop"
- `init` → "Initialize the local machine config and state roots"
- `ssh` → "Show the future guest SSH target once host orchestration is
  available"
- `rm` → "Remove the local machine config, state, and runtime roots"

**Risk:** Users see implementation details they do not need or understand.
"Validate persisted machine state before a future graceful stop" tells the user
nothing about what the command does from their perspective.

**Fix:** Rewrite all machine subcommand help strings to match Podman's style:

| Subcommand | Current | Target |
| --- | --- | --- |
| `init` | Initialize the local machine config and state roots | Initialize a new machine |
| `start` | Validate persisted machine state and prepare runtime roots for startup | Start an existing machine |
| `stop` | Validate persisted machine state before a future graceful stop | Stop a running machine |
| `status` | Show the current machine config, state, and derived paths | Display machine status |
| `ssh` | Show the future guest SSH target once host orchestration is available | Log in to a machine using SSH |
| `rm` | Remove the local machine config, state, and runtime roots | Remove an existing machine |

### 3. Flag naming diverges from Podman — inconsistent

Neovex mirrors Podman's architecture (krunkit, ignition, FCOS guest, SSH
identity for guest access) but uses different flag names:

| Neovex flag | Podman flag | Semantic match? |
| --- | --- | --- |
| `--ssh-identity` | `--identity` | Yes — both accept a path to an SSH private key for guest access |
| `--ignition-file` | `--ignition-path` | Yes — both accept a path to a Butane/Ignition config file |
| `--efi-store` | (none) | N/A — Podman does not expose this; `--firmware` is more standard across VM tools |
| `--memory-mib` | `--memory` (in MiB) | Yes — same unit, different name |
| `--disk-gib` | `--disk-size` (in GiB) | Yes — same unit, different name |

**Risk:** Users familiar with Podman must re-learn flag names for semantically
identical concepts.

**Fix:** Rename to align with Podman where the semantics match:

| Current | New | Rationale |
| --- | --- | --- |
| `--ssh-identity` | `--identity` | Podman convention; shorter |
| `--ignition-file` | `--ignition-path` | Podman convention |
| `--efi-store` | `--firmware` | More standard across VM tools (QEMU, libvirt); Podman does not expose this at all |
| `--memory-mib` | `--memory` | Podman convention; keep MiB as the unit (document in help text) |
| `--disk-gib` | `--disk-size` | Podman convention; keep GiB as the unit (document in help text) |

Internal field names (`ssh_identity_path`, `memory_mib`, `disk_gib`, etc.) can
remain unchanged — these renames are CLI-surface only.

### 4. No combined init+start flow — friction

**Colima** merges create and start into `colima start` (create-if-not-exists,
then start). **Podman** keeps them separate but offers `init --now` to combine
them.

**Neovex current:** Going from zero to running requires two commands:
```
neovex machine init
neovex machine start
```

**Risk:** Every new user must learn the two-step dance. Friction at first
contact is the highest-cost friction.

**Fix:** Implement both patterns:
1. `start` creates-if-not-exists (Colima pattern) — `start` on an
   uninitialized machine runs `init` with defaults, then starts
2. `init --now` (Podman pattern) — `init --now` runs `start` immediately after
   initialization

This satisfies both Colima-style users who want one command and Podman-style
users who expect `init --now`. The Colima-style `start` also keeps the
quick-start documentation simple: "run `neovex machine start`".

### 5. No `--format` on status output — not scriptable

**Podman** supports `--format json` and Go templates on `list`, `inspect`, and
`info`. **Lima** supports `--format json|yaml|table` and Go templates on
`list`. **Docker** supports `--format json` on `inspect`.

**Neovex current:** `render_machine_view()` always outputs YAML via
`serde_yaml::to_string()`. No way to get JSON or table output.

**Risk:** Users cannot script against machine status output. YAML is unusual as
the only output format for CLI tools in this space.

**Fix:** Add `--format` flag to `status` accepting `json`, `yaml`, and `table`.
Default to `table` (human-readable) with `json` and `yaml` available for
scripting. `table` should be a condensed human-readable view, not the full
serialized config.

### 6. No `list` subcommand — needed for multi-machine

**Podman** (`podman machine list`/`ls`), **Lima** (`limactl list`), **Colima**
(`colima list`), and **OrbStack** (`orb list`) all provide list subcommands.

**Neovex current:** No `list` subcommand. Only one machine (`default`) is
supported today.

**Risk:** When multi-machine support arrives (especially for the Windows WSL2
provider where `requires_exclusive_active = false`), users need a way to see
all machines. Adding it now establishes the pattern.

**Fix:** Add `list` (with `ls` alias) that shows a table of machines with name,
status, provider, CPUs, memory, disk. Support `--format json|table` and
`--quiet` (names only).

### 7. No `inspect` subcommand — no programmatic config access

**Podman** (`podman machine inspect`) outputs full JSON machine config.
**Docker** (`docker context inspect`) does the same. Both support `--format`.

**Neovex current:** `status` shows a YAML view that mixes config, state, and
derived paths. No way to get the raw config or state records programmatically.

**Fix:** Add `inspect` that outputs the full `MachineConfigRecord` and
`MachineStateRecord` as JSON by default. Support `--format json|yaml`.

### 8. No `set` subcommand — reconfiguration requires rm+init

**Podman** (`podman machine set`) allows changing CPUs, memory, disk, rootful
mode, and USB devices on a stopped machine without recreating it.

**Neovex current:** Changing any machine config requires `rm` + `init`, losing
any non-default settings that the user does not remember to re-specify.

**Fix:** Add `set` that accepts `--cpus`, `--memory`, `--disk-size` on a
stopped machine. Validates, updates `config.json`, and shows the updated
config.

### 9. Machine name hardcoded — no multi-machine targeting

**Podman** (`podman machine start [name]`), **Lima** (`limactl start [name]`),
and **OrbStack** (`orb start [name]`) all accept a machine name as an optional
positional argument, defaulting to `default`.

**Neovex current:** `DEFAULT_MACHINE_NAME` is hardcoded throughout
`mod.rs` (30 references). Every subcommand operates on the `default` machine
with no way to target a different one.

**Risk:** Without machine-name targeting, the `list` subcommand (DX6) is
cosmetic — users can see machines but cannot operate on a specific one. This is
also a prerequisite for multi-machine support (Windows WSL2 provider allows
concurrent machines).

**Fix:** Add an optional positional `[NAME]` argument to all machine
subcommands that currently hardcode `DEFAULT_MACHINE_NAME`. Default to
`"default"` when omitted. Thread the name through `roots.paths(name)` instead
of `roots.paths(DEFAULT_MACHINE_NAME)`.

### 10. Init flag help text uses implementation language

DX2 (finding 2) covers subcommand-level help strings, but the init flag help
text is equally jargon-heavy:

| Flag | Current help text | Target |
| --- | --- | --- |
| `--cpus` | Guest vCPU count to record in the machine config | Number of CPUs |
| `--memory` | Guest memory size in MiB to record in the machine config | Memory in MiB |
| `--disk-size` | Guest disk size in GiB to record in the machine config | Disk size in GiB |
| `--image` | Guest image source. Accepts a published OCI reference, an absolute local raw-disk path, or an http(s) URL override for diagnostics | Machine OS image |
| `--identity` | Optional SSH identity path used for direct guest debugging on bootable local disk images | Path to SSH identity for guest access |
| `--ignition-path` | Optional first-boot Ignition file to serve over the guest bootstrap vsock channel | Path to Ignition config file |
| `--firmware` | Optional EFI variable-store path for booting an existing disk with its known-good firmware state | Path to EFI variable store |
| `--volume` | Host:guest volume mapping to record for future virtiofs setup | Host:guest volume mount |

**Fix:** Fold flag help text rewrites into DX2 scope (they are the same kind
of change).

### 11. No short flags for common init options

**Colima** uses `-c` (cpus), `-m` (memory), `-d` (disk), `-V` (mount).
**Podman** uses `-m` (memory), `-v` (volume).

**Neovex current:** All init flags are long-only.

**Risk:** Low — long flags are correct and discoverable. But short flags for
the most common options reduce typing during interactive use.

**Fix:** Add short aliases for the most common flags: `-c` (cpus), `-m`
(memory), `-d` (disk-size), `-v` (volume). Fold into DX3 scope since they are
flag-surface changes.

### 12. Top-level about text incomplete

**Neovex current:** `#[command(about = "Reactive document database")]` — the
about text does not mention machine management or service orchestration, which
are now first-class subcommands.

**Risk:** Low — but this is the first line users see in `neovex --help`.

**Fix:** Update to something like "Reactive document database with machine and
service orchestration". Fold into DX2 scope.

### 13. No `--quiet`/`-q` flag — not scriptable

**Podman** supports `--quiet`/`-q` on `machine list` (names only) and
`container list` (IDs only). **Lima** supports `--quiet` on `list`.

**Neovex current:** `--quiet` is not available on any machine subcommand. The
`list` item (DX6) adds it for `list` only.

**Risk:** Shell scripts that want machine names for iteration
(`for m in $(neovex machine ls -q); do ...`) cannot suppress verbose output.

**Fix:** Add `--quiet`/`-q` to `status`, `list`, and `inspect`. On `status`
and `list`, quiet mode prints only the machine name(s). On `inspect`, quiet
mode is not applicable (omit). This flag is orthogonal to `--format` and
suppresses all output except the minimal identifier.

### 14. No `cp` subcommand — no host↔guest file transfer

**Podman** (`podman machine cp`) allows copying files between host and guest
without the user needing to know SSH details, port, or identity path.

**Neovex current:** Users must manually construct `scp` commands using the
SSH identity, port, and user from `neovex machine status`.

**Risk:** Low — this is a debugging convenience, not a core workflow. But it
removes a friction point for users who need to inspect guest filesystem state.

**Fix:** Add `cp` subcommand that wraps `scp` using the machine's configured
SSH identity, allocated port, and SSH user. Accept `host:path` and
`machine:path` syntax matching Podman's convention.

---

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Roadmap Status Ledger` and `Execution Log`
3. `crates/neovex-bin/src/main.rs` and `crates/neovex-bin/src/machine/mod.rs`

General rules:

- This plan owns CLI surface ergonomics. It does not own machine architecture
  (macOS plan), machine reliability (hardening plan), or machine guest content
  (neovex-machine-os).
- Flag renames are breaking changes. Per CLAUDE.md's pre-launch policy,
  breaking changes are preferred over compatibility layers. Do not add old-name
  aliases — just rename.
- Help text changes do not require test changes unless tests assert on help
  output.
- Every code change must pass `cargo fmt --all --check`, focused `cargo check`,
  and focused tests for touched modules.
- Update this plan's ledger and execution log in the same change set.

## Scope

This plan covers:

- top-level `--version` flag
- all user-visible help text rewrites (subcommands, flags, top-level about)
- machine flag renames (`--identity`, `--ignition-path`, `--firmware`,
  `--memory`, `--disk-size`) and short aliases (`-c`, `-m`, `-d`, `-v`)
- combined init+start flow (`start` creates-if-not-exists, `init --now`)
- machine name as optional positional argument on all subcommands
- `--format` output flag on `status`
- `list`/`ls` subcommand
- `inspect` subcommand
- `set` subcommand
- `--quiet`/`-q` flag on output-producing subcommands
- `cp` subcommand for host↔guest file transfer

This plan does not cover:

- machine architecture or guest image changes
- machine lifecycle hardening (owned by hardening plan)
- `neovex serve` or `neovex service` DX changes
- interactive prompts or TUI features

## Verification Contract

### Minimum verification for every code item

- `cargo fmt --all --check`
- focused `cargo check -p neovex-bin`
- focused `cargo test -p neovex-bin` for touched modules
- plan ledger and execution-log update in the same change set

### Required verification lanes

- **Version lane**
  - `neovex --version` prints the version from `Cargo.toml`

- **Help text lane**
  - `neovex machine --help` shows user-facing descriptions
  - no implementation jargon in any visible help text

- **Flag rename lane**
  - old flag names (`--ssh-identity`, `--ignition-file`, `--efi-store`,
    `--memory-mib`, `--disk-gib`) are removed, not aliased
  - new flag names work correctly in init and test fixtures
  - short aliases (`-c`, `-m`, `-d`, `-v`) work
  - internal field names (`ssh_identity_path`, `memory_mib`, `disk_gib`) remain
    unchanged
  - error messages referencing old flag names are updated

- **Init+start lane**
  - `neovex machine start` on uninitialized machine creates with defaults and
    starts
  - `neovex machine init --now` initializes and starts in one command
  - `neovex machine init` without `--now` still works (init only)
  - `neovex machine start` on initialized machine still works (start only)

- **Format lane**
  - `neovex machine status --format json` outputs valid JSON
  - `neovex machine status --format yaml` outputs valid YAML
  - `neovex machine status` defaults to table output

- **List lane**
  - `neovex machine list` shows machine table
  - `neovex machine ls` alias works
  - `--format json` and `--quiet` flags work

- **Inspect lane**
  - `neovex machine inspect` outputs full config+state as JSON
  - `--format yaml` alternative works

- **Set lane**
  - `neovex machine set --cpus 4` on a stopped machine updates the config
  - `neovex machine set` on a running machine produces a clear error
  - updated config is visible in `inspect` and `status`

- **Machine name lane**
  - `neovex machine start` operates on `default` (backwards compatible)
  - `neovex machine start my-machine` operates on `my-machine`
  - all error messages include the machine name

- **Copy lane**
  - `neovex machine cp ./local machine:/remote` copies to guest
  - `neovex machine cp machine:/remote ./local` copies from guest
  - machine must be running with SSH identity configured

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| DX1 | done | Added Clap's top-level `#[command(version)]` support on `Cli`; `neovex --version` now prints the Cargo package version and has focused parser coverage | none |
| DX2 | todo | Rewrite all user-visible help text to user-facing language: subcommand descriptions, init flag descriptions, and top-level about text | none |
| DX3 | todo | Rename machine init flags (`--identity`, `--ignition-path`, `--firmware`, `--memory`, `--disk-size`) and add short aliases (`-c`, `-m`, `-d`, `-v`) | none |
| DX4 | todo | Combined init+start: `start` creates-if-not-exists with defaults; `init --now` runs start after init | none |
| DX5 | todo | Add `--format json\|yaml\|table` to `status` subcommand; default to table | none |
| DX6 | todo | Add `list`/`ls` subcommand with `--format` and `--quiet` | DX5 (shared format infrastructure) |
| DX7 | todo | Add `inspect` subcommand with `--format json\|yaml` | DX5 (shared format infrastructure) |
| DX8 | todo | Add `set` subcommand for stopped-machine reconfiguration (`--cpus`, `--memory`, `--disk-size`) | none |
| DX9 | todo | Add `--quiet`/`-q` flag to `status` and `list` for minimal scripting output | DX5 (shared format infrastructure), DX6 |
| DX10 | todo | Add `cp` subcommand for host↔guest file transfer wrapping `scp` | none |
| DX11 | todo | Add optional positional `[NAME]` argument to all machine subcommands, defaulting to `"default"` | none |

## Implementation Checkpoints

### DX1 — Add `--version` flag

Repo outputs:

- `#[command(version)]` attribute on `Cli` struct in `main.rs`

Acceptance criteria:

- `neovex --version` prints `neovex {version}` where version matches
  `Cargo.toml`

### DX2 — Rewrite all user-visible help text

Repo outputs:

- updated doc comments on `MachineSubcommand` variants in `mod.rs`
- updated doc comments on `MachineInitCommand` fields (flag-level help)
- updated top-level `about` text on `Cli` struct in `main.rs`

Acceptance criteria:

- `neovex --help` about text reflects the full CLI surface (not just "database")
- `neovex machine --help` shows terse, user-facing descriptions
- `neovex machine init --help` shows terse, user-facing flag descriptions
- no references to "persisted machine state", "runtime roots", "future
  graceful stop", "to record in the machine config", "for future virtiofs
  setup", or "over the guest bootstrap vsock channel" in any user-visible text

### DX3 — Rename machine init flags and add short aliases

Repo outputs:

- `--ssh-identity` → `--identity` (clap `#[arg(long)]` rename)
- `--ignition-file` → `--ignition-path` (clap rename)
- `--efi-store` → `--firmware` (clap rename)
- `--memory-mib` → `--memory` (clap rename, help text notes MiB unit)
- `--disk-gib` → `--disk-size` (clap rename, help text notes GiB unit)
- short aliases: `-c` (cpus), `-m` (memory), `-d` (disk-size), `-v` (volume)
- internal struct field names unchanged
- all test fixtures updated to use new flag names
- error messages referencing old flag names updated (e.g., `manager.rs` line
  857: `re-run neovex machine init --ssh-identity <path>` → `--identity`)

Acceptance criteria:

- old flag names produce "unexpected argument" errors
- new flag names work correctly
- short aliases work (`neovex machine init -c 4 -m 4096 -d 40`)
- `neovex machine init --help` shows new flag names with unit documentation

### DX4 — Combined init+start flow

Repo outputs:

- `run_machine_start()` detects uninitialized state and runs init with defaults
  before starting
- `--now` flag on `MachineInitCommand` that calls `run_machine_start()` after
  init completes
- `start` inherits init's resource flags (`--cpus`, `--memory`, `--disk-size`,
  `--identity`, `--ignition-path`, `--firmware`, `--volume`) for the
  create-if-not-exists path

Acceptance criteria:

- `neovex machine start` on a clean system initializes with defaults and starts
- `neovex machine init --now --cpus 4` initializes with 4 CPUs and starts
- `neovex machine start` on an already-initialized machine starts without
  re-initializing
- `neovex machine init` without `--now` initializes without starting

### DX5 — Output format flag on `status`

Repo outputs:

- `OutputFormat` enum: `Json`, `Yaml`, `Table`
- `--format` flag on `MachineStatusCommand`
- `render_machine_view()` updated to accept format parameter
- table format: condensed human-readable output (name, status, provider,
  CPUs, memory, disk)
- json/yaml format: full `MachineStatusView` serialization

Acceptance criteria:

- `neovex machine status` defaults to table output
- `neovex machine status --format json` outputs valid JSON
- `neovex machine status --format yaml` outputs valid YAML
- backwards-compatible: existing scripts parsing YAML will need to switch to
  `--format yaml` (acceptable per pre-launch breaking-change policy)

### DX6 — List subcommand

Repo outputs:

- `List` variant in `MachineSubcommand` with `ls` alias
- `--format json|table` flag
- `--quiet` / `-q` flag (names only)
- scans machine config root for initialized machines

Acceptance criteria:

- `neovex machine list` and `neovex machine ls` show machine table
- `--format json` outputs JSON array of machine summaries
- `--quiet` outputs machine names only, one per line

### DX7 — Inspect subcommand

Repo outputs:

- `Inspect` variant in `MachineSubcommand`
- outputs full `MachineConfigRecord` + `MachineStateRecord` as JSON by default
- `--format json|yaml` flag

Acceptance criteria:

- `neovex machine inspect` outputs valid JSON combining config and state
- `--format yaml` outputs the same data as YAML
- output includes all fields (version, provider, guest config, resources,
  volumes, roots, lifecycle, manager state, runtime state)

### DX8 — Set subcommand

Repo outputs:

- `Set` variant in `MachineSubcommand`
- `--cpus`, `--memory`, `--disk-size` flags (all optional)
- validates machine is stopped before applying
- updates `config.json` atomically
- prints updated config summary

Acceptance criteria:

- `neovex machine set --cpus 4` on a stopped machine updates the config
- `neovex machine set --memory 4096` on a stopped machine updates the config
- `neovex machine set` on a running machine produces a clear error
- `neovex machine set` with no flags produces a helpful usage message
- updated values visible in `inspect` and `status`

### DX9 — Quiet flag on output-producing subcommands

Repo outputs:

- `--quiet` / `-q` flag on `MachineStatusCommand` and `MachineListCommand`
- quiet mode on `status`: prints only the machine name
- quiet mode on `list`: prints machine names, one per line
- `--quiet` is orthogonal to `--format` and takes precedence when both are
  specified

Acceptance criteria:

- `neovex machine status -q` prints the machine name only
- `neovex machine list -q` prints machine names, one per line
- `for m in $(neovex machine ls -q); do echo $m; done` works in shell scripts

### DX10 — Copy subcommand for host↔guest file transfer

Repo outputs:

- `Cp` variant in `MachineSubcommand`
- wraps `scp` using the machine's configured SSH identity, allocated port, and
  SSH user
- accepts positional `SRC` and `DST` arguments
- `machine:path` prefix indicates guest-side path; bare paths are host-side
- reuses the same SSH options as `build_ssh_command()` (strict host key
  checking disabled, connection timeout, identity path)

Acceptance criteria:

- `neovex machine cp ./local-file machine:/tmp/remote-file` copies to guest
- `neovex machine cp machine:/tmp/remote-file ./local-file` copies from guest
- missing SSH identity produces a clear error (same as `ssh` subcommand)
- machine must be running (same check as `ssh` subcommand)

### DX11 — Machine name positional argument

Repo outputs:

- optional positional `[NAME]` argument on all machine subcommands (`init`,
  `start`, `stop`, `status`, `ssh`, `rm`, `set`, `inspect`, `cp`)
- default value: `"default"`
- thread the name through `roots.paths(name)` instead of
  `roots.paths(DEFAULT_MACHINE_NAME)` in all command handlers
- `list` does not take a name argument (it shows all machines)
- error messages include the machine name for clarity

Acceptance criteria:

- `neovex machine start` operates on the `default` machine (backwards
  compatible)
- `neovex machine start my-machine` operates on `my-machine`
- `neovex machine status my-machine` shows status for `my-machine`
- `neovex machine rm my-machine` removes `my-machine` only
- machine name appears in all error messages and status output

## Dependency Graph

- `DX1` through `DX4` have no dependencies on each other and can proceed in any
  order.
- `DX5` establishes shared `OutputFormat` infrastructure that `DX6`, `DX7`, and
  `DX9` reuse.
- `DX6` and `DX7` depend on `DX5` but are independent of each other.
- `DX8` has no dependencies.
- `DX9` depends on `DX5` (format infrastructure) and `DX6` (list subcommand).
- `DX10` has no dependencies.
- `DX11` has no hard dependencies but should land before `DX6` (list is not
  useful without the ability to target a machine by name).

## Recommended Delivery Order

1. `DX1` — `--version` (one-line change, immediate user value)
2. `DX2` — help text rewrites (no code behavior change, immediate polish)
3. `DX3` — flag renames + short aliases (breaking change, best done early)
4. `DX4` — combined init+start (highest-impact DX improvement)
5. `DX11` — machine name positional arg (prerequisite for list being useful)
6. `DX5` — output format flag (enables scriptability)
7. `DX8` — set subcommand (enables reconfiguration without rm+init)
8. `DX6` — list subcommand (prepares for multi-machine)
9. `DX7` — inspect subcommand (programmatic config access)
10. `DX9` — quiet flag (scripting convenience, after format and list land)
11. `DX10` — cp subcommand (debugging convenience)

Items 1-4 are the highest-priority batch — they remove the most friction from
the getting-started experience. Item 5 is an architectural prerequisite for
multi-machine. Items 6-9 add power-user and scripting features. Items 10-11
are low-priority polish.

## Execution Log

- 2026-04-16: Created this plan based on comparative DX audit against Podman
  machine, Lima, Colima, and OrbStack CLIs. Identified 8 DX gaps by comparing
  the neovex machine CLI surface against best-in-class tooling. Each gap has a
  concrete reference to comparable tool behavior and a specific fix. Confirmed
  that `--identity` matches Podman's semantics exactly (SSH private key path for
  guest access), `--ignition-path` matches Podman's flag name for ignition
  config, and `--firmware` is more standard than `--efi-variable-store` across
  VM tools (Podman does not expose EFI configuration at all). Full review
  added: DX9 (`--quiet`/`-q`), DX10 (`cp`), DX11 (machine name positional
  arg — prerequisite for multi-machine targeting), expanded DX2 to include init
  flag help text and top-level about text, and expanded DX3 to include short
  flag aliases (`-c`, `-m`, `-d`, `-v`).
- 2026-04-16: Closed `DX1` by adding Clap's top-level `version` attribute to
  `crates/neovex-bin/src/main.rs` and a focused parser regression proving
  `neovex --version` now exits through Clap's display-version path with the
  package version from `Cargo.toml`. Verified with `cargo test -p neovex-bin
  cli_supports_top_level_version_flag -- --test-threads=1` plus a direct
  `cargo run -p neovex-bin -- --version` smoke check. This closes the CLI DX
  task, while live macOS guest proof still depends on a Linux guest asset built
  from this updated source rather than the already-published `v0.1.8` asset.
