# Plan: Machine CLI Alignment and UX Control Plane

Canonical execution plan for the next machine-command UX wave in Nimbus. This
plan promotes a consistent, battle-tested CLI style system for commands that
map naturally to Podman and Docker/Moby conventions, while implementing that
system with idiomatic Rust CLI building blocks instead of one-off formatting.

The intent is simple: borrow the best command UX ideas shamelessly, keep
Podman as the primary implementation reference where we have local source, use
Docker/Moby as a product-surface comparator where the semantics fit, and make
Nimbus consistent rather than bespoke.

Reviewed against:

- `crates/nimbus-bin/src/main.rs`
- `crates/nimbus-bin/src/machine/mod.rs`
- `crates/nimbus-bin/src/machine/manager.rs`
- `crates/nimbus-bin/src/service/mod.rs`
- `docs/reference/cli.md`
- `docs/plans/archive/machine-cli-dx-plan.md`
- `/Users/jack/src/github.com/containers/podman/cmd/podman/root.go`
- `/Users/jack/src/github.com/containers/podman/cmd/podman/machine/start.go`
- `/Users/jack/src/github.com/containers/podman/cmd/podman/machine/init.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/stdpull/url.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/compression/decompress.go`
- `/Users/jack/src/github.com/containers/podman/cmd/podman/volumes/list.go`
- `/Users/jack/src/github.com/moby/moby/integration-cli/cli`

Notes on local references:

- The local Podman checkout is the primary source-backed implementation
  reference for machine-command structure, help/usage layout, success-summary
  style, progress semantics, `quiet` precedence, and table-oriented human
  output.
- The local Moby checkout does not include the modern standalone `docker/cli`
  repository. Treat Docker/Moby as a product-surface comparator for terse
  operator UX and output tone, but do not force code-shape alignment to local
  Moby paths that do not actually own the current Docker CLI.

---

## Status

- **Status:** `completed`
- **Primary owner:** this plan
- **Active roadmap item:** none - archived completed execution record
- **Activation gate:** post-closeout review of the archived machine CLI plan
  and real local proof on 2026-04-18 surfaced a new class of consistency gaps:
  better `machine start` progress landed, but the broader command family still
  lacks a unified output/style system
- **Related references:**
  - `docs/reference/microvm-service-baseline.md`
  - `docs/reference/macos-machine-flow.md`
  - `docs/reference/cli.md`
  - `docs/plans/archive/machine-cli-dx-plan.md`

---

## Current Assessed State

- The first machine CLI DX wave is complete and archived. Core ergonomic gains
  such as `machine start` create-if-missing, `machine status --format`, `list`,
  `inspect`, `set`, `cp`, and named-machine targeting are landed.
- The recent mutation-output cleanup improved one important seam: `machine
  start` now behaves more like a real action command, and the status dump is no
  longer printed automatically after mutation commands.
- The next problem is consistency, not feature absence.
- Human-facing action output, progress reporting, help layout, tables, warning
  style, and structured-output boundaries are still implemented ad hoc instead
  of through a single CLI UX system.
- The root command contract is still under-specified compared to Podman:
  - explicit help/usage template ownership
  - explicit error-printing and exit-code behavior
  - explicit version/help short-circuit behavior
- Some command flows now feel Podman-like, but the implementation still mixes:
  - hand-rolled table formatting
  - hand-rolled phase messages
  - default Clap help layout
  - inconsistent operator banners in proof commands and examples
- Local proof also surfaced a real operator-confusion case:
  - local debug binary vs installed Homebrew binary version drift
  - isolated XDG roots vs the default machine roots
  - helper fallback paths that are technically correct but not clearly
    explained to operators

This plan turns those findings into a durable CLI control plane rather than a
series of one-off polish patches.

---

## Target UX Principles

1. Action commands should act like action commands.
   They print concise success summaries on stdout and stream progress on stderr.

2. Summary and inspect surfaces must follow their nearest strong analogue.
   Summary surfaces such as `list` and `status` should default to human-readable
   tables or summaries. Strong `inspect` analogues may remain structured by
   default when Podman or Docker already set that expectation, but CLIA1 must
   write that choice down explicitly instead of letting it drift.

3. Human output and structured output must be different products.
   JSON/YAML are for scripts and stable tooling; tables, color, hints, and
   progress belong only to the human surface.

4. `--quiet` must be predictable.
   Quiet mode suppresses human chatter and progress, and it wins over other
   human-oriented formatting switches.

5. Flag precedence must be documented, not inferred.
   If `--quiet`, `--format`, aliases, header toggles, or future style flags
   interact, the exact precedence must be captured in the command matrix before
   code lands.

6. TTY-sensitive behavior must be explicit.
   Progress bars, spinners, colors, and rich hints appear only on interactive
   terminals; non-interactive output stays stable and boring.

7. Podman is the primary command UX reference where semantics match.
   Docker/Moby is a secondary comparator for terse operator voice and familiar
   defaults, not a reason to invent a second style family.

8. Root-command behavior is part of the UX contract.
   Help rendering, version output, error formatting, and exit-code behavior are
   not “outside the plan”; they must align with the same operator surface.

9. One output primitive per concern.
   No more per-command custom printers if the concern is already solved by the
   shared UX layer.

---

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Roadmap Status Ledger` and `Execution Log`
3. `docs/reference/cli.md`
4. the owning command implementations in `crates/nimbus-bin/src/*`

General rules:

- This plan owns command-surface ergonomics, shared CLI output primitives, and
  the consistency contract for machine-adjacent commands.
- It does not own microVM architecture, machine guest content, or service
  control-plane architecture.
- Podman local source is the primary code-backed reference. When Nimbus already
  has a command whose meaning matches Podman, copy Podman's behavior unless
  Nimbus has a documented reason not to.
- Docker/Moby is a secondary UX reference. Use it for terse action phrasing,
  pull/progress expectations, and help/output familiarity when Podman has no
  stronger command analogue.
- Breaking changes are preferred over aliases, compatibility shims, or dual
  spelling. This repo is still pre-launch.
- Shared CLI UX should live behind reusable helpers or a dedicated internal
  module, not duplicated formatting code in each command handler.
- Root-command help, version, usage, and error rendering belong to the same
  shared UX contract as subcommands; do not leave the top-level binary on an
  implicit default style while polishing only subcommands.
- Human progress belongs on stderr. Final success payloads belong on stdout.
- JSON/YAML output must never contain ANSI styling, progress chatter, or human
  hints.
- The canonical CLI reference must stay correct as the UX contract changes.
  If `docs/reference/cli.md` disagrees with the parser or shipped binary
  behavior, fix the doc in the same change set as the UX change.
- Add a new UX crate only when it replaces repeated hand-rolled code across
  multiple commands, not for a single isolated flourish.

---

## Rust CLI Building Blocks

This plan standardizes the Rust building blocks we should use for command UX.

### Keep

- `clap`
  - command tree, argument parsing, help text, shell-facing error handling

### Adopt

- `indicatif`
  - progress bars, spinners, byte-progress reporting, elapsed/ETA display, TTY
    gating for long-running operations such as image downloads and guest-binary
    fetches
- `comfy-table` as the current frontrunner for table rendering
  - human-readable tables for list/status-style output so we stop maintaining
    custom spacing/alignment logic by hand
  - CLIA4 must confirm it cleanly covers the required header, width,
    truncation, and empty-state behavior before we lock it in
- `anstream` plus `anstyle`
  - cross-platform styled human output, warnings, tips, and phase banners
    without leaking ANSI formatting into non-interactive surfaces

### Intentionally defer

- `miette`, `color-eyre`, or other compiler-style rich diagnostic stacks
  - not the right fit for Podman/Docker-style operator CLI output; Nimbus
    should prefer concise actionable errors, not Rust-developer backtraces in
    normal command output
- interactive prompt or TUI frameworks
  - out of scope for this plan

---

## Shared UX Contract

### Help and usage

- Use a Podman-style help layout:
  - short summary first
  - optional description
  - compact usage section
  - examples block
  - options grouped and named consistently
- Commands with close Podman analogues should use the same noun/verb phrasing
  and help tone unless Nimbus needs a clearly different contract.
- Root help/version behavior must be included:
  - explicit subcommand requirements
  - terse top-level version output
  - no accidental fallback behavior that docs or examples do not describe

### Human success output

- Mutation commands print one concise success line by default.
- If a command materially changed state and there is a common next step, print
  at most one short follow-up hint.
- Do not print full YAML/JSON-like state dumps on successful mutations.

### Progress output

- Use phase banners when bytes are unknown.
- Use byte-aware progress bars when bytes are known.
- Keep wording consistent:
  - `Pulling ...`
  - `Downloading ...`
  - `Extracting ...`
  - `Starting ...`
  - `Waiting for ...`
  - `Verifying ...`
- Suppress progress in non-TTY and `--quiet` mode.

### Tables and structured output

- Human summary commands default to tables.
- Machine-readable output requires explicit `--format`.
- Tables should use one shared style system for headers, column alignment, null
  placeholders, truncation, and empty-state messaging.
- Per-command precedence between `--quiet`, `--format`, and any header toggles
  must be written down and tested; do not rely on a global assumption.

### Errors and remediation

- Errors stay terse but actionable.
- When there is a clear operator next step, include one remediation hint.
- Avoid panic-style internal phrasing in user-visible errors unless the issue
  is genuinely internal and unexpected.
- Root-command exit behavior must be deliberate:
  - help/version should short-circuit cleanly
  - operator errors should not dump usage unless the contract explicitly wants
    it
  - exit codes should stay stable across human and structured surfaces

---

## Scope

This plan covers:

- shared CLI UX infrastructure inside `nimbus-bin`
- root-command help/version/error behavior
- help/usage/examples template consistency
- progress styling and byte-progress adoption
- table rendering consistency
- human vs structured output contract
- machine-command family alignment
- service-command family alignment where semantics map well to Podman/Docker
- top-level binary/version/help polish needed to support the shared style

This plan does not cover:

- microVM architecture changes
- machine image or guest OS changes
- service control-plane architecture changes
- GUI/Desktop UI work
- interactive TUI workflows

---

## Command Families in Scope

### Wave 1: Machine commands

Primary source reference: Podman machine commands.

- `machine init`
- `machine start`
- `machine stop`
- `machine status`
- `machine list` / `machine ls`
- `machine inspect`
- `machine set`
- `machine cp`
- `machine ssh`
- `machine rm`
- `machine os apply`
- `machine os upgrade`

### Wave 2: Service and local runtime commands

Primary source reference: Podman + Docker action/list/inspect/log/ps flows,
applied only where semantics map cleanly.

- `service up`
- `service down`
- `service list`
- `service inspect`
- `service logs`
- `service ps`
- `serve`

---

## Comparative Reference Matrix

This section is the CLIA1 freeze point. If a later code change wants to diverge
from one of these contracts, it must update this matrix in the same change set
and explain why Nimbus needs the difference.

### Root command contract

| Surface | Primary reference | Target contract | Intentional Nimbus note |
| --- | --- | --- | --- |
| `nimbus --help` | Podman root help in `cmd/podman/root.go` | custom root help/usage template, concise command taxonomy, examples where needed, and no default Clap layout leakage | Nimbus keeps its own product description and command tree |
| `nimbus --version` | Podman and Docker terse version output | single-line version output with no extra prose | no change to the explicit top-level flag |
| `nimbus` with no subcommand | intentional Nimbus contract, documented against the current parser | remain an explicit-subcommand CLI; the parser, help text, and docs must all agree that `serve` is explicit | do not reintroduce implicit server start |
| root errors and exit behavior | Podman root execution/error flow | terse stderr errors, no automatic usage dump unless the contract explicitly wants it, stable exit codes, and help/version short-circuit behavior | implementation stays in Clap/Rust, but the operator surface should feel Podman-like |

### Machine command matrix

| Nimbus surface | Primary reference | Secondary comparator | Target default mode | Target contract and notes |
| --- | --- | --- | --- | --- |
| `machine init` | `podman machine init` | none | action summary | concise stdout success summary; if `--now` starts the VM, hand off to the same progress and readiness path as `machine start`; any resource/image flags keep Podman-like naming where already shipped |
| `machine start` | `podman machine start` | none | action summary plus progress | stdout prints the final success summary; stderr carries phase/progress output; retain Nimbus create-if-missing behavior as an intentional divergence; landed Podman-style `--quiet` and `--no-info`, with `--quiet` suppressing phase/progress chatter while keeping the final success summary and `--no-info` suppressing advisory `info:` notices only |
| `machine stop` | `podman machine stop` | none | action summary | one concise success line, with operator guidance only when something unusual happened |
| `machine status` | Podman behavior split across `machine list`, `machine inspect`, and `machine info` | Docker-style human status tables | human table | keep the consolidated Nimbus `status` surface intentionally; default is a human summary table, `--format json|yaml` is explicit structured output, and `--quiet` stays names-only if the command keeps that flag |
| `machine list` / `machine ls` | `podman machine list` | none | human table | mirror Podman table feel and aliasing; `--format` semantics should match Podman precedence, with explicit user format winning over `--quiet` |
| `machine inspect` | `podman machine inspect` | `docker inspect` | structured inspect | JSON remains the default because the strong analogue is structured-by-default; YAML is an explicit Nimbus extension; human summaries belong in `status` and `list`, not here |
| `machine set` | `podman machine set` | none | action summary | concise mutation result, no structured status dump on success |
| `machine cp` | `podman machine cp` | `docker cp` | action with passthrough copy output | keep direct copy semantics and Podman-like `--quiet`; do not wrap transfer output in extra styling |
| `machine ssh` | `podman machine ssh` | none | interactive passthrough | preserve SSH/stdin/stdout passthrough behavior; avoid decorative output around the session |
| `machine rm` | `podman machine rm` | `docker rm` for terse removal tone | action summary | short success line, with any safety or state-precondition errors rendered tersely |
| `machine os apply` | `podman machine os apply` | none | action summary | explicit host-managed image apply surface; restart guidance only when relevant |
| `machine os upgrade` | `podman machine os upgrade` | none | action summary | explicit host-managed upgrade surface; any dry-run or structured result must stay explicit rather than polluting the default mutation path |

### Service and local runtime matrix

| Nimbus surface | Primary reference | Secondary comparator | Target default mode | Target contract and notes |
| --- | --- | --- | --- | --- |
| `service config` | `docker compose config` | none | structured validation output | keep structured resolved output by default because the closest analogue is structured; `--services` remains a terse names-only projection |
| `service up` | `docker compose up` | Podman action-command tone | action summary plus progress | move away from YAML lifecycle dumps toward concise action summaries; any long-running setup or warnings belong on stderr |
| `service down` | `docker compose down` | Podman stop/rm tone | action summary | default success path should be terse and mutation-oriented, not a serialized outcome dump |
| `service list` | `docker compose ps` for project-scoped human summaries | `podman ps` table feel | human table | target a default table surface in CLIA8; keep `list` as the Nimbus command name because `ps` is reserved for host-process detail |
| `service inspect` | `docker inspect` | `podman inspect` | structured inspect | the strong analogue is structured-by-default; if a human summary is useful, add a separate summary surface instead of overloading inspect |
| `service logs` | `docker compose logs` | `podman logs` | passthrough logs | preserve direct log streaming semantics; `--follow` remains a transport/control flag, not a style flag |
| `service ps` | `docker top` | `podman top` | human process summary | keep `ps` focused on process snapshots for one service; target a human-readable process table or summary rather than raw YAML once CLIA8 lands |
| `serve` | no exact Podman/Docker analogue | `podman system service` for help/error tone and `docker compose up` for attached output expectations | long-running server command | keep `serve` explicitly Nimbus-specific; borrow terse action/help/error conventions where they fit, but do not force a fake container-runtime analogy onto the server lifecycle |

### Frozen intentional divergences

- `nimbus` keeps an explicit `serve` subcommand instead of implicit server
  startup.
- `machine start` keeps create-if-missing behavior because that developer path
  is already part of the shipped Nimbus contract.
- `machine status` remains a consolidated Nimbus summary surface instead of
  splitting status across multiple Podman-style commands.
- `service list` stays distinct from `service ps`: `list` is the project/tenant
  lifecycle summary surface, while `ps` is reserved for per-service process
  detail.
- `serve` remains a product-specific server command, not a thin clone of any
  existing Podman or Docker subcommand.

---

## Verification Contract

### Minimum verification for every code item

- `cargo fmt --all --check`
- focused `cargo check -p nimbus-bin`
- focused `cargo test -p nimbus-bin` for touched modules
- update this plan's ledger and execution log in the same change set

### Required UX verification lanes

- **Help lane**
  - top-level and affected subcommand help output use the shared layout
  - examples are present where operators need them
  - help tone stays concise and operator-facing
  - root `--help` and `--version` remain correct and intentional

- **Action lane**
  - mutation commands print concise summaries instead of structured dumps
  - stdout/stderr split is correct
  - `--quiet` suppresses non-essential human chatter
  - flag-precedence behavior is verified where `--quiet` interacts with
    `--format` or similar switches

- **Progress lane**
  - interactive terminal run shows progress bars or phase banners
  - non-interactive run does not emit rich progress output
  - byte-aware downloads use progress bars where size is known

- **Table lane**
  - default human tables are consistent across commands
  - empty-state rows and null placeholders are consistent
  - JSON/YAML output remains stable and unstyled

- **Error lane**
  - expected operator failures include actionable remediation where possible
  - no stale implementation jargon leaks into user-facing errors

- **Real-host proof lane**
  - isolated macOS machine proof for TTY and non-TTY paths
  - local debug binary and installed/Homebrew binary paths are distinguished
    explicitly in proof commands and captured output
  - where relevant, compare the Nimbus command flow directly against the
    analogous Podman command on the same host

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| CLIA1 | completed | Froze the comparative reference matrix for root, machine, service, and `serve` surfaces; documented flag-precedence ownership and the intentional Nimbus divergences that later UX work must preserve or explicitly change | none |
| CLIA2 | completed | Introduced a shared internal `cli_ux` layer for stdout/stderr helpers, TTY-gated phase output, and reusable table rendering; migrated machine action output, machine summary tables, machine-manager notices, and service warning/stdout paths onto it | CLIA1 |
| CLIA3 | completed | Adopted `indicatif`-backed byte progress in the shared `cli_ux` seam for guest release-asset downloads, machine image HTTP/OCI pulls, and raw/compressed materialization and extraction paths, including replacing the guest archive shell-out with Rust extraction so first-start progress stays byte-aware end-to-end | CLIA2 |
| CLIA4 | completed | Adopted `comfy-table` in the shared table helper, migrated machine status/list onto the crate-backed borderless contract, and added table helper coverage for alignment, minimum widths, and empty-row header rendering | CLIA2 |
| CLIA5 | completed | Replaced default Clap help rendering with shared Podman-aligned help/usage/examples templates, applied them across the root, machine, machine-os, service, and serve surfaces, and added focused help coverage for machine and service leaf commands plus live `cargo run` proof for representative leaf help output | CLIA1 |
| CLIA6 | completed | Standardized machine-family mutation success summaries, added shared action-block and `Hint:` helpers in `cli_ux`, replaced the default YAML mutation dump on `machine os apply` / `machine os upgrade` with concise action summaries, and tightened common machine remediation errors so real operator failures read like guidance instead of internal assertions | CLIA2, CLIA5 |
| CLIA7 | completed | Completed the machine-family convergence sweep: explicit `machine list --format` now wins over `--quiet`, `machine start` now carries Podman-style `--quiet/--no-info` output controls, and the machine command family now follows the shared action/table/structured-output contract with real macOS PTY proof | CLIA3, CLIA4, CLIA6 |
| CLIA8 | completed | Completed the service and `serve` convergence sweep: `service up` / `down` now use concise action summaries, `service list` defaults to a human table with explicit structured formats, `service inspect` now behaves like a structured inspect surface with JSON default and YAML explicit, `service ps` now defaults to a human process summary with explicit structured formats, and the service family now follows the same action/table/structured-output rules as the machine family | CLIA2, CLIA4, CLIA6 |
| CLIA9 | completed | Added the checked-in isolated-root/local-binary machine proof lane (`collect-nimbus-machine-cli-proof`), its deterministic verifier, Makefile wiring, and macOS flow documentation so local proof commands clearly state which binary and roots are under test and no longer rely on ad hoc shell snippets | CLIA7 |
| CLIA10 | completed | Refreshed real-host proof across the isolated local-binary and packaged/Homebrew lanes, fixed the service-proof readiness matcher to accept the shipped JSON `service inspect` contract, and closed the plan with release-grade macOS proof bundles for machine and service surfaces before archiving the control plane | CLIA7, CLIA8, CLIA9 |

---

## Implementation Checkpoints

### CLIA1 — Comparative reference matrix

Outputs:

- explicit per-command mapping table:
  - Nimbus command
  - Podman analogue
  - Docker/Moby analogue, if useful
  - target default output mode
  - target success summary style
  - target progress/error/help expectations
  - target flag-precedence rules (`quiet`, `format`, headers, aliases)
- explicit root-command contract:
  - top-level help behavior
  - top-level version behavior
  - parser-required explicit subcommands
  - error/usage/exit-code conventions

Acceptance criteria:

- no in-scope command is left with an undefined UX reference family
- any intentionally Nimbus-specific divergence is written down before code
- the root command and documentation agree about whether `serve` is explicit or
  implicit

### CLIA2 — Shared internal CLI UX layer

Outputs:

- reusable internal module(s) for:
  - TTY detection
  - human success summaries
  - warnings/tips
  - shared table rendering entrypoint
  - stdout/stderr split helpers
  - root/help/error rendering helpers where Clap defaults are insufficient

Acceptance criteria:

- new command UX logic does not duplicate ad hoc formatting helpers
- existing machine-command output helpers begin converging on the shared layer

### CLIA3 — Progress infrastructure

Outputs:

- `indicatif`-backed progress helpers
- phase banners only where byte counts are unknown
- machine image and guest-binary flows migrated first

Acceptance criteria:

- first-start machine flows show byte-aware progress where possible
- non-TTY runs stay quiet and stable

### CLIA4 — Table infrastructure

Outputs:

- chosen table crate wired into shared table helpers
- machine status/list migrated
- service list/ps prepared for later migration

Acceptance criteria:

- the chosen crate is validated against the required header, width,
  truncation, and empty-state contract before the decision is treated as final
- no hand-maintained spacing logic remains in migrated commands
- headers, alignment, and empty-state handling are consistent

### CLIA5 — Help template alignment

Outputs:

- shared help/usage/examples template
- affected command help text rewritten to fit the new template
- top-level command help/version behavior aligned to the same template contract

Acceptance criteria:

- help output is recognizably closer to Podman's layout than default Clap
- examples render cleanly and consistently
- top-level `nimbus --help` and `nimbus --version` remain intentional and
  documented

### CLIA6 — Success and error contract

Outputs:

- mutation success summaries standardized
- common remediation-hint helpers
- stale implementation-jargon audit for user-facing errors

Acceptance criteria:

- action commands no longer oscillate between terse success lines and verbose
  diagnostic dumps
- user-facing failures read like operator guidance, not internal assertions

### CLIA7 — Machine family convergence

Outputs:

- machine commands aligned to the shared contract
- `machine status` / `list` / `inspect` stable as inspection surfaces
- mutation commands aligned as action surfaces

Acceptance criteria:

- machine command family feels internally consistent
- live PTY proof on macOS matches the documented contract

### CLIA8 — Service and serve convergence

Outputs:

- service/serve commands migrated where the analogues are strong enough
- list/log/inspect flows match the same stdout/stderr/format rules

Acceptance criteria:

- users moving between `machine` and `service` commands do not hit a second
  style system

### CLIA9 — Docs and proof-helper cleanup

Outputs:

- updated docs and helper commands
- isolated-root examples that clearly state which binary and roots are under
  test
- CLI reference updated where shipped parser behavior changed

Acceptance criteria:

- proof commands are copy-pasteable without confusing local-vs-installed
  binary behavior
- `docs/reference/cli.md` does not contradict the shipped root-command contract

### CLIA10 — Closeout

Outputs:

- release-grade proof bundle
- plan archive move
- AGENTS/docs updates if the control-plane owner changes

Acceptance criteria:

- the shared CLI style system is documented, implemented, and verified on real
  host flows
- released/Homebrew verification covers the changed surfaces, not only local
  cargo-built binaries

---

## Execution Log

- 2026-04-18: Authored and activated this plan after the archived machine CLI
  DX closeout. Trigger was a real local proof showing that the next class of
  UX work is consistency: progress output, success summaries, table styling,
  help layout, and proof-helper clarity need a shared control plane instead of
  incremental one-off tweaks.
- 2026-04-18: Audit pass tightened the plan before implementation: added
  explicit ownership for root-command help/version/error behavior, required
  per-command flag-precedence rules instead of a blanket `quiet` rule, and
  added release/Homebrew proof plus CLI-reference correctness to the
  verification contract.
- 2026-04-18: Completed CLIA1. Froze the comparative reference matrix for root,
  machine, service, and `serve` surfaces; documented the intentional Nimbus
  divergences that later slices must preserve or explicitly change; and updated
  `AGENTS.md` so future agents treat this plan as the active CLI control plane
  and resume the earliest unfinished `CLIA` item.
- 2026-04-18: Completed CLIA2. Added `crates/nimbus-bin/src/cli_ux.rs` as the
  shared CLI UX seam for stdout/stderr writes, TTY-gated phase output, action
  summary formatting, and reusable table rendering. Migrated machine action
  summaries, machine status/list tables, machine-manager progress/info/warning
  output, and service warning/stdout paths onto the shared helpers. Verified
  with `cargo fmt --all --check`, `cargo check -p nimbus-bin`, and
  `cargo test -p nimbus-bin`.
- 2026-04-18: Started CLIA3. The active in-progress slice is byte-aware
  progress plumbing through the shared `cli_ux` seam for guest release-asset
  downloads, machine image downloads, OCI blob pulls, and raw/compressed
  materialization paths, while keeping plain phase banners only where the byte
  count truly is not available.
- 2026-04-18: Completed CLIA3. Added `indicatif`-backed byte progress helpers
  to `crates/nimbus-bin/src/cli_ux.rs` and wired them into guest release-asset
  download and extraction, machine image HTTP download, OCI blob pulls, and
  raw/gzip/zstd materialization. Replaced the guest archive shell-out with Rust
  gzip+tar extraction so the guest-binary path can report byte-aware progress
  end-to-end. Verified with `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`, and `cargo test -p nimbus-bin`.
- 2026-04-18: Completed CLIA4. Adopted `comfy-table` as the shared table crate
  inside `crates/nimbus-bin/src/cli_ux.rs`, preserving the borderless,
  Podman-like summary layout while removing the last hand-maintained spacing
  logic from migrated machine status/list output. Added helper coverage for
  minimum-width alignment and empty-row header rendering, then verified with
  `cargo fmt --all --check`, `cargo check -p nimbus-bin`, and
  `cargo test -p nimbus-bin`.
- 2026-04-18: Started CLIA5. Landed the first shared help-template slice in
  `cli_ux` plus root/group examples for `nimbus`, `machine`, `service`, and
  `serve`, and validated the rendered root help with
  `cargo run -p nimbus-bin -- --help`. The next CLIA5 slice is extending that
  template contract and example coverage across the remaining leaf commands so
  their `--help` output is consistently Podman-aligned instead of default Clap.
- 2026-04-18: Completed CLIA5. Extended the shared help template and example
  contract across the remaining machine, machine-os, and service leaf
  commands, added focused service leaf help coverage to match the machine
  sweep, and re-verified with `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`, `cargo test -p nimbus-bin`, plus live help
  spot-checks via `cargo run -p nimbus-bin -- machine init --help` and
  `cargo run -p nimbus-bin -- service up --help`.
- 2026-04-18: Started CLIA6. Auditing the machine command family now for
  inconsistent success summaries, stale failure wording, and missing
  remediation hints so the next slice can standardize the default human
  contract on top of the newly aligned help surface.
- 2026-04-18: Completed CLIA6. Added shared `cli_ux` helpers for multi-line
  action summaries and `Hint:` guidance, migrated `machine os apply` /
  `machine os upgrade` away from default YAML status dumps and onto concise
  machine-action summaries, and tightened common machine conflict errors so
  operators get an immediate next step instead of raw implementation wording.
  Verified with `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin`, and isolated-root local-binary proof showing the
  new `machine os upgrade --dry-run` action summary plus guided
  `machine start --memory 4096` error output.
- 2026-04-18: Started CLIA7. Landed the first machine-family convergence slice
  by distinguishing default vs explicit `machine list --format` selection so
  an explicit `--format json` now wins over `--quiet` as planned, while plain
  `--quiet` still stays names-only. Verified with `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`, `cargo test -p nimbus-bin`, and an isolated
  rebuilt-binary proof of `machine list --quiet` versus
  `machine list --format json --quiet`.
- 2026-04-19: Continued CLIA7. Added shared `cli_ux` output-mode guards and
  wired `machine start --quiet` / `machine start --no-info` to the Podman-like
  start contract: `--quiet` suppresses phase/progress chatter while preserving
  the final success summary, and `--no-info` suppresses only advisory
  `info:` notices. Updated the CLI reference in the same change set. Verified
  with `cargo fmt --all --check`, `cargo check -p nimbus-bin`, and
  `cargo test -p nimbus-bin`, plus isolated real-host macOS proof using the
  rebuilt local binary under temp XDG roots with a local raw guest image and
  explicit Linux guest-binary override:
  `script -q /tmp/nimbus-clia7-proof.RRzIHu/logs/verbose.log /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine start --image $HOME/.local/share/nimbus/machine/default/images/default.raw`,
  `script -q /tmp/nimbus-clia7-proof.RRzIHu/logs/quiet.log /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine start --image $HOME/.local/share/nimbus/machine/default/images/default.raw --quiet`,
  and
  `script -q /tmp/nimbus-clia7-proof.RRzIHu/logs/noinfo.log /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine start --image $HOME/.local/share/nimbus/machine/default/images/default.raw --no-info`
  with `NIMBUS_MACHINE_GUEST_BINARY=$HOME/.cache/nimbus/machine/guest-nimbus/v0.1.18-nimbus_linux_arm64-nimbus`.
- 2026-04-19: Completed CLIA7 and advanced the active roadmap item to CLIA8
  after an audit against the landed machine matrix and current service code.
  The machine family now satisfies the plan matrix and acceptance criteria:
  action commands use concise summaries, `status` / `list` / `inspect` keep
  clear human-vs-structured boundaries, and the new `machine start` quiet
  controls have real macOS PTY proof. The opening CLIA8 audit shows the next
  biggest gap clearly: `service up`, `service down`, `service list`,
  `service inspect`, and `service ps` still render default YAML-style dumps in
  `crates/nimbus-bin/src/service/mod.rs`, so the next slice should migrate
  those surfaces onto the shared action/table contract.
- 2026-04-19: Continued CLIA8 with the first service-mutation slice. Replaced
  the default YAML lifecycle dumps from `service up` and `service down` with
  shared action-block summaries that name the project, tenant, service action,
  sandbox id, and resulting status. Updated `docs/reference/cli.md` in the
  same change set. Verified with `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin macos_service_up_uses_forwarded_machine_api -- --nocapture`,
  and
  `cargo test -p nimbus-bin macos_service_commands_use_forwarded_machine_api_for_container_projects -- --nocapture`.
- 2026-04-19: Completed CLIA8 and advanced the active roadmap item to CLIA9.
  Migrated the remaining service summary/inspect surfaces onto the shared
  contract: `service list` now defaults to a human table with explicit
  `--format json|yaml|table`, `service inspect` now behaves like a structured
  inspect surface with JSON default plus explicit YAML, and `service ps` now
  defaults to a human process summary with explicit `--format json|yaml|table`.
  Updated `docs/reference/cli.md` in the same change set. Verified with
  `cargo fmt --all --check`, `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin service::tests:: -- --nocapture`, and
  `cargo test -p nimbus-bin`.
- 2026-04-19: Completed CLIA9 and advanced the active roadmap item to CLIA10.
  Promoted the earlier ad hoc isolated-root/local-binary drill into the
  checked-in `scripts/collect-nimbus-machine-cli-proof.sh` proof collector,
  added the deterministic
  `scripts/verify-nimbus-machine-cli-proof-helper.sh` harness verifier, wired
  both through `Makefile`, and updated `docs/reference/macos-machine-flow.md`
  so operators can distinguish the local-binary proof lane from the packaged
  Homebrew lane without guessing which binary or roots are under test.
  Revalidated with `bash -n scripts/collect-nimbus-machine-cli-proof.sh`,
  `bash -n scripts/verify-nimbus-machine-cli-proof-helper.sh`,
  `bash scripts/verify-nimbus-machine-cli-proof-helper.sh`,
  `cargo fmt --all --check`, and `cargo check -p nimbus-bin`.
- 2026-04-19: Completed CLIA10 and closed this control plane. Real-host proof
  refreshed four macOS lanes: an explicit negative-control bundle at
  `/tmp/nimbus-clia10-local.EVNbBo` showed that the `collect-nimbus-machine-cli-proof`
  `--image /path/to/default.raw` override is debug-only and does not exercise
  the shipped host-managed machine-API contract (`machine status` ended
  `API unreachable` and the forwarded socket stayed missing); the real default
  contract then succeeded at `/tmp/nimbus-clia10-default.DMMOvC` with the
  checked-in CLI collector plus host `service up/list/inspect/ps/logs/down`
  and localhost `http://127.0.0.1:18080/healthz` proof; the packaged
  Homebrew/cask machine lane succeeded at `/tmp/nimbus-clia10-cask.DWA3eu`;
  and the packaged machine-plus-service lane succeeded at
  `/tmp/nimbus-clia10-cask-service-clean.57DHQV` after an explicit packaged
  `machine start` restart between the machine-only and service-only collectors.
  During that rerun, the real packaged bundle exposed one remaining harness
  seam: `scripts/collect-nimbus-machine-service-proof.sh` still waited for the
  pre-CLIA8 YAML `status: ready` line even though the shipped `service inspect`
  default is JSON. Fixed the matcher to accept both JSON and YAML readiness,
  updated `scripts/verify-nimbus-machine-service-proof-helper.sh` to emit the
  JSON shape, and re-verified with
  `bash -n scripts/collect-nimbus-machine-service-proof.sh`,
  `bash scripts/verify-nimbus-machine-service-proof-helper.sh`,
  `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`,
  `cargo build --release -p nimbus-bin`,
  the real-host default-path bundle command
  `bash scripts/collect-nimbus-machine-cli-proof.sh --root /tmp/nimbus-clia10-default.DMMOvC --output-dir /tmp/nimbus-clia10-default.DMMOvC/cli-proof --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --keep-machine`
  plus
  `bash scripts/collect-nimbus-machine-service-proof.sh --home /tmp/nimbus-clia10-default.DMMOvC/home --runtime-root /tmp/nimbus-clia10-default.DMMOvC/runtime --output-dir /tmp/nimbus-clia10-default.DMMOvC/service-proof --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --compose-file /tmp/nimbus-clia10-default.DMMOvC/project/compose.yaml --service demo --published-url http://127.0.0.1:18080/healthz`,
  and the packaged lane command
  `bash scripts/collect-nimbus-homebrew-cask-proof.sh --output-dir /tmp/nimbus-clia10-cask-service-clean.57DHQV --host-binary /Users/jack/src/github.com/nimbus/nimbus/target/release/nimbus --keep-installed`
  followed by
  `HOME=/tmp/nimbus-clia10-cask-service-clean.57DHQV/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-clia10-cask-service-clean.57DHQV/runtime /opt/homebrew/bin/nimbus-dev machine start`,
  `bash scripts/collect-nimbus-machine-service-proof.sh --home /tmp/nimbus-clia10-cask-service-clean.57DHQV/home --runtime-root /tmp/nimbus-clia10-cask-service-clean.57DHQV/runtime --output-dir /tmp/nimbus-clia10-cask-service-clean.57DHQV/service-proof --nimbus /opt/homebrew/bin/nimbus-dev --compose-file /tmp/nimbus-clia10-cask-service-clean.57DHQV/project/compose.yaml --service demo --published-url http://127.0.0.1:18080/healthz`,
  then packaged `machine stop` / `machine rm` and Homebrew cask cleanup.
