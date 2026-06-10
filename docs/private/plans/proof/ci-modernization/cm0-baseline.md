# CM0 — pre-CM1 baseline snapshot

Captures the state of every CI-modernization gap at the moment the plan
scaffold landed, so CM1-CM7 deltas are unambiguous later.

## Verifier baseline

`bash scripts/verify-ci-modernization.sh` at CM0 entry:

```
Summary: 1 passed, 11 failed

Failing conditions:
  - .github/actions/setup-rust-cached/action.yml missing
  - Inline sccache-action references remain — ci.yml(9); desktop-ui.yml(1); node-compat-nightly.yml(2);
  - Third-party uses: not properly pinned — every dtolnay/, Swatinem/,
    mozilla-actions/, taiki-e/, orhun/, shogo82148/, codecov/ uses: is tag-pinned
  - ubuntu-latest still used — apt-repo.yml(2); ci.yml(14); copr-srpms.yml(1);
    desktop-ui.yml(1); linux-distribution-release.yml(1);
    node-compat-nightly.yml(2); release.yml(2); verify-nimbus-crun-patch.yml(1)
  - actions/* pins use patch-version granularity —
    actions/create-github-app-token@v3.2.0 in release.yml (3 sites)
  - Insufficient GITHUB_STEP_SUMMARY usage — found 0, need >= 4
  - .github/workflows/codeql.yml missing
  - docs/plans/proof/ci-modernization/cm7-dependabot-audit.md missing
  - CLAUDE.md does not reference ci-modernization-plan
  - Ledger has unfinished rows — pending=9
  - Latest CI run on main is in_progress (sha=9d6b2adf, CC9 in flight)
```

The single PASS is condition 1 (plan file exists), confirming the
verifier itself is wired correctly. Every other condition correctly
reflects a real gap.

## Per-gap inventory

### Gap 1 — Duplicated Rust+sccache+Swatinem bootstrap (CM1)

Counted via `grep -c 'mozilla-actions/sccache-action' .github/workflows/*.yml`:

- `ci.yml`: 9 sites
- `desktop-ui.yml`: 1 site
- `node-compat-nightly.yml`: 2 sites
- **Total: 12 sites**

Each site is a 4-step block:

```yaml
- name: Configure googlesource credentials
  if: env.GOOGLESOURCE_COOKIE != ''
  env:
    GOOGLESOURCE_COOKIE: ${{ secrets.GOOGLESOURCE_COOKIE }}
  run: |
    ...

- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@stable

- name: Install sccache
  uses: mozilla-actions/sccache-action@v0.0.10

- name: Cache cargo artifacts
  uses: Swatinem/rust-cache@v2
  with:
    shared-key: <per-job-unique>
    cache-directories: ~/.cargo/.rusty_v8
    cache-on-failure: "true"
    save-if: ${{ github.ref == 'refs/heads/main' }}
```

The `shared-key` varies per job; some sites omit `cache-directories` or
add `cache-bin: "false"`. Otherwise the block is identical.

### Gap 2 — Zero SHA-pinning of third-party actions (CM2)

Inventory of third-party (non `actions/*`) action references across all
workflows + composite-actions:

| Action | Pin | Sites |
|--------|-----|-------|
| `dtolnay/rust-toolchain` | `@stable` | 9+ |
| `Swatinem/rust-cache` | `@v2` | 9+ |
| `mozilla-actions/sccache-action` | `@v0.0.10` | 12 |
| `taiki-e/install-action` | `@v2` | 2+ |
| `orhun/git-cliff-action` | `@v4` | 1 |
| `shogo82148/actions-setup-perl` | `@v1` | 1 |
| `codecov/codecov-action` | `@v6` | 1 |

Every one of these is tag-pinned. Modern OpenSSF Scorecard / CISA
supply-chain guidance is explicit: pin third-party actions to a 40-char
SHA with a `# vX.Y.Z` (or `# stable`) comment so the tag can't be
silently retargeted by a maintainer or attacker.

### Gap 3 — `runs-on: ubuntu-latest` non-determinism (CM3)

Sites using `runs-on: ubuntu-latest` (22 total):

```
apt-repo.yml(2); ci.yml(14); copr-srpms.yml(1); desktop-ui.yml(1);
linux-distribution-release.yml(1); node-compat-nightly.yml(2);
release.yml(2); verify-nimbus-crun-patch.yml(1)
```

`ubuntu-latest` is currently aliased to Ubuntu 24.04 by GitHub but the
mapping flips across major Ubuntu releases. Pipelines silently shift
when GitHub changes the alias; explicit `ubuntu-24.04` makes the
runner upgrade a deliberate code change.

ARM runners (`ubuntu-24.04-arm`) are already pinned at 3 sites.

### Gap 4 — `create-github-app-token@v3.2.0` over-pin (CM4)

```
release.yml:293 uses: actions/create-github-app-token@v3.2.0
release.yml:444 uses: actions/create-github-app-token@v3.2.0
release.yml:648 uses: actions/create-github-app-token@v3.2.0
```

Every other `actions/*` reference floats at major. The `v3.2.0` patch
pin blocks dependabot from rolling forward security patches within the
v3 line without manual approval each time.

### Gap 5 — Zero `$GITHUB_STEP_SUMMARY` usage (CM5)

```
$ grep -rn 'GITHUB_STEP_SUMMARY' .github/
(zero matches)
```

The CC verifier script, the deny audit, the coverage summary, the
desktop UI smoke walk, and the harness lanes all produce structured
output that lives only in raw log scrollback. Job summaries
(`echo "..." >> "$GITHUB_STEP_SUMMARY"`) would surface
pass/fail counts, top advisories, slowest tests, and links to artifacts
on the run summary page so triage doesn't require log diving.

### Gap 6 — No CodeQL workflow (CM6)

```
$ ls .github/workflows/codeql*.yml
(no matches)
```

Standard modern baseline for GitHub-hosted repos. Free for public
repos, multi-language scanning (Rust via custom build, JS/TS),
findings integrate with the Security tab.

### Gap 7 — Dependabot configured but didn't catch CC9 staleness (CM7)

`.github/dependabot.yml` exists and includes the `github-actions`
ecosystem (weekly Monday, grouped). However, when CC8 landed on
2026-05-22 the `mozilla-actions/sccache-action@v0.0.6` pin was already
~14 months stale (v0.0.8 was 2025-03-07). CM7 audits whether
dependabot had opened PRs that went unmerged, or whether the schedule
hadn't fired since the config was added (2026-05-12).

## What's already modern (so "mostly" wasn't empty)

The CC scope landed real wins that this baseline doesn't undo:

- Concurrency groups with `cancel-in-progress: true` on every workflow
- `permissions:` blocks (workflow- and job-level least-privilege)
- `timeout-minutes` on every job
- Dependabot config exists (just not catching everything)
- `CARGO_INCREMENTAL=0 + RUSTC_WRAPPER=sccache` env discipline
- Swatinem v2 with `save-if: refs/heads/main` PR-cannot-poison gate
- All action pins now at floor post-CC9 (still tag-pinned; CM2 fixes)
- Modern Node.js (v22 LTS) with npm caching wired into setup-node

The CM plan turns "mostly modern" into "all modern" by closing the
seven gaps above.
