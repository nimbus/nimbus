# CC9 — sccache-action pin floor + save-always retraction + stale-pin audit

## Why this exists

CC8 closed the plan; the verifier reached 11/12 locally with the
remaining condition being the live CI run on main. When that run on
`a29f14df` finally completed, it failed — every Rust job in the
matrix crashed with the same sccache startup error:

```
sccache: error: Server startup failed: cache storage failed to read:
Unexpected (permanent) at read => <h2>Our services aren't available
right now</h2>...
   uri: https://artifactcache.actions.githubusercontent.com/<token>/_apis/artifactcache/cache?keys=sccache/.sccache_check&version=sccache-v0.15.0
   response: Parts { status: 400, ... }
```

Initial hypothesis: GitHub Actions cache backend was having an outage.
The HTML body returned (`"Our services aren't available right now"`)
is Azure Front Door's standard error page, which matches what a real
GHA service interruption would look like.

That hypothesis was wrong. `https://www.githubstatus.com/` and the
Actions component status API both showed Actions fully operational
with no incidents in the prior 6 hours. The most recent Actions
incident was 2026-05-20, fully resolved before any CC commits landed.

## Root cause

`mozilla-actions/sccache-action@v0.0.6` (released **2024-09-27**)
predates GitHub's actions cache v1 → v2 migration. Subsequent action
releases addressed it:

- **v0.0.8** (2025-03-07) — release notes literally say: *"adjust the
  GHA changes and prepare release 0.0.8 — Fixes:
  mozilla/sccache#2351 — **Please update quickly!**"*
- **v0.0.9** (2025-06-18) — *"prepare release 0.0.9 + force github
  action 2"* (switched to the new cache backend protocol).
- **v0.0.10** (2026-04-22) — bumped to Node.js 24.

GitHub retired the legacy `_apis/artifactcache/cache` endpoint earlier
in 2025. Any caller using the deprecated protocol now gets HTTP 400 +
the Front Door HTML body. That's exactly the error sccache surfaced.

The CC plan pinned `v0.0.6` because that was the version live in
`.github/workflows/ci.yml` when CC1 wired the Coverage pilot. The pin
was never re-validated against the action's release cadence as CC2-CC5
expanded sccache usage across every Rust job.

## Save-always retraction

Separately: CC3 added `save-always: true` to every
`Swatinem/rust-cache@v2` invocation, intending to make reruns
re-save the cache. The actual CI logs surface this warning on every
job:

```
! Unexpected input(s) 'save-always', valid inputs are ['prefix-key',
'shared-key', 'key', 'add-job-id-key', 'add-rust-environment-hash-key',
'env-vars', 'workspaces', 'cache-directories', 'cache-targets',
'cache-on-failure', 'cache-all-crates', 'cache-workspace-crates',
'save-if', 'cache-provider', 'cache-bin', 'lookup-only', 'cmd-format']
```

`save-always: true` is **not a valid input** for
`Swatinem/rust-cache@v2`. It comes from `actions/cache@v4`'s schema,
not Swatinem's. Swatinem v2 saves on the success path by default and
exposes only `save-if` as a save-side knob — there is no equivalent
of `actions/cache`'s save-on-failure or save-on-rerun toggle.

The CC3 commit's rerun-safety claim was wrong-headed. The runtime
effect was nil (Swatinem ignored the unknown input), but the warning
noise polluted every CI run. CC9 removed it from all 12 invocations
across the three Rust workflows.

## Stale-pin audit (broader sweep)

After identifying the sccache-action regression, every action pin in
`.github/workflows/` was checked against its upstream `releases/latest`
endpoint. Three additional stale pins were found:

| Action | Was | Now | Latest | Sites |
|--------|-----|-----|--------|-------|
| `mozilla-actions/sccache-action` | `v0.0.6` | `v0.0.10` | `v0.0.10` | 12 |
| `actions/cache` | `v4` | `v5` | `v5.0.5` | 1 |
| `actions/upload-artifact` | `v4` | `v7` | `v7.0.1` | 2 |
| `actions/download-artifact` | `v7` | `v8` | `v8.0.1` | 3 |

All four bumps share a common theme: the upstream actions migrated to
Node.js 24 runtime and the new `@actions/artifact` / `@actions/cache`
backends, and our floor was below the cutoff. None of the bumps
introduce semantic API changes for our usage:

- `actions/upload-artifact` v5/v6 only added Node-runtime updates;
  v7 added an optional `archive: false` direct-upload mode we don't
  use.
- `actions/download-artifact` v8 errors on hash mismatch by default
  (was a warning in v7). We control both producer and consumer, so
  this is a safer default, not a breaking change for us.
- `actions/cache` v5 only requires runner ≥ 2.327.1; GitHub-hosted
  runners are well past that.

The remaining pins were already current at the major-version floor:
`actions/checkout@v6`, `actions/setup-node@v6`, `actions/setup-go@v6`,
`actions/attest@v4`, `actions/deploy-pages@v5`,
`actions/upload-pages-artifact@v5`, `Swatinem/rust-cache@v2` (latest
`v2.9.1` is still in the `v2` major series),
`codecov/codecov-action@v6`, `taiki-e/install-action@v2`,
`orhun/git-cliff-action@v4`, `shogo82148/actions-setup-perl@v1`,
`actions/create-github-app-token@v3.2.0`.

`dtolnay/rust-toolchain@stable` is a floating tag pin by design — the
action's maintainer updates `stable` as Rust releases happen, and the
recommended usage is exactly that floating ref.

## Verifier change

Condition 4 in
`scripts/verify-ci-caching-canonicalization.sh` was rewritten:

- **Old**: "Swatinem invocations set `save-always: true`" — counted
  the (nonexistent) input.
- **New**: "sccache-action pinned ≥ v0.0.10 and save-always
  retracted" — greps for any `mozilla-actions/sccache-action@v0.0.{0..9}`
  pin and any `save-always: true` line across the three Rust workflows.
  Both must be zero.

This makes the pin floor enforceable so a future careless downgrade
re-trips the verifier.

## Docs change

`docs/operating/ci-caching.md` was updated:

- Pin floor bumped to `v0.0.10` throughout.
- Removed `save-always: true` from the documented per-invocation
  options block.
- Removed the "CC3 rerun-save" section that described non-existent
  Swatinem behavior.
- Added a new triage entry (item 7) for the HTTP 400 sccache symptom
  → bump pin floor.

## What this commit changes

- `.github/workflows/ci.yml`: 9 sccache-action pin bumps + 9
  save-always removals + 3 download-artifact pin bumps.
- `.github/workflows/desktop-ui.yml`: 1 sccache-action pin bump + 1
  save-always removal + 1 actions/cache bump + 1 upload-artifact
  bump.
- `.github/workflows/node-compat-nightly.yml`: 2 sccache-action pin
  bumps + 2 save-always removals + 1 upload-artifact bump.
- `scripts/verify-ci-caching-canonicalization.sh`: condition 4
  rewritten; CC9 noted in header comment.
- `docs/operating/ci-caching.md`: pin floor + save-always rewrites +
  new triage entry.
- `docs/plans/archive/ci-caching-canonicalization-plan.md`: CC9 row
  added to the ledger as done; Execution Log appended with the
  CC9 SHA.

## Outcome

After CC9 lands and CI runs to completion on `main`, every sccache job
should start cleanly (no HTTP 400), the GHA cache v2 backend should
populate normally, and the `Unexpected input(s) 'save-always'` warning
should disappear from every job log.

## Sources

- `docs/plans/archive/ci-caching-canonicalization-plan.md` rows CC0-CC9.
- `https://github.com/Mozilla-Actions/sccache-action/releases` —
  v0.0.6 through v0.0.10 release notes.
- `https://github.com/actions/upload-artifact/releases` — v5/v6/v7
  release notes.
- `https://github.com/actions/download-artifact/releases` — v8
  release notes.
- `https://github.com/actions/cache/releases` — v5 release notes.
- `https://www.githubstatus.com/` — Actions component status at the
  time of the failed run on `a29f14df`.
- Failed run log:
  `gh run view 26311462482 --log-failed` (the `Rust Dependency Audit`
  job's stderr is the primary evidence).
