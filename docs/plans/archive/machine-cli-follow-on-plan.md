# Plan: Machine CLI Follow-on UX Plan

Canonical execution plan for the next Neovex CLI UX wave after the completed
machine/service alignment rollout archived at
`docs/plans/archive/machine-cli-alignment-plan.md`.

This plan owns the next round of Podman-aligned and Docker-informed command UX
work for the shipped CLI surface, with a bias toward battle-tested operator
patterns over bespoke formatting.

Reviewed against:

- `crates/neovex-bin/src/main.rs`
- `crates/neovex-bin/src/machine/mod.rs`
- `crates/neovex-bin/src/machine/manager.rs`
- `crates/neovex-bin/src/service/mod.rs`
- `docs/reference/cli.md`
- `docs/plans/archive/machine-cli-alignment-plan.md`
- `/Users/jack/src/github.com/containers/podman/cmd/podman/machine/info.go`
- `/Users/jack/src/github.com/containers/podman/cmd/podman/machine/list.go`
- `/Users/jack/src/github.com/containers/podman/cmd/podman/machine/inspect.go`

## Status

- **Status:** `completed`
- **Primary owner:** this plan
- **Active roadmap item:** none
- **Activation gate:** completed comparative audit on 2026-04-19 confirmed that
  the aligned CLI baseline is stable, but still misses a few important
  Podman/Docker operator ergonomics
- **Related references:**
  - `docs/reference/microvm-service-baseline.md`
  - `docs/reference/macos-machine-flow.md`
  - `docs/reference/cli.md`
  - `docs/plans/archive/machine-cli-alignment-plan.md`

## Current Assessed State

- `neovex machine status` now defaults to a compact table and `machine start`
  has human-friendly progress output.
- The CLI now includes a host-level `neovex machine info` command analogous to
  `podman machine info`, with a stable YAML/JSON structured contract.
- Summary and inspect output now use consistent `-f` / `--format` aliases, and
  table-producing summary commands support `--noheading` for operator-friendly
  piping.
- Go-template output remains intentionally deferred. The current shipped
  machine-readable contract is `json` / `yaml`, and the human summary contract
  is the default table renderer.
- `machine list` now surfaces the default machine in the human table, exposes
  the `default` bit in structured output, and sorts active machines ahead of
  inactive ones with the default machine anchored near the top.
- Help/examples now lead with the settled short-flag style and point to the
  current macOS flow reference instead of an obsolete active-plan pointer.

## Control Plan Rules

- Treat the current git worktree, this plan, and `docs/reference/cli.md` as the
  source of truth for this workstream.
- Podman is the primary implementation and UX reference when Neovex already has
  a command with matching semantics.
- Docker is a secondary product-surface comparator for concise operator tone and
  familiar output defaults.
- Breaking changes are preferred over compatibility shims or dual behavior.
- Human output and machine-readable output are different products:
  structured output must stay boring and stable, while tables/progress/hints
  remain human-only.
- Keep shared UX logic in reusable helpers instead of duplicating per-command
  formatting.

## Roadmap Status Ledger

| ID | Status | Scope | Verification |
| --- | --- | --- | --- |
| `CLIF1` | `completed` | Add Podman-aligned host-level `neovex machine info` with a documented structured output contract and root discovery summary. | `cargo fmt --all --check`; `cargo check -p neovex-bin`; focused `cargo test -p neovex-bin machine_info -- --nocapture`; manual `./target/release/neovex machine info` / `--format json` |
| `CLIF2` | `completed` | Expand output-shaping parity for summary/inspect commands: consistent `-f`/`--format`, header control, and the next deliberate decision on template support. | `cargo fmt --all --check`; `cargo check -p neovex-bin`; focused `cargo test -p neovex-bin machine_status -- --nocapture`; `cargo test -p neovex-bin machine_list -- --nocapture`; `cargo test -p neovex-bin machine_inspect -- --nocapture`; `cargo test -p neovex-bin service_list -- --nocapture`; `cargo test -p neovex-bin service_inspect -- --nocapture`; `cargo test -p neovex-bin service_ps -- --nocapture`; `cargo test -p neovex-bin render_table_ -- --nocapture`; direct `./target/debug/neovex machine list --help`; `./target/debug/neovex machine inspect --help`; `./target/debug/neovex service list --help`; `./target/debug/neovex service ps --help`; isolated `./target/debug/neovex machine status --noheading`; isolated `./target/debug/neovex machine list --noheading` |
| `CLIF3` | `completed` | Improve `machine list` operator ergonomics: ordering, default/running signals, and better parity with Podman’s human table. | `cargo fmt --all --check`; `cargo check -p neovex-bin`; focused `cargo test -p neovex-bin machine_list -- --nocapture`; direct isolated `./target/debug/neovex machine list`; isolated `./target/debug/neovex machine list -f json` |
| `CLIF4` | `completed` | Reconcile remaining help/example drift with the active macOS contract and the settled CLI style system. | `cargo fmt --all --check`; `cargo check -p neovex-bin`; focused `cargo test -p neovex-bin machine_help_uses_user_facing_descriptions -- --nocapture`; `cargo test -p neovex-bin machine_info_help_describes_structured_formats -- --nocapture`; direct `./target/debug/neovex machine --help`; `./target/debug/neovex machine info --help` |
| `CLIF5` | `completed` | Final real-host proof on the shipped macOS path if any user-facing machine/service surface changes materially. | Real macOS-host CLI proof bundle at `/tmp/neovex-clif5-proof.RuHf4S` with isolated root `/tmp/neovex-clif5-root.N3T9bB`; captures `machine --help`, `machine info --help`, `machine list --help`, `machine status --help`, `service list --help`, `service ps --help`, `machine status --noheading`, `machine list`, and `machine list -f json` |

## Execution Log

- 2026-04-19: Promoted this active follow-on plan from the completed archived
  alignment plan after a fresh Podman/Docker comparative audit. The audit
  identified three highest-signal gaps: missing `machine info`, narrower output
  shaping than Podman/Docker, and a less operator-friendly `machine list`.
- 2026-04-19: Landed `CLIF1` by adding `neovex machine info`, updating the CLI
  reference and agent/docs ownership, and keeping the next output-shaping wave
  explicitly tracked as `CLIF2`.
- 2026-04-19: Landed `CLIF2` by adding consistent `-f` aliases to
  machine/service summary and inspect commands, threading `--noheading`
  through the shared table renderer, documenting the explicit decision to defer
  template output, and fixing a real empty-table panic caught by the isolated
  direct CLI proof for `machine list --noheading`. `CLIF3` is now the active
  ergonomics item.
- 2026-04-19: Landed `CLIF3` by making `machine list` more Podman-like for
  operators: active machines sort ahead of inactive ones, the default machine
  is marked with `*` in human table output, and structured JSON now exposes a
  stable `default` boolean. Direct isolated macOS-host proof confirmed the
  human table and JSON surfaces.
- 2026-04-19: Landed `CLIF4` by reconciling the remaining help/example drift:
  the machine help now leads with the settled `-f` style, `machine info`
  examples match the same convention, and `docs/reference/cli.md` points to
  the current macOS flow reference instead of an obsolete active-plan note.
  `CLIF5` is now the active real-host closeout item.
- 2026-04-19: Landed `CLIF5` by capturing a real macOS-host proof bundle under
  `/tmp/neovex-clif5-proof.RuHf4S` using isolated roots under
  `/tmp/neovex-clif5-root.N3T9bB`. The bundle records the final live help
  surfaces plus the machine status/list human and JSON outputs after the CLI
  follow-on wave. All roadmap items are now complete and the plan is ready to
  archive.
