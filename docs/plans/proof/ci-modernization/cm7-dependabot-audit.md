# CM7: Dependabot audit (2026-05-22)

CM7 audits the Dependabot configuration and PR queue at the moment
CM1-CM6 finish landing. This proof captures the state so a future
agent can tell whether subsequent activity is healthy or stuck.

## Configuration in tree

`.github/dependabot.yml`:

| Ecosystem | Directory | Schedule | Grouping |
|-----------|-----------|----------|----------|
| `cargo` | `/` | weekly, Monday | `rust-dependencies: *` |
| `github-actions` | `/` | weekly, Monday | `github-actions: *` |
| `npm` | `/` | weekly, Monday | `npm-dependencies: *` |

All three ecosystems group every package into a single PR per
ecosystem per week. This matches the CC and CM cadence — review a
small, finite number of PRs once a week rather than dozens of
single-package PRs.

After CM2, the `github-actions` ecosystem is the highest-leverage
one: SHA-pinned third-party actions will surface SHA bumps as
reviewable PRs with the `# vX.Y.Z` comment preserved.

## PR queue snapshot

`gh pr list --author "dependabot[bot]" --state all --limit 20`:

```
[]
```

No PRs open or closed by dependabot exist as of the audit. The most
plausible reasons:

1. **Dependabot has not yet executed its first scheduled run.** The
   schedule is Monday weekly; CM2 landed on 2026-05-22 (Friday). The
   next firing is Monday 2026-05-25. Until then, the github-actions
   ecosystem has nothing to scan against the SHA-pinned references.
2. **No updates were available the last time Dependabot scanned.**
   For a project this young the github-actions ecosystem may have
   nothing newer than the explicit pins; Dependabot opens a PR only
   when a newer version is available.

Either way, there are no superseded PRs to close. If the next
Monday scan produces PRs for action versions we have already
pinned to the latest SHA in CM2, they should be closed as
superseded with a comment referencing this audit.

## Repository security_and_analysis state

`gh api repos/nimbus/nimbus` reports:

| Feature | Status |
|---------|--------|
| `dependabot_security_updates` | **disabled** |
| `secret_scanning` | disabled |
| `secret_scanning_push_protection` | disabled |
| `secret_scanning_non_provider_patterns` | disabled |
| `secret_scanning_validity_checks` | disabled |

`dependabot_security_updates` being disabled means a CVE on a
direct dependency will not auto-open a PR; only the weekly
version-update PRs fire. Enabling it is a documentation-only
follow-up (a UI toggle in repository settings — not a workflow
change, so out of CM7 scope).

Secret scanning + push protection are likewise UI toggles. Out of
CM scope; capture as future work.

## Action items (out of CM scope)

- Toggle `dependabot_security_updates` to enabled in repo settings.
- Toggle secret scanning + push protection.
- Verify the first post-CM2 github-actions PR (week of 2026-05-25)
  closes cleanly or is closed-superseded with a link to this audit.

## Verifier delta

Before CM7:

- Condition 9 (audit doc present): FAIL — file missing.

After CM7:

- Condition 9: PASS — this file exists at
  `docs/plans/proof/ci-modernization/cm7-dependabot-audit.md`.
