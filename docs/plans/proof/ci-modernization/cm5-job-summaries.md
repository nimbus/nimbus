# CM5: Job summaries via $GITHUB_STEP_SUMMARY

CM5 makes four CI jobs append a structured markdown block to
`$GITHUB_STEP_SUMMARY` so the GitHub Actions run summary page is
informative without having to open raw logs.

## Why

`$GITHUB_STEP_SUMMARY` is a per-job file that GitHub renders as
markdown on the workflow-run summary page. A well-shaped summary
turns the run page into a usable triage surface — failure mode,
metric trend, link to artifacts — so the most common question
("did anything regress on this push?") can be answered without
clicking into each job's log.

The bar is intentionally narrow: this should be the high-value
report, not a copy of the log. Each summary is bounded
(`tail -n 80` or a short metric table), uses GitHub's emoji-shortcode
status icons, and degrades gracefully (an `if: always()` wrapper or
defensive file check so a failed prior step still emits its
summary).

## Jobs instrumented

| Job | Workflow | Summary shape |
|-----|----------|---------------|
| `deny` | `ci.yml` | status + last 60 lines of `cargo deny check` |
| `coverage` | `ci.yml` | files instrumented, lines hit / total, percentage |
| `rust-gate-summary` | `ci.yml` | per-lane pass/fail table for 6 Rust gates |
| `desktop-ui` | `desktop-ui.yml` | status + last 80 lines of `make verify-desktop-ui` |

Each summary block:

- Leads with an `H2` markdown heading that names the job.
- Uses GitHub's status emoji-shortcodes (`:white_check_mark:`, `:x:`,
  `:no_entry:`, `:fast_forward:`) for visual scanning.
- Bounds the embedded log output to a fixed tail so the run page
  stays compact.
- Wraps embedded raw output in a `<details>` block so the default
  view is the structured headline.

The coverage step computes `LH:` / `LF:` totals from `lcov.info` with
awk; this matches the line-coverage metric Codecov reports without
needing an extra tool installed.

## Verifier delta

Before CM5:

- Condition 7 (>= 4 `GITHUB_STEP_SUMMARY` references): FAIL — 0 hits.

After CM5:

- Condition 7: PASS — 7 references across `ci.yml` and
  `desktop-ui.yml`. (Each summary block emits multiple
  `$GITHUB_STEP_SUMMARY` writes, so the count exceeds the 4-job
  floor.)
