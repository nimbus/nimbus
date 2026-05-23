# CM6: CodeQL SAST workflow

CM6 adds `.github/workflows/codeql.yml`, a matrix of CodeQL analyses
covering the languages Nimbus ships.

## Why

CodeQL is GitHub's hosted static analysis engine. It runs query packs
against a built program database to flag whole-program issues
(injection, unsafe deserialization, taint-flow bugs, etc.) that
linters do not see. Until CM6, Nimbus had no SAST coverage in CI.

The "security-and-quality" query suite is the right baseline: it
includes both the security-only pack and the broader correctness
queries. False-positive rate is acceptable for a project of this
size; tune via `.github/codeql/codeql-config.yml` later if noise
becomes an issue.

## Language matrix

| Language | build-mode | Notes |
|----------|------------|-------|
| `javascript-typescript` | `none` | CodeQL extracts from source without a build step (preferred for monorepo JS/TS). |
| `rust` | `manual` | Rust support is GA. CodeQL is given an explicit build (`make check`) so type checking + macro expansion run before extraction. |

`make check` is used in place of `cargo check` so the UI dependency
graph (codegen + SPA build) runs first — `nimbus-server` has
`include_str!` references to `.nimbus/convex/` outputs that must
exist before the Rust extractor runs. This mirrors what the cached
clippy lane does.

## Schedule

- On every push to `main` and every pull-request targeting `main`.
- Weekly cron (`0 9 * * 1`) so new query packs and advisory drops
  catch the most recent main.
- Manual `workflow_dispatch` for ad-hoc reruns when investigating.

## Permissions

`security-events: write` is required so CodeQL can publish SARIF
results to the GitHub Security tab. The job is otherwise
read-only.

## Verifier delta

Before CM6:

- Condition 8 (`.github/workflows/codeql.yml` references
  `github/codeql-action`): FAIL — file missing.

After CM6:

- Condition 8: PASS — file present and references
  `github/codeql-action/init@v3` and
  `github/codeql-action/analyze@v3`.
