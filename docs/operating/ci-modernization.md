# CI modernization contract

Canonical contract for the CI infrastructure that surrounds the
caching stack documented in `ci-caching.md`. Authored by the
CI Modernization (CM) plan; see
`docs/plans/archive/ci-modernization-plan.md` for the underlying
ledger and proof artifacts.

## Single composite action for Rust toolchain bootstrap

`.github/actions/setup-rust-cached/action.yml` is the only place the
following third-party actions are referenced:

- `dtolnay/rust-toolchain`
- `mozilla-actions/sccache-action`
- `Swatinem/rust-cache`

Every Rust job that needs a toolchain + sccache + cargo cache calls
the composite:

```yaml
- name: Set up Rust with cache
  uses: ./.github/actions/setup-rust-cached
  with:
    shared-key: <ci-ubuntu-stable-<job>-no-bin-v2>
    toolchain-components: <optional rustup components>
    cache-bin: <"false" by default; "true" only for desktop-ui>
    googlesource-cookie: ${{ secrets.GOOGLESOURCE_COOKIE }}
```

When the upstream actions need bumping, **edit one file**. Do not
inline `mozilla-actions/sccache-action` or `Swatinem/rust-cache` into
a job; if a new Rust job ships, route it through the composite.

The CC `save-if: refs/heads/main` gate and the `cache-bin: "false"`
default flow through the composite — see `ci-caching.md` for the
caching contract those settings enforce.

## SHA pinning policy

Every non-`actions/*` `uses:` reference in `.github/workflows/*.yml`
and `.github/actions/**/action.yml` MUST be:

1. A 40-char hex commit SHA.
2. Followed by a comment on the same line or within the next two
   lines containing the human-readable version tag (e.g.
   `# v0.0.10`, `# stable`, `# v4`).

Dependabot keeps SHA pins fresh; the version-name comment is what
makes the diff legible in the PR.

`actions/*` first-party references may stay tag-pinned, but only at
major granularity (`@v3`, not `@v3.2.0`). Patch pinning blocks
Dependabot from flowing security patches.

Three-segment refs like `github/codeql-action/init@<sha>` are
allowed; the verifier regex accepts nested action sub-paths.

## Runner pinning

Use explicit Ubuntu LTS image tags. **Never** use the moving alias
`ubuntu-latest`. Today's pin is `ubuntu-24.04` (plus
`ubuntu-24.04-arm` for ARM jobs). `ubuntu-22.04` may still appear
where ABI compatibility with the release archive requires it
(e.g. `release.yml::build` for `x86_64-unknown-linux-gnu`); document
the reason inline if you keep an older image.

## Job summaries

High-value jobs emit a structured `$GITHUB_STEP_SUMMARY` markdown
block so the workflow-run page is a useful triage surface. The
established shape:

```bash
{
  echo "## <job name>"
  echo
  echo "**Status:** :white_check_mark: passed"   # or :x:, :no_entry:
  echo
  echo "<details><summary>Output (last N lines)</summary>"
  echo
  echo '```'
  tail -n N /tmp/out
  echo '```'
  echo
  echo "</details>"
} >> "${GITHUB_STEP_SUMMARY}"
```

Use `:white_check_mark:` / `:x:` / `:no_entry:` / `:fast_forward:`
for status icons. Bound embedded log output (`tail -n 80` is a
sensible default). Defensive: wrap in `if: always()` and check
output files exist so a failed earlier step still emits its
summary.

Jobs currently emitting summaries: `deny`, `coverage`,
`rust-gate-summary` (ci.yml); `desktop-ui` (desktop-ui.yml).

## Static analysis (CodeQL)

`.github/workflows/codeql.yml` runs `github/codeql-action/init` and
`github/codeql-action/analyze` (SHA-pinned per CM2) over a matrix of
JavaScript/TypeScript (build-mode `none`) and Rust (build-mode
`manual`, using `make check` as the build step). Schedule: push to
main, PR targeting main, weekly cron, manual dispatch.

Results land in the GitHub Security tab. False positives can be
tuned later via `.github/codeql/codeql-config.yml`; no config file is
required for the default `security-and-quality` query suite.

## Dependabot

`.github/dependabot.yml` covers `cargo`, `github-actions`, and `npm`
with a weekly Monday cadence and per-ecosystem grouping. The
`github-actions` ecosystem is the one that surfaces SHA bumps for
the third-party actions in the composite and one-off workflow sites.
PRs preserve the `# vX.Y.Z` comment.

## Aggregate gate

`bash scripts/verify-ci-modernization.sh` is the local equivalent of
the closeout check; it exits 0 iff all 12 conditions of the original
CM plan are satisfied. The CC plan has its own gate at
`scripts/verify-ci-caching-canonicalization.sh`. Both gates can be
run independently; CM2 is composite-aware on the CC side, so neither
regresses the other.

## Routing

- Canonical contract: this file (`docs/operating/ci-modernization.md`).
- Plan archive: `docs/plans/archive/ci-modernization-plan.md`.
- Proof artifacts: `docs/plans/proof/ci-modernization/`.
- Verifier: `scripts/verify-ci-modernization.sh`.
- Sister contract (caching layer): `docs/operating/ci-caching.md`.
