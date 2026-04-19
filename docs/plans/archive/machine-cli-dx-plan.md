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

- **Status:** `completed`
- **Primary owner:** this plan
- **Activation gate:** prompted by comparative DX audit on 2026-04-16
- **Related plans:**
  - `docs/reference/macos-machine-flow.md` — current macOS delivery contract;
    that reference owns architecture and workflow, this plan owns CLI surface
    polish
  - `docs/plans/archive/macos-machine-support-plan.md` — completed macOS
    closeout record with the historical execution and proof context behind the
    current contract
  - `docs/plans/archive/machine-lifecycle-hardening-plan.md` — archived
    shared hardening record; it captures the landed reliability baseline while
    this plan owns ongoing CLI ergonomics

## Current Assessed State

- The machine-facing CLI surface now covers the baseline ergonomic features
  expected from modern developer VM tooling while staying aligned with Podman
  where semantics match.
- `neovex --version` now works via Clap's top-level `version` support and has
  both focused parser coverage and real macOS/Homebrew proof on the released
  `v0.1.14` path.
- Top-level `--help`, `machine --help`, `machine os --help`, and
  `machine init --help` now use concise operator-facing language instead of
  implementation jargon.
- `machine init` now uses Podman-aligned flag spellings (`--identity`,
  `--ignition-path`, `--firmware`, `--memory`, `--disk-size`) and short
  aliases for the most common resource flags (`-c`, `-m`, `-d`, `-v`).
- `machine start` is now the primary create-or-start path: on a clean host it
  records the default machine contract and then boots it, while
  `machine init --now` provides the Podman-style explicit combined shortcut.
- On the macOS host-managed contract, `machine start` now auto-generates a
  machine-owned SSH identity under the Neovex machine data root when no
  explicit `--identity` override was recorded, so first-run convergence does
  not require a separate key-provisioning step.
- Machine lifecycle commands now accept an optional positional `[NAME]`,
  defaulting to `default`; `machine ssh` follows Podman's first-argument
  resolution and treats the first arg as a machine name only when that machine
  record actually exists.
- `machine status` now defaults to a condensed table view and supports
  `--format json|yaml|table` for structured or human-oriented output.
- `machine set` now provides Podman-style stopped-machine reconfiguration for
  `--cpus`, `--memory`, and `--disk-size`, updating the recorded machine
  contract without forcing `rm` + `init`.
- `machine list` / `machine ls` now enumerates initialized machine records from
  the config root and supports table, JSON, and quiet-name output.
- `machine inspect` now returns the persisted machine config and refreshed
  state record directly, defaulting to JSON and supporting YAML as the
  human-readable alternate.
- `machine status` now matches `machine list` with a names-only `--quiet` /
  `-q` mode that overrides the structured output format flags.
- `machine cp` now provides Podman-style host↔guest transfer through shared
  localhost SSH safety options, named-machine `NAME:/path` guest endpoints,
  and quiet/success-message handling appropriate for both operators and
  scripts.

## Current Review Findings

All DX items in this plan are now closed and validated. The finding writeups
below are preserved as the original audit record that drove the completed
implementation.

### 1. No `cp` subcommand — no host↔guest file transfer

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
  (macOS reference), the landed machine reliability baseline captured in the
  archived hardening plan, or machine guest content (`neovex-machine-os` /
  Podman image contract).
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
- all user-visible machine help text rewrites (subcommands, flags, top-level
  about, and the public `machine os` help surface)
- machine flag renames (`--identity`, `--ignition-path`, `--firmware`,
  `--memory`, `--disk-size`) and short aliases (`-c`, `-m`, `-d`, `-v`)
- combined init+start flow (`start` creates-if-not-exists, `init --now`)
- machine name as an optional positional argument across the named machine
  lifecycle commands, with `cp` using Podman-style `<machine>:/path` endpoints
- `--format` output flag on `status`
- `list`/`ls` subcommand
- `inspect` subcommand
- `set` subcommand
- `--quiet`/`-q` flag on output-producing subcommands
- `cp` subcommand for host↔guest file transfer

This plan does not cover:

- machine architecture or guest image changes
- machine lifecycle hardening beyond user-facing CLI ergonomics; the current
  reliability baseline is captured in the archived hardening plan
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
  - `neovex machine os --help` shows user-facing descriptions
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
- `neovex machine cp ./local default:/remote` copies to guest
- `neovex machine cp default:/remote ./local` copies from guest
  - machine must be running with SSH identity configured

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| DX1 | done | Added Clap's top-level `#[command(version)]` support on `Cli`; `neovex --version` now prints the Cargo package version and has focused parser coverage | none |
| DX2 | done | Rewrote top-level about text plus public `machine`, `machine os`, and `machine init` help text to concise user-facing language and added focused parser/help regression coverage | none |
| DX3 | done | Renamed `machine init` to Podman-aligned flags (`--identity`, `--ignition-path`, `--firmware`, `--memory`, `--disk-size`), added short aliases (`-c`, `-m`, `-d`, `-v`), and updated helper scripts/docs/error text to match | none |
| DX4 | done | Made `machine start` the primary create-or-start path, added `init --now`, threaded init-style resource overrides through create-if-missing start, and auto-generated a machine-owned macOS SSH identity when the host-managed contract needs one | none |
| DX5 | done | Added `--format json\|yaml\|table` to `machine status`, defaulted status output to a condensed table, and kept the full serialized status view available through JSON/YAML | none |
| DX6 | done | Added `list` / `ls` with table and JSON output plus `--quiet` names-only mode, backed by config-root machine enumeration and per-machine state refresh | DX5 (shared format infrastructure) |
| DX7 | done | Added `inspect` with JSON-by-default config+state output, YAML support, and named-machine targeting through the existing record-locking path | DX5 (shared format infrastructure) |
| DX8 | done | Added `set` for stopped-machine reconfiguration (`--cpus`, `--memory`, `--disk-size`), validated required-change and stopped-only guardrails, and persisted updated resource contracts back to `config.json` | none |
| DX9 | done | Added `--quiet` / `-q` to `status`, kept `list` aligned with the same precedence rule, and validated names-only scripting output on the real macOS machine root | DX5 (shared format infrastructure), DX6 |
| DX10 | done | Added Podman-aligned `cp` with machine-prefixed guest paths, shared localhost SSH safety options, recursive `scp` transport, and real host↔guest round-trip proof on macOS | none |
| DX11 | done | Added optional positional `[NAME]` targeting on the machine lifecycle surface, defaulting to `default`, and matched Podman's SSH first-argument resolution so existing machine names win over guest commands | none |

## Implementation Checkpoints

### DX1 — Add `--version` flag

Repo outputs:

- `#[command(version)]` attribute on `Cli` struct in `main.rs`

Acceptance criteria:

- `neovex --version` prints `neovex {version}` where version matches
  `Cargo.toml`

### DX2 — Rewrite all user-visible help text

Repo outputs:

- updated doc comments on public machine command types in `mod.rs`,
  including `MachineSubcommand`, `MachineOsSubcommand`, and any public
  machine-OS argument fields
- updated doc comments on `MachineInitCommand` fields (flag-level help)
- updated top-level `about` text on `Cli` struct in `main.rs`

Acceptance criteria:

- `neovex --help` about text reflects the full CLI surface (not just "database")
- `neovex machine --help` shows terse, user-facing descriptions
- `neovex machine os --help` shows terse, user-facing descriptions
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

- `--quiet` / `-q` flag on `MachineStatusCommand`
- quiet mode on `status`: prints only the selected machine name
- `status --quiet` follows the same precedence rule that `list` already uses:
  `--quiet` overrides `--format`

Acceptance criteria:

- `neovex machine status -q` prints the machine name only
- `neovex machine status --format json -q` still prints only the machine name
- existing `neovex machine list -q` behavior continues to print machine names,
  one per line
- `for m in $(neovex machine ls -q); do echo $m; done` works in shell scripts

### DX10 — Copy subcommand for host↔guest file transfer

Repo outputs:

- `Cp` variant in `MachineSubcommand`
- wraps `scp` using the machine's configured SSH identity, allocated port, and
  SSH user
- accepts positional `SRC_PATH` and `DEST_PATH` arguments
- `<machine>:/path` prefix indicates the guest-side path; bare paths are
  host-side
- rejects host↔host and machine↔machine copies, matching Podman's
  single-machine copy contract
- uses recursive `scp` by default and supports `--quiet` / `-q` to suppress
  the success message
- reuses the same localhost SSH safety options as `build_ssh_command()`
  (strict host key checking disabled, connection timeout, identity path)

Acceptance criteria:

- `neovex machine cp ./local-file default:/tmp/remote-file` copies to guest
- `neovex machine cp default:/tmp/remote-file ./local-file` copies from guest
- missing SSH identity produces a clear error (same as `ssh` subcommand)
- machine must be running (same check as `ssh` subcommand)
- copying between two machines is rejected
- copying between two host paths is rejected

### DX11 — Machine name positional argument

Repo outputs:

- optional positional `[NAME]` argument on all machine subcommands (`init`,
  `start`, `stop`, `status`, `ssh`, `rm`, `set`, `inspect`)
- default value: `"default"`
- thread the name through `roots.paths(name)` instead of
  `roots.paths(DEFAULT_MACHINE_NAME)` in all command handlers
- `list` does not take a name argument (it shows all machines)
- `cp` targets a named machine through the Podman-style `<machine>:/path`
  prefix instead of a separate positional name
- error messages include the machine name for clarity

Acceptance criteria:

- `neovex machine start` operates on the `default` machine (backwards
  compatible)
- `neovex machine start my-machine` operates on `my-machine`
- `neovex machine status my-machine` shows status for `my-machine`
- `neovex machine rm my-machine` removes `my-machine` only
- machine name appears in all error messages and status output

## Dependency Graph

- `DX4`, `DX8`, and `DX11` have no hard dependencies on the later DX items and
  can proceed in any order.
- `DX5` establishes shared `OutputFormat` infrastructure that `DX6`, `DX7`, and
  `DX9` reuse.
- `DX6` and `DX7` depend on `DX5` but are independent of each other.
- `DX8` has no dependencies.
- `DX9` depends on `DX5` (format infrastructure) and `DX6` (list subcommand).
- `DX10` has no dependencies.
- `DX11` has no hard dependencies but should land before `DX6` (list is not
  useful without the ability to target a machine by name).

## Remaining Recommended Delivery Order

All roadmap items are complete. Archive this plan as historical execution
record and use it only when future work needs the proof bundle paths, the
original comparative audit, or the exact DX closeout sequence.

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
  `cargo run -p neovex-bin -- --version` smoke check.
- 2026-04-18: Closed the lingering live-proof note for `DX1` on the current
  shipped macOS contract. Using the isolated Homebrew proof bundle rooted at
  `/tmp/neovex-v0.1.14-homebrew-proof/run`, the released `v0.1.14` darwin
  archive was checksum-verified, installed through an isolated temporary cask,
  and then used to initialize and start a clean machine on the pinned Podman
  digest with no `NEOVEX_MACHINE_GUEST_BINARY` override. That run proved host
  `neovex 0.1.14`, guest `neovex 0.1.14` from `/usr/local/bin/neovex
  --version`, packaged `libexec/gvproxy`, reachable forwarded machine API, and
  guest machine-API `/healthz` plus `/capabilities` success on the booted VM.
  Durable conclusion: the user-facing `neovex --version` DX task is fully
  validated on macOS through the shipped Homebrew/release-asset path, not
  through a locally built Linux guest binary workaround.
- 2026-04-18: Audited this plan against the current worktree and shipped
  `v0.1.14` baseline, then corrected stale control text before further CLI
  work. The audit removed the outdated claim that `neovex --version` is still
  missing, refreshed the macOS rationale to reflect the current pinned Podman
  digest contract plus release-asset guest binary flow, expanded `DX2` to
  cover the public `machine os` help surface, clarified that lifecycle
  hardening is now an archived reliability baseline rather than an active
  sibling plan, and converted the delivery section into a remaining-work order
  starting at `DX2`.
- 2026-04-18: Closed `DX2` by rewriting the public machine help surface to
  operator-facing language. `crates/neovex-bin/src/main.rs` now advertises the
  full machine/service orchestration scope at the top level, and
  `crates/neovex-bin/src/machine/mod.rs` now uses terse Podman-style wording
  for `machine`, `machine os`, and `machine init` help text. Added focused
  help regression coverage for `neovex --help`, `neovex machine --help`,
  `neovex machine os --help`, and `neovex machine init --help`. Verified with
  `cargo fmt --all --check`, `cargo check -p neovex-bin`, `cargo test -p
  neovex-bin cli_supports_top_level_version_flag -- --nocapture`, `cargo test
  -p neovex-bin cli_help_describes_machine_and_service_surface -- --nocapture`,
  `cargo test -p neovex-bin machine_help_uses_user_facing_descriptions --
  --nocapture`, `cargo test -p neovex-bin
  machine_os_help_uses_user_facing_descriptions -- --nocapture`, and `cargo
  test -p neovex-bin machine_init_help_uses_user_facing_flag_descriptions --
  --nocapture`. Next item: `DX3`.
- 2026-04-18: Closed `DX3` by aligning `neovex machine init` with Podman's
  flag surface: `--identity`, `--ignition-path`, `--firmware`, `--memory`,
  and `--disk-size`, plus short aliases `-c`, `-m`, `-d`, and `-v`. Internal
  config field names stayed stable while user-facing error text, the CLI
  reference, the macOS recreate helper, the Homebrew proof collector, and the
  helper Make target were all updated to the new spellings in the same change
  set. Verified with `cargo fmt --all --check`, `cargo check -p neovex-bin`,
  `cargo test -p neovex-bin parses_machine_init_with_resource_overrides --
  --nocapture`, `cargo test -p neovex-bin
  machine_init_accepts_short_flag_aliases -- --nocapture`, `cargo test -p
  neovex-bin machine_init_rejects_legacy_flag_names -- --nocapture`, `cargo
  test -p neovex-bin machine_init_help_uses_user_facing_flag_descriptions --
  --nocapture`, `cargo test -p neovex-bin
  podman_machine_os_bootstrap_contract_requires_ssh_identity -- --nocapture`,
  and `bash scripts/verify-neovex-machine-recreate-helper.sh`. Next item:
  `DX4`.
- 2026-04-18: Closed `DX4` by making `neovex machine start` the primary
  create-or-start path and adding the Podman-style `neovex machine init --now`
  shortcut. `crates/neovex-bin/src/machine/mod.rs` now lets `start` inherit
  init-style resource overrides for the create-if-missing path, auto-initializes
  the machine when no config exists yet, and rejects those create-only
  overrides once a machine already exists. `crates/neovex-bin/src/machine/manager.rs`
  now auto-generates a machine-owned SSH keypair under the Neovex machine data
  root for the macOS host-managed contract when no explicit `--identity` was
  recorded, so first-run guest convergence, SSH, and guest-binary sync all work
  on the default path. Updated `docs/reference/cli.md` and
  `docs/reference/macos-machine-flow.md` to reflect that `start` is now the
  shortest happy path and that the macOS contract auto-provisions a machine-owned
  identity when needed. Verified with `cargo fmt --all --check`, `cargo check -p
  neovex-bin`, `cargo test -p neovex-bin machine_init_parses_now_flag --
  --nocapture`, `cargo test -p neovex-bin
  machine_start_parses_create_if_missing_overrides -- --nocapture`, `cargo test
  -p neovex-bin machine_help_uses_user_facing_descriptions -- --nocapture`,
  `cargo test -p neovex-bin machine_init_help_uses_user_facing_flag_descriptions
  -- --nocapture`, `cargo test -p neovex-bin
  machine_start_help_describes_create_if_missing_overrides -- --nocapture`,
  `cargo test -p neovex-bin
  machine_start_reports_oci_materialization_failure_for_unreachable_registry_image
  -- --nocapture`, `cargo test -p neovex-bin
  machine_start_auto_initializes_before_start_failure -- --nocapture`, `cargo
  test -p neovex-bin machine_init_now_attempts_start_after_initialization --
  --nocapture`, `cargo test -p neovex-bin
  machine_start_rejects_create_if_missing_overrides_when_machine_exists --
  --nocapture`, and `cargo test -p neovex-bin
  ensure_machine_bootstrap_identity_generates_machine_owned_key_for_host_managed_contract
  -- --nocapture`. Real host proof was captured in the isolated bundle rooted
  at `/tmp/neovex-dx4-proof.9uILev`: `target/debug/neovex machine start` on a
  clean root succeeded with no explicit `--identity`, auto-created
  `/tmp/neovex-dx4-proof.9uILev/home/.local/share/neovex/machine/default/machine`
  plus `.pub`, converged the pinned Podman image digest, reported
  `machine_api.reachable: true` in `run/machine-status.txt`, returned
  `neovex 0.1.14` from guest `run/guest-neovex-version.txt`, returned `200`
  from guest `/v1/machine-api/healthz`, and exposed forwarded capabilities from
  `run/machine-api-capabilities.txt` before the machine was stopped cleanly.
  Next item: `DX11`.
- 2026-04-18: Closed `DX11` by threading optional `[NAME]` targeting through
  the current machine lifecycle surface (`init`, `start`, `stop`, `status`,
  `ssh`, `rm`) while preserving `default` as the no-argument target. The
  machine manager now locks and resolves config/state/data/runtime paths per
  requested machine instead of hardcoding `default`, status rendering now shows
  the selected machine name from the resolved path set, and `machine ssh`
  mirrors Podman's first-argument behavior by treating the first argument as a
  machine name only when that machine record exists. Updated
  `docs/reference/cli.md` to document the new `[NAME]` syntax and the Podman-style
  `ssh` resolution rule. Verified with `cargo fmt --all --check`, `cargo check
  -p neovex-bin`, and `cargo test -p neovex-bin machine_ -- --nocapture
  --test-threads=1`, which covered the focused parser and behavior additions
  (`machine_lifecycle_subcommands_accept_optional_name_positionals`,
  `machine_ssh_prefers_existing_machine_name_before_guest_command`,
  `machine_ssh_treats_unknown_first_arg_as_guest_command`,
  `machine_init_writes_named_machine_records`,
  `machine_remove_only_deletes_requested_machine`, and
  `machine_start_auto_initializes_named_machine_before_start_failure`) alongside
  the existing machine regression slice. Next item: `DX5`.
- 2026-04-18: Closed `DX5` by adding a real `MachineStatusOutputFormat`
  surface to `neovex machine status`: `--format table` is now the default
  human-oriented summary, while `--format json` and `--format yaml` emit the
  full structured `MachineStatusView`. The implementation now shares a single
  status-view builder across all render paths so table, JSON, and YAML stay in
  sync instead of diverging into separate data models. Updated
  `docs/reference/cli.md` to document the default table output and explicit
  structured format flags. Verified with `cargo fmt --all --check`, `cargo
  check -p neovex-bin`, and `cargo test -p neovex-bin machine_ -- --nocapture
  --test-threads=1`, including the focused parser/help/output coverage
  (`machine_status_defaults_to_table_output_format`,
  `machine_status_accepts_json_and_yaml_output_formats`,
  `machine_status_help_describes_output_formats`,
  `machine_status_table_output_is_default_human_summary`,
  `machine_status_json_output_serializes_full_status_view`, and
  `machine_status_yaml_output_serializes_full_status_view`). Real macOS proof
  reused the isolated bundle rooted at `/tmp/neovex-dx4-proof.9uILev`: after
  rebuilding `target/debug/neovex`, the same isolated machine root produced
  `run/machine-status-table.txt`, `run/machine-status.json`, and
  `run/machine-status.yaml`, proving the new table default plus JSON/YAML modes
  against a real pinned-Podman macOS machine record without touching the
  user's installed state. Next item: `DX8`.
- 2026-04-18: Closed `DX8` by adding `neovex machine set` as the stopped-only
  resource reconfiguration surface for the current machine contract.
  `crates/neovex-bin/src/machine/mod.rs` now accepts optional `--cpus`,
  `--memory`, and `--disk-size` overrides, rejects no-op invocations with no
  requested changes, requires the named machine to be stopped before applying
  updates, persists the edited resource contract back to `config.json`, and
  prints the updated machine view through the existing rendering path. Updated
  `docs/reference/cli.md` to document the new command and its stopped-machine
  scope. Verified with `cargo fmt --all --check`, `cargo check -p neovex-bin`,
  `cargo test -p neovex-bin machine_set_ -- --nocapture --test-threads=1`, and
  `cargo test -p neovex-bin machine_ -- --nocapture --test-threads=1`. Focused
  coverage proved parser/help behavior, persisted config updates on a stopped
  machine, the required-change error path, and the stopped-only guardrail.
  Next item: `DX6`.
- 2026-04-18: Closed `DX6` by adding `neovex machine list` with a visible
  `ls` alias, `--format json|table`, and `--quiet` names-only output.
  `crates/neovex-bin/src/machine/mod.rs` now enumerates initialized machine
  records directly from the config root, refreshes each machine's persisted
  state under its own per-machine lock, and renders a compact summary view for
  human and scripted use instead of reusing the heavier single-machine status
  payload. Updated `docs/reference/cli.md` to document `list`, `ls`, and the
  new output modes, and tightened this plan's remaining `DX9` scope so it now
  owns only `status --quiet` plus precedence alignment with the already-landed
  list quiet mode. Verified with `cargo fmt --all --check`, `cargo check -p
  neovex-bin`, `cargo test -p neovex-bin machine_list_ -- --nocapture
  --test-threads=1`, and real local macOS CLI proof using the isolated bundle
  rooted at `/tmp/neovex-dx4-proof.9uILev`, which produced
  `run/machine-list-table.txt`, `run/machine-list.json`, and
  `run/machine-list-quiet.txt` from the existing pinned-Podman machine record
  without touching the user's installed machine state. Next item: `DX7`.
- 2026-04-18: Closed `DX7` by adding `neovex machine inspect` as the direct
  config-and-state inspection surface for the current machine manager.
  `crates/neovex-bin/src/machine/mod.rs` now accepts optional named-machine
  targeting, defaults inspect output to JSON, supports `--format yaml`, and
  renders the persisted `MachineConfigRecord` plus refreshed
  `MachineStateRecord` without mixing in the broader derived status payload.
  Updated `docs/reference/cli.md` to document the new command and its output
  contract. Verified with `cargo fmt --all --check`, `cargo check -p
  neovex-bin`, `cargo test -p neovex-bin machine_inspect_ -- --nocapture
  --test-threads=1`, and real local macOS CLI proof using the isolated bundle
  rooted at `/tmp/neovex-dx4-proof.9uILev`, which produced
  `run/machine-inspect.json` and `run/machine-inspect.yaml` from the existing
  pinned-Podman machine record, including the pinned digest, machine-owned SSH
  identity path, XDG roots, and refreshed helper/runtime state, without
  touching the user's installed machine state. Next item: `DX9`.
- 2026-04-18: Closed `DX9` by adding `--quiet` / `-q` to `neovex machine
  status` and aligning its precedence rules with the already-landed
  `machine list --quiet` surface. `crates/neovex-bin/src/machine/mod.rs` now
  lets quiet mode short-circuit the normal table/JSON/YAML rendering and emit
  only the selected machine name, making shell scripting predictable without
  parsing human-oriented output. Updated `docs/reference/cli.md` to document
  the quiet status mode. Verified with `cargo fmt --all --check`, `cargo check
  -p neovex-bin`, `cargo test -p neovex-bin machine_status_ -- --nocapture
  --test-threads=1`, and real local macOS CLI proof using the isolated bundle
  rooted at `/tmp/neovex-dx4-proof.9uILev`, which produced
  `run/machine-status-quiet.txt` from
  `target/debug/neovex machine status --format json --quiet` and confirmed
  that quiet output wins over the structured format flag while leaving the
  user's installed machine state untouched. Next item: `DX10`.
- 2026-04-18: Closed `DX10` by adding `neovex machine cp` as the Podman-style
  host↔guest transfer surface. `crates/neovex-bin/src/machine/mod.rs` now
  parses `<machine>:/path` guest endpoints, rejects host↔host and
  machine↔machine copies, and shells out through `scp` only after the named
  machine's running/identity contract has been validated. The shared transport
  layer in `crates/neovex-bin/src/machine/manager.rs` now reuses the same
  localhost SSH safety options as `machine ssh`, switching only the port flag
  and remote-path formatting that `scp` needs. Updated `docs/reference/cli.md`
  to document the new command and finalized the historical DX11 note so `cp`
  is explicitly called out as the Podman-style `NAME:/path` exception to the
  broader `[NAME]` positional targeting pattern. Verified with
  `cargo fmt --all --check`, `cargo check -p neovex-bin`, `cargo test -p
  neovex-bin machine_cp_ -- --nocapture --test-threads=1`, `cargo test -p
  neovex-bin scp_command_ -- --nocapture --test-threads=1`, and real local
  macOS proof using the isolated bundle rooted at `/tmp/neovex-dx4-proof.9uILev`:
  `target/debug/neovex machine start` outside the sandbox to use real host
  port binding and virtualization, a host→guest copy into
  `default:/tmp/neovex-dx10-copy.txt`, a guest→host round-trip back into
  `run/machine-cp-host-roundtrip.txt`, and a clean `machine stop`. The proof
  artifacts `run/machine-cp-upload.txt`, `run/machine-cp-download.txt`, and
  `run/machine-cp-host-roundtrip.txt` showed `Copy successful` in both
  directions and preserved the exact `dx10-copy-proof` file contents.
