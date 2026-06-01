# DUA5 Deno Release And Nimbus Repin

status: pending
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
source worktree: /Users/jack/src/github.com/nimbus/deno
source branch: nimbus/v2.8.1
pr: https://github.com/nimbus/nimbus/pull/11
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA5 is pending. Release and Nimbus repin work is
   waiting for the hardened `rusty_v8` branch CI run `26769536048`.
2. **Input baseline.** DUA4 source commit is
   `18f76a9a19ab74d49d9a40037733cc4aec983d26`; `nimbus/deno` default branch is
   currently `nimbus/v2.8.0`.
3. **Disposition table.** DUA5 has no new Deno source patch dispositions yet;
   it publishes and consumes the DUA2-DUA4 source decisions.
4. **Implementation evidence.** Pending. This file records the release
   checklist so compaction cannot lose it.
5. **Focused verification.** Pending. Must rerun the focused Deno fork check
   after repinning from diagnostic `rusty_v8` `.1` to hardened `.2+`.
6. **Broad verification.** Pending. DUA6 owns the broad Node compatibility
   rebaseline after Nimbus consumes immutable tags.
7. **Residual risks.** DUA5 must not publish or repin from the diagnostic
   `v149.2.0-nimbus.1` tag.

## Input Baseline

| Field | Value |
| --- | --- |
| Current Deno fork branch | `nimbus/v2.8.1` |
| Current Deno fork head | `18f76a9a19ab74d49d9a40037733cc4aec983d26` |
| Current `nimbus/deno` default branch | `nimbus/v2.8.0` |
| Required `nimbus/deno` default branch after release | `nimbus/v2.8.1` |
| Current diagnostic `rusty_v8` tag in Deno fork | `v149.2.0-nimbus.1` |
| Required hardened `rusty_v8` tag | `v149.2.0-nimbus.2` or later |
| Current blocker | `nimbus/rusty_v8` branch run `26769536048` still in progress. |

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
| Publish hardened `rusty_v8` tag | pending | Corrected branch CI passes; tag `v149.2.0-nimbus.2` or later is pushed from the hardened workflow commit; release assets are complete. |
| Repin Deno fork to hardened `rusty_v8` tag | pending | `.cargo/config.toml`, `Cargo.toml`, and `Cargo.lock` use the hardened tag; focused Deno fork check passes. |
| Publish Deno fork tag | pending | Tag `v2.8.1-nimbus.1` or later is pushed from the verified Deno fork commit. |
| Publish `nimbus/deno` GitHub release | pending | GitHub release exists for the Deno tag and records upstream base, `rusty_v8` tag, and verification. |
| Update `nimbus/deno` default branch | pending | GitHub default branch is changed from `nimbus/v2.8.0` to `nimbus/v2.8.1` after the Deno release is published. |
| Repin Nimbus | pending | Nimbus `Cargo.toml` and `Cargo.lock` use immutable `nimbus/deno` and matching `nimbus/rusty_v8` tags and SHAs; no local path override remains. |

## Implementation Evidence

Pending. DUA5 will record:

- `gh run view 26769536048 --repo nimbus/rusty_v8 --json status,conclusion,jobs`
- `git tag` and `git push` evidence for `nimbus/rusty_v8`
- Deno fork repin diff and lockfile evidence
- Deno focused fork check output
- `git tag` and `git push` evidence for `nimbus/deno`
- `gh release create` or equivalent GitHub release evidence for `v2.8.1-nimbus.*`
- `gh repo edit nimbus/deno --default-branch nimbus/v2.8.1` evidence
- Nimbus `Cargo.toml` and `Cargo.lock` repin evidence

## Focused Verification

Pending. Required before DUA5 can move to done:

```console
/usr/bin/env CARGO_ENCODED_RUSTFLAGS= cargo check -p deno_core -p deno_node -p deno_node_crypto -p deno_fetch --locked
bash scripts/verify-deno-fork-provenance.sh
bash scripts/verify-deno-fork-upstream-policy.sh
```

The focused Deno fork check must run after the Deno fork consumes the hardened
`rusty_v8` tag, not against diagnostic `.1`.

## Broad Verification

DUA5 does not own broad Node compatibility counts. DUA6 runs the broad
rebaseline after Nimbus repins to immutable published tags.

## Residual Risks

- The corrected `rusty_v8` branch CI is still running; release work must wait.
- The `nimbus/deno` default branch still points at `nimbus/v2.8.0`; this is
  acceptable only until the `v2.8.1-nimbus.*` release exists.
- DUA5 must not repin Nimbus from local paths or diagnostic tags.
