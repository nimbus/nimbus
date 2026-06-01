# DUA5 Deno Release And Nimbus Repin

status: done
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
source worktree: /Users/jack/src/github.com/nimbus/deno
source branch: nimbus/v2.8.1
pr: https://github.com/nimbus/nimbus/pull/11
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA5 is done. Release and Nimbus repin work consumed
   the maintainer-approved `v149.2.0-nimbus.1` substrate.
2. **Input baseline.** DUA4 source commit is
   `18f76a9a19ab74d49d9a40037733cc4aec983d26`; `nimbus/deno` default branch is
   currently `nimbus/v2.8.0`.
3. **Disposition table.** DUA5 has no new Deno source patch dispositions; it
   publishes and consumes the DUA2-DUA4 source decisions.
4. **Implementation evidence.** Done. Deno tag/release/default-branch update
   and Nimbus Cargo repin evidence are recorded below.
5. **Focused verification.** Done. Deno fork check, Nimbus runtime check,
   provenance verifier, and upstream-policy verifier passed.
6. **Broad verification.** Pending. DUA6 owns the broad Node compatibility
   rebaseline after Nimbus consumes immutable tags.
7. **Residual risks.** The hardened `rusty_v8` release workflow path will be
   consumed by a future `v149.2.0-nimbus.2`; it is not a DUA5 blocker after the
   2026-06-01 maintainer decision.

## Input Baseline

| Field | Value |
| --- | --- |
| Current Deno fork branch | `nimbus/v2.8.1` |
| Current Deno fork head | `18f76a9a19ab74d49d9a40037733cc4aec983d26` |
| Prior `nimbus/deno` default branch | `nimbus/v2.8.0` |
| Current `nimbus/deno` default branch | `nimbus/v2.8.1` |
| Required `nimbus/deno` default branch after release | `nimbus/v2.8.1` |
| Accepted `rusty_v8` tag in Deno fork | `v149.2.0-nimbus.1` |
| Accepted `rusty_v8` commit | `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` |
| Required `rusty_v8` release assets | 16 assets present on the GitHub release |
| Current blocker | none |

Current GitHub default branch evidence:

```console
gh repo view nimbus/deno --json defaultBranchRef,nameWithOwner,url
```

Observed:

```json
{"defaultBranchRef":{"name":"nimbus/v2.8.0"},"nameWithOwner":"nimbus/deno","url":"https://github.com/nimbus/deno"}
```

## Disposition Table

| Release step | Disposition | Required proof before marking done |
| --- | --- | --- |
| Publish accepted `rusty_v8` tag | done | Tag `v149.2.0-nimbus.1` exists at `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d`; GitHub release has all 16 required assets. |
| Verify Deno fork against accepted `rusty_v8` tag | done | `.cargo/config.toml`, `Cargo.toml`, and `Cargo.lock` use `v149.2.0-nimbus.1`; focused Deno fork check passed. |
| Publish Deno fork tag | done | Tag `v2.8.1-nimbus.1` was pushed from `18f76a9a19ab74d49d9a40037733cc4aec983d26`. |
| Publish `nimbus/deno` GitHub release | done | GitHub release exists for `v2.8.1-nimbus.1` and records upstream base, `rusty_v8` tag, and verification. |
| Update `nimbus/deno` default branch | done | GitHub default branch is `nimbus/v2.8.1` after release publication. |
| Repin Nimbus | done | Nimbus `Cargo.toml` and `Cargo.lock` use immutable `nimbus/deno` and matching `nimbus/rusty_v8` tags and SHAs; no local path override remains. |

## Implementation Evidence

`nimbus/rusty_v8` release substrate:

```console
gh release view v149.2.0-nimbus.1 --repo nimbus/rusty_v8 --json tagName,targetCommitish,isDraft,isPrerelease,assets,url
```

Observed: release `v149.2.0-nimbus.1` is not draft, not prerelease, resolves to
`ce6663111a3ff8fde06bc04ba19bbbced60dbc8d`, and has the 16 required release
assets.

Deno fork release:

```console
git push origin v2.8.1-nimbus.1
gh release create v2.8.1-nimbus.1 --repo nimbus/deno --title v2.8.1-nimbus.1 --notes "... upstream denoland/deno v2.8.1; ... rusty_v8 v149.2.0-nimbus.1 ..."
gh release view v2.8.1-nimbus.1 --repo nimbus/deno --json tagName,targetCommitish,isDraft,isPrerelease,url,createdAt,publishedAt
```

Observed:

```json
{"createdAt":"2026-06-01T17:38:50Z","isDraft":false,"isPrerelease":false,"publishedAt":"2026-06-01T17:39:08Z","tagName":"v2.8.1-nimbus.1","targetCommitish":"nimbus/v2.8.1","url":"https://github.com/nimbus/deno/releases/tag/v2.8.1-nimbus.1"}
```

Default branch:

```console
gh repo edit nimbus/deno --default-branch nimbus/v2.8.1
gh repo view nimbus/deno --json defaultBranchRef,nameWithOwner,url
```

Observed:

```json
{"defaultBranchRef":{"name":"nimbus/v2.8.1"},"nameWithOwner":"nimbus/deno","url":"https://github.com/nimbus/deno"}
```

Nimbus repin:

```console
rg -n 'v2\.8\.1-nimbus|v149\.2\.0-nimbus|github.com/nimbus/deno|github.com/nimbus/rusty_v8' Cargo.toml Cargo.lock
```

Observed: `Cargo.toml` patch entries use `v2.8.1-nimbus.1` for the
Deno-family crates and `v149.2.0-nimbus.1` for `v8`; `Cargo.lock` resolves
Deno-family crates to
`git+https://github.com/nimbus/deno?tag=v2.8.1-nimbus.1#18f76a9a19ab74d49d9a40037733cc4aec983d26`
and `v8` to
`git+https://github.com/nimbus/rusty_v8?tag=v149.2.0-nimbus.1#ce6663111a3ff8fde06bc04ba19bbbced60dbc8d`.

## Focused Verification

Commands:

```console
/usr/bin/env CARGO_ENCODED_RUSTFLAGS= cargo check -p deno_core -p deno_node -p deno_node_crypto -p deno_fetch --locked
cargo check -p nimbus-runtime --lib
bash scripts/verify-deno-fork-provenance.sh
bash scripts/verify-deno-fork-upstream-policy.sh
```

Observed:

- Deno fork focused check passed: `Finished dev profile [unoptimized + debuginfo] target(s) in 1.25s`.
- Nimbus runtime check passed: `Finished dev profile [unoptimized + debuginfo] target(s) in 45.84s`.
- `bash scripts/verify-deno-fork-provenance.sh`: `5 passed, 0 failed`.
- `bash scripts/verify-deno-fork-upstream-policy.sh`: `27 passed, 0 failed`.

## Broad Verification

DUA5 does not own broad Node compatibility counts. DUA6 runs the broad
rebaseline after Nimbus repins to immutable published tags.

## Residual Risks

- `v149.2.0-nimbus.1` was published before the later release-workflow
  hardening commits, but the release now has all 16 required assets and is the
  maintainer-approved DUA5 substrate.
- The later `rusty_v8` release-workflow hardening path should be used for
  `v149.2.0-nimbus.2` or later.
- DUA5 does not prove broad Node fixture movement; DUA6 owns the rebaseline.
