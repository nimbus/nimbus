# CM4: bump actions/create-github-app-token from @v3.2.0 to @v3

CM4 repins three sites in `release.yml` from
`actions/create-github-app-token@v3.2.0` (patch-version) to
`actions/create-github-app-token@v3` (major-only).

## Why major-only for first-party actions

GitHub's own `actions/*` releases follow semver and maintain
`@vN` major-tag pointers that always reference the latest stable
`vN.M.P`. Pinning a patch version freezes patch updates from
flowing — that includes the security and bug-fix patches that
Dependabot would otherwise pull in transparently.

`@v3.2.0` was an accidental over-pin. Tracking the major preserves
Dependabot's ability to keep first-party actions current without
manual intervention, and surfaces a reviewable PR if/when a
breaking `@v4` ships.

(Third-party actions still SHA-pin per CM2; the supply-chain
posture differs because GitHub does not own those releases.)

## Sites changed

`.github/workflows/release.yml`:

| Line | Job |
|------|-----|
| 293 | machine-os release dispatch |
| 444 | linux-distribution-release dispatch |
| 648 | post-release machine-os release dispatch |

All three call sites use the action to mint an installation token
for the `MACHINE_OS_RELEASE_APP` GitHub App; the patch over-pin had
no semantic effect — it just blocked Dependabot.

## Verifier delta

Before CM4:

- Condition 6 (actions/* major-only): FAIL — 3 hits of
  `@v3.2.0` patch pin.

After CM4:

- Condition 6: PASS — `grep -rnE 'actions/[a-z-]+@v[0-9]+\.[0-9]+'
  .github/workflows/` returns no matches.
