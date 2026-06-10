# CM2: SHA-pin third-party actions

CM2 replaces every third-party `uses:` reference in workflows (and the
CM1 composite action) with a 40-char commit SHA followed by a
`# vX.Y.Z` version-name comment. First-party `actions/*` references
stay tag-pinned because GitHub owns those releases and Dependabot
keeps the major-tag pointers fresh.

## Why SHA-pin third parties

A tag like `@v2` or `@stable` is a movable pointer. Anyone with push
access to the action repo (compromised maintainer account, malicious
PR merged by an underwater reviewer, etc.) can repoint that tag to a
new tree containing a credential exfiltrator. OpenSSF Scorecard's
"Pinned-Dependencies" check and CISA's supply-chain guidance both call
this out as the highest-leverage hardening step for GitHub Actions.

SHA pins make the action immutable at the call site. A bad new release
upstream cannot affect already-pinned callers; Dependabot opens a PR
to bump the SHA, which is reviewable diff that surfaces the new
contents.

## Site inventory

After CM1 most of the third-party surface is concentrated in
`.github/actions/setup-rust-cached/action.yml`. CM2 SHA-pins that
composite plus the remaining one-off third-party `uses:` lines across
`ci.yml` and `release.yml`.

### `.github/actions/setup-rust-cached/action.yml`

| Step | Action | SHA | Source tag |
|------|--------|-----|------------|
| Install Rust toolchain | `dtolnay/rust-toolchain` | `29eef336d9b2848a0b548edc03f92a220660cdb8` | `stable` branch head |
| Install sccache | `mozilla-actions/sccache-action` | `9e7fa8a12102821edf02ca5dbea1acd0f89a2696` | `v0.0.10` |
| Cache cargo artifacts | `Swatinem/rust-cache` | `e18b497796c12c097a38f9edb9d0641fb99eee32` | `v2` |

### `.github/workflows/ci.yml`

| Line | Action | SHA | Source tag |
|------|--------|-----|------------|
| 43 (rust-format) | `dtolnay/rust-toolchain` | `29eef336d9b2848a0b548edc03f92a220660cdb8` | `stable` |
| 99 (deny) | `taiki-e/install-action` | `6c1f7cf125e42770ff087ea443901b487cc5471a` | `v2` (was `@cargo-deny` shorthand) |
| 151 (workspace nextest) | `taiki-e/install-action` | `6c1f7cf125e42770ff087ea443901b487cc5471a` | `v2` |
| 590 (coverage llvm-cov) | `taiki-e/install-action` | `6c1f7cf125e42770ff087ea443901b487cc5471a` | `v2` (was `@cargo-llvm-cov` shorthand) |
| 664 (codecov upload) | `codecov/codecov-action` | `e79a6962e0d4c0c17b229090214935d2e33f8354` | `v6` |

`taiki-e/install-action` supports shorthand refs like `@cargo-deny` and
`@cargo-llvm-cov` that pin the install-action version *and* select the
tool. SHA-pinning is incompatible with that shorthand; CM2 converts
each site to the canonical `@v2 + tool: <name>` form so it can be
SHA-pinned.

### `.github/workflows/release.yml`

| Line | Action | SHA | Source tag |
|------|--------|-----|------------|
| 94, 187 | `dtolnay/rust-toolchain` | `29eef336d9b2848a0b548edc03f92a220660cdb8` | `stable` |
| 97, 212 | `Swatinem/rust-cache` | `e18b497796c12c097a38f9edb9d0641fb99eee32` | `v2` |
| 191 | `shogo82148/actions-setup-perl` | `a198315ec4e9244f206879ea7b63078003aec8a6` | `v1` |
| 581, 590 | `orhun/git-cliff-action` | `f50e11560dce63f7c33227798f90b924471a88b5` | `v4` |

The three `actions/create-github-app-token@v3.2.0` sites in `release.yml`
are first-party but use patch-version granularity; CM4 owns the
`v3.2.0 → v3` major-only repin separately.

## SHA collection methodology

For tag refs (`v0.0.10`, `v2`, etc.), via `gh api`:

```
gh api repos/<owner>/<repo>/git/refs/tags/<tag>          # → tag object SHA
gh api repos/<owner>/<repo>/git/tags/<tag-object-sha>    # → commit SHA
```

(Annotated tags need the second hop; lightweight tags return the
commit SHA directly from the first call.)

For branch refs (`dtolnay/rust-toolchain@stable` is a branch, not a
tag), via:

```
gh api repos/<owner>/<repo>/branches/<branch>
```

The branch head is captured as of CM2 land. Dependabot will keep this
SHA fresh going forward — it monitors the upstream and opens PRs to
bump the SHA with the matching `# vX.Y.Z` comment.

## Verifier delta

Before CM2:

- Condition 4 (third-party SHA-pinned): FAIL — third-party `uses:` lines
  carry tag refs.

After CM2:

- Condition 4: PASS — every non-`actions/*` `uses:` in
  `.github/workflows/` is a 40-char hex SHA followed within 2 lines by
  a `# vX.Y.Z` version-name comment.

The CC verifier (`scripts/verify-ci-caching-canonicalization.sh`)
remains 12/12 passed because the pin-floor search corpus added the
composite in the CM1 hotfix; SHA pins on `sccache-action` and
`rust-cache` do not change the floor logic.

## Sites intentionally not SHA-pinned

- `actions/*` first-party: GitHub owns these release pipelines. Tag
  pins remain the recommended shape; Dependabot patches the majors.
- `./.github/workflows/*` and `./.github/actions/*` local references:
  these are evaluated from the repo's own tree, not from a remote
  registry; the verifier excludes them by anchored `^[a-zA-Z0-9_-]+/`
  match.
- `actions/create-github-app-token@v3.2.0`: patch-version pin owned
  by CM4.
