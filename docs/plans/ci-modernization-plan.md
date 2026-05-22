# CI Modernization Plan (CM)

The CC plan canonicalized the **caching stack** (sccache, Swatinem v2,
ui-artifacts/warm-sccache leaders, save-if gates) and closed with the
CC9 stale-pin sweep. The CM plan canonicalizes the **CI infrastructure
around** that stack so the same modernization gaps cannot resurface.

## Why this plan exists

CC9's post-mortem surfaced seven concrete modernization gaps that were
deferred as out of CC scope. The CC plan title is "CI Caching
Canonicalization"; SHA-pinning, runner determinism, job summaries,
SAST coverage, and composite-action DRY are **CI hygiene**, not
caching. They deserve their own ledger so the work is auditable and
gated by an enforceable verifier.

Most importantly: CC9 had to bump **12 duplicated sites** for one
pin floor because the Rust toolchain + sccache + Swatinem cache +
googlesource bootstrap block is copy-pasted across 3 workflows. The
duplication is the root cause of CC9's blast radius. CM1 cures it.

## Scope

In scope:

- `.github/workflows/*.yml` (every CI workflow)
- `.github/actions/**` (new composite actions)
- `.github/dependabot.yml` (config audit + adjustments)
- `scripts/verify-ci-modernization.sh` (this plan's verifier)
- `docs/operating/ci-modernization.md` (canonical contract after
  closeout)
- Routing entries in `docs/plans/README.md`, `AGENTS.md`,
  `CLAUDE.md`

Out of scope:

- Caching mechanics — owned by archived CC plan
- Rust workspace structure, test layout, harness — owned by other
  active plans
- Release-pipeline shape (signing, attestation, distribution) —
  owned by `distribution-plan.md` family

## Ledger

| CM  | Description | Status |
|-----|-------------|--------|
| CM0 | Scaffold this plan + the verifier at `scripts/verify-ci-modernization.sh` with 12 conditions. Routing entries added to `docs/plans/README.md` + `AGENTS.md` + `CLAUDE.md`. Baseline proof at `docs/plans/proof/ci-modernization/cm0-baseline.md` captures the state of each gap before remediation. | done |
| CM1 | Extract `.github/actions/setup-rust-cached/action.yml` composite action consolidating the Rust toolchain + sccache + Swatinem cache + googlesource credentials bootstrap. Migrate all 12 sites in `ci.yml`, `desktop-ui.yml`, `node-compat-nightly.yml` to `uses: ./.github/actions/setup-rust-cached`. Verifier asserts: composite action file exists; zero inline `mozilla-actions/sccache-action` references outside the composite; every Rust job that previously had the 4-step block now uses the composite. This is the **single highest-leverage** change because the next sccache-action bump becomes a 1-line PR. | done |
| CM2 | SHA-pin every third-party action (`mozilla-actions/*`, `Swatinem/*`, `dtolnay/*`, `taiki-e/*`, `orhun/*`, `shogo82148/*`, `codecov/*`) to a 40-char SHA with a `# vX.Y.Z` version-name comment. First-party `actions/*` may remain tag-pinned (lower supply-chain risk; GitHub controls). Dependabot config keeps updating SHA pins via comments. After CM1, the third-party surface is concentrated in the composite action — this step touches very few sites. Verifier asserts every non-`actions/*` `uses:` is a 40-char hex SHA with a version comment within 2 lines. | done |
| CM3 | Replace `runs-on: ubuntu-latest` with `runs-on: ubuntu-24.04` across all 22 sites that use it. ARM jobs (`ubuntu-24.04-arm`) already pinned. Verifier asserts zero `ubuntu-latest` references remain. | pending |
| CM4 | Bump `actions/create-github-app-token@v3.2.0` to `@v3` so patch updates flow naturally. One-site change. Verifier asserts no patch-version pinning of `actions/*` actions. | pending |
| CM5 | Emit `$GITHUB_STEP_SUMMARY` markdown from high-value jobs: the CC verifier-equivalent reports, `cargo deny` output, coverage summary, the desktop UI smoke walk. Each job appends a structured markdown block (pass/fail counts, top advisories, link to artifacts) so the run summary page is informative without raw-log diving. Verifier asserts at least N (TBD; ≥4) jobs reference `GITHUB_STEP_SUMMARY`. | pending |
| CM6 | Add `.github/workflows/codeql.yml` using GitHub's CodeQL template configured for the languages we ship (Rust via custom build, JavaScript/TypeScript). Standard schedule (weekly) + on PR. Verifier asserts the workflow exists and references `github/codeql-action`. | pending |
| CM7 | Audit the dependabot PR queue (`gh pr list --author dependabot`). Confirm the github-actions ecosystem is firing on its schedule; if pending PRs exist for any action we just bumped, close them as superseded with a comment. Document findings in proof. Verifier is a doc-presence check at `docs/plans/proof/ci-modernization/cm7-dependabot-audit.md`. | pending |
| CM8 | Closeout. Flip every ledger row to `done`, append Execution Log with actual SHAs, move plan to `docs/plans/archive/`, promote `docs/operating/ci-modernization.md` (synthesis of CM1-CM6 contracts), update routing in `docs/plans/README.md` + `AGENTS.md` to the archived path. Verifier's plan-file regex accepts both active and archived paths. | pending |

## Completion Gate

`bash scripts/verify-ci-modernization.sh` exits 0 with summary line
`12 passed, 0 failed`. The 12 conditions:

1. Plan file exists (`docs/plans/ci-modernization-plan.md` or
   `docs/plans/archive/ci-modernization-plan.md`).
2. Composite action exists at
   `.github/actions/setup-rust-cached/action.yml`.
3. Zero inline `mozilla-actions/sccache-action` references in
   workflow files (all flow through the composite).
4. Every third-party (non-`actions/*`) `uses:` in workflow files is
   SHA-pinned (40-char hex) with a version comment within 2 lines.
5. Zero `runs-on: ubuntu-latest` references; every Ubuntu runner is
   pinned to `ubuntu-24.04` (or `-arm`).
6. No `actions/*` pin uses patch-version granularity (no `@vN.M.P`
   pattern; only `@vN`).
7. At least 4 jobs reference `GITHUB_STEP_SUMMARY`.
8. `.github/workflows/codeql.yml` exists and references
   `github/codeql-action`.
9. CM7 audit doc exists at
   `docs/plans/proof/ci-modernization/cm7-dependabot-audit.md`.
10. Routing entry exists in CLAUDE.md naming this plan.
11. Every ledger row marked `done`.
12. Latest CI run on main is green (status=completed,
    conclusion=success).

## Proof directory

`docs/plans/proof/ci-modernization/`:

- `cm0-baseline.md` — snapshot of every modernization gap pre-CM1
- `cm1-composite-action.md` — extraction rationale + before/after
  diff sketches
- `cm2-sha-pinning.md` — SHA lookups + supply-chain rationale
- `cm3-ubuntu-pin.md` — runner determinism rationale
- `cm5-job-summaries.md` — chosen jobs + markdown shape
- `cm6-codeql.md` — language coverage rationale
- `cm7-dependabot-audit.md` — PR-queue findings
- `cm8-closeout.md` — final state + retro

## Execution Log

Will be appended as each CM lands on main.

| CM  | Commit(s) | Subject |
|-----|-----------|---------|

## Notes on staging order

CM1 first because every later step touches fewer sites once the
composite exists:

- CM2 (SHA-pin) goes from `12+ third-party usage sites` to
  `~3 sites in the composite + ~6 sites for one-off third-party
  actions (codecov, taiki-e, orhun, shogo82148)`.
- CM3 (ubuntu-pin) is orthogonal to the composite but cheap to
  apply afterward.
- CM5 (job summaries) is independent.
- CM6 (CodeQL) is a new file.

Within the wave, each CM is a separate commit so the Execution Log
SHAs are individually auditable.
