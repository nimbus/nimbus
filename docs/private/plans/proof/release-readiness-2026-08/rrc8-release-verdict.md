# RRC8 Release Verdict

Date: 2026-08-29

Result: **NO-GO** for v0.1.46. The repaired source and provisional candidate
passed every locally available product, adapter, storage, workload, desktop,
and distribution lane. However, the release candidate is not reproducible
from a clean committed Nimbus tree because the required Deno fork revision is
local-only. Required publication-owned package and notarization evidence also
remains absent. No tag, push, release, package publication, or credential
change occurred.

## Candidate Identity

- Nimbus committed branch head:
  `ae63f18798b2d020c029a4d65443c45c0acf347f`.
- Nimbus upstream baseline:
  `b57a2d680891de852d5576e65ccaea787b005431`.
- Desktop head: `bbc103f84b2a88e2baa4b522e45447bed31e04c7`.
- Required Deno integration head:
  `1c17e86b296af380f67c48f3b9a89876db154604` on the local
  `codex/release-readiness-websocket-egress` branch.
- Preserved provisional macOS binary:
  `/private/tmp/nimbus-release-candidate-875c1dc65b4d/nimbus`, SHA-256
  `875c1dc65b4dec6a72fda5518628b0c417bb9c3416bf0ed7ab93f6c57cf0df0f`.

The Nimbus connection-broker integration remains an uncommitted patch because
the dependency must use an immutable revision that a clean build can fetch.
The provisional binary identifies as 0.1.45 and is not the exact v0.1.46
release artifact.

## Critical Repair Evidence

The final connection-broker repair closes the live WebSocket authorization
gap and DNS-rebinding gap for both fetch and WebSocket connections. The Deno
fork now authorizes the URL before resolution, authorizes every concrete
resolved address before connect, binds WebSocket authorization to its target,
rejects custom clients and proxy paths when it cannot prove the policy, and
does not cache checker-bearing clients. Nimbus maps those resolved addresses
through the same tenant gateway. Runtime tests prove that loopback and link-
local resolutions are denied before the listener accepts a socket.

The complete local-Deno integration passed these focused checks:

- fetch and WebSocket resolved-address denial: 2 of 2;
- fetch and WebSocket allowed paths: 2 of 2;
- bridge internal-address denial for fetch and WebSocket: 2 of 2;
- `cargo check -p nimbus-runtime -p nimbus-bridge`;
- Deno fetch checker, resolver, proxy, and WebSocket tests;
- the connection-broker verifier: 14 of 14 conditions.

The final patch also rejects supervisor-proxy policies at all non-proxy
backend start and reload seams, adds exhaustive WebSocket protocol handling,
keeps runtime gateway authorization free of terminal audit or quota side
effects, and records the vendored `lazy_static` lint repair accurately.

## Documentation Gates

- `bash scripts/check-docs.sh`: 109 pages checked with no error.
- `bash scripts/verify-nimbus-docs-site.sh`: 17 of 17 checks passed.
- The current-capabilities contract, private proof files, and release matrix
  distinguish provisional evidence from exact-candidate evidence.

## Security and Dependency Gates

- `make deny`: passed.
- Third-party attribution gate: one crate and eight vendored patches passed all
  legal and provenance checks.
- Third-party attribution helper: 11 of 11 passed.
- Nimbus and desktop clean-install npm audits reported zero vulnerabilities.
- Repeated secret scans in the structured reviews were clean.

## Independent Reviews

The final uncommitted Nimbus integration received two independent structured
reviews after the last accepted correction:

- Claude Opus 5 at high reasoning: clean, with no accepted or actionable P0
  through P3 finding.
- GPT-5.6 Sol at xhigh reasoning: clean, with no accepted or actionable P0
  through P3 finding.

The reviewers checked the DNS-rebinding closure, fail-closed proxy and custom-
client behavior, gateway purity contract, supervisor-proxy rejection seams,
WebSocket protocol exhaustiveness, the one-line lockfile delta, vendored-code
provenance, and verification-script honesty. The Deno fork received its own
Opus review loop and closed clean after all P3 hardening findings were fixed.

## Remaining Release Blockers

1. The Deno revision
   `1c17e86b296af380f67c48f3b9a89876db154604` is not reachable from an
   immutable remote reference. This plan does not authorize a push or tag.
2. Therefore, Nimbus cannot commit the dependency update, produce a clean
   exact v0.1.46 candidate, or run `make ci` and all smoke lanes against that
   exact candidate.
3. Apple notarization and stapling remain unverified because the authorized
   API credentials are unavailable in this local release lane.
4. Public apt and COPR install proofs remain owned by the distribution plan and
   require separate publication authority.
5. Exact v0.1.46 archives and OCI artifacts do not exist until the exact
   candidate can be built.

## Matrix Decision

The fixed 46-condition matrix ends with 3 passes, 43 blocked conditions, zero
unverified conditions, zero failures, and zero structural errors. Documentation,
dependency security, and independent review are direct passes. Every product or
artifact condition stays blocked because its evidence is provisional or belongs
to an authorized publication lane. A skip or provisional result is not green.

## Cleanup

The exact-test worktree and 13 GiB rebuildable Nimbus Cargo target were removed
after both reviews. Free macOS filesystem space increased from 86 GiB to 100
GiB. The 663 MiB candidate artifact, source changes, proof files, and desktop
release artifacts were preserved.

## Required Next Action

Publish the final Deno revision through an authorized immutable remote ref.
Then update and commit the Nimbus dependency, build the exact v0.1.46 candidate
from a clean tree, run `make ci`, repeat all critical macOS, Linux, desktop,
archive, and OCI lanes against that candidate, and complete the authorized
notarization, apt, and COPR proofs. The release can become GO only after the
matrix reports 46 passes.
