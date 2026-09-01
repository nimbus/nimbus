# Deno And rusty_v8 Fork Policy

This tracked policy is the CI-visible source of truth for Nimbus fork
maintenance. Detailed campaign evidence may live under ignored `docs/private/`,
but required verification must work in a fresh checkout without private files.

## Canonical repositories

- Nimbus consumer: `/Users/jack/src/github.com/nimbus/nimbus`
- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- rusty_v8 fork: `/Users/jack/src/github.com/nimbus/rusty_v8`
- Deno upstream: `https://github.com/denoland/deno`
- rusty_v8 upstream: `https://github.com/denoland/rusty_v8`

Do not use `/private/tmp` checkouts as campaign progress state. Create dedicated
clean worktrees beside the canonical repositories and preserve unrelated dirty
worktrees.

## Current release ledger

| State | Repository | Tag | Peeled commit | Meaning |
| --- | --- | --- | --- | --- |
| consumed | `nimbus/deno` | `v2.9.6-nimbus.2` | `625e4c259488dfa1c3c9d03fabde17758e1130d9` | Deno 2.9.6 with the selected Locker, egress, Node heap-policy, lazy-ESM, and packaged extension-source contracts |
| consumed | `nimbus/rusty_v8` | `v150.4.0-nimbus.1` | `961a76d0cee88efdecfa9224c519fd153c404b51` | V8 150.4 line declared by Deno 2.9.6, with the Nimbus Locker bridge |
| published, not consumed | `nimbus/rusty_v8` | `v150.2.0-nimbus.1` | `4786595e29679ee5ad9ba4925cdcd1cc83ab6448` | Forward-maintenance V8 150 line; awaits a compatible Deno V8 roll |

The consumed rows are derived and verified from `Cargo.toml` and `Cargo.lock`;
do not copy those values into verifier code. The published-but-unconsumed row is
deliberately separate so a new fork release cannot silently change Nimbus's V8
ABI.

## Upstream-first patch dispositions

Every upstream bump must classify each existing fork patch and each overlapping
upstream change:

- **Upstream Deno-family**: adopt upstream and delete the fork hunk.
- **Nimbus-only host integration**: retain the smallest hook or wrapper needed
  for Nimbus authority, lifecycle, or observability.
- **Temporary carry**: retain behavior not yet available upstream, with a
  focused regression and an explicit removal trigger.
- **Drop**: remove obsolete, patch-equivalent, experimental, historical, or
  false-green code.

Every retained patch records its owner, regression, and **Removal or upstream trigger**.
Prefer wrappers around upstream logic over copied implementations.

## Release sequence

1. Fetch upstream and fork remotes; census branches, tags, releases, workflows,
   default branches, local dirt, and exact upstream tag objects.
2. Audit upstream changes and disposition existing carries before editing.
3. Build a clean candidate branch from the exact upstream tag.
4. Temporarily unpin Nimbus to the candidate commit and prove the real consumer
   before creating an immutable tag.
5. Commit, tag, and push with explicit refspecs. Use `--no-follow-tags` when
   pushing branches/tags so unrelated upstream annotated tags do not publish.
6. Publish a non-draft, non-prerelease release and verify branch/tag workflows,
   release assets, checksums, and the new default branch.
7. Repin Nimbus to published tags, regenerate Cargo.lock, and run
   `scripts/verify-deno-fork-provenance.sh` plus
   `scripts/verify-deno-fork-upstream-policy.sh`.
8. Run isolate/cage/snapshot/termination/egress/teardown and affected Node
   compatibility evidence before the Nimbus PR is merged.

## Release Proof Checklist

- Candidate commit equals the peeled annotated tag and all recorded SHAs.
- Fork branch and tag CI are green at that exact commit.
- GitHub default branch names the new versioned Nimbus branch; old refs remain.
- Releases are public and their notes identify upstream base, retained/dropped
  carries, consumer impact, and verification.
- Required rusty_v8 assets and SHA-256 sidecars pass the exact manifest gate.
- Nimbus is repinned to published tags only; no local path or candidate revision
  remains.
- Deno's resolved `v8` crate version matches the consumed rusty_v8 tag line.
- Generated Node evidence is refreshed only when compatibility claims move;
  unchanged or unsupported outcomes remain explicit.
- Published-but-unconsumed fork lines remain separately ledgered.
