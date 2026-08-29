# RRC8 Release Verdict

Date: 2026-08-29

## Superseding Status Update

The owner later authorized updates to all Nimbus repositories. Deno PR #1
merged at `d0a6b9094e0da6acbb53ecd0d88ed6b81a142e63`, and annotated public fork
release `v2.9.3-nimbus.2` now provides the required immutable reference. Its
candidate, branch, and tag CI runs passed. Nimbus compiles and passes the
focused connection-broker gates against that tag.

The result remains **NO-GO**. The tracks-latest fork gate now reports upstream
Deno `v2.9.6`, while the checkpoint consumes `v2.9.3-nimbus.2`. Upstream
`v2.9.6` also introduces `deno_v8` 0.3.0 and moves the Rust V8 boundary from
149.4 to 150.4. The release candidate must use a reviewed and immutable
`v2.9.6` Nimbus fork release, then repeat the exact-candidate CI, host,
application, desktop, archive, and OCI proofs. Apple notarization and the
public apt and COPR proofs also remain pending. The sections below preserve the
initial NO-GO snapshot and its evidence.

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
fork now authorizes the URL before resolution and each concrete resolved
address before connect. It binds WebSocket authorization to its target. It
rejects custom clients and proxy paths when it cannot prove the policy. It
does not cache checker-bearing clients.

Nimbus maps those resolved addresses through the same tenant gateway. Runtime
tests show that the gateway denies loopback and link-local resolutions before
the listener accepts a socket.

The complete local-Deno integration passed these focused checks:

- fetch and WebSocket resolved-address denial: 2 of 2.
- fetch and WebSocket allowed paths: 2 of 2.
- bridge internal-address denial for fetch and WebSocket: 2 of 2.
- `cargo check -p nimbus-runtime -p nimbus-bridge`.
- Deno fetch checker, resolver, proxy, and WebSocket tests.
- the connection-broker verifier: 14 of 14 conditions.

The final patch rejects supervisor-proxy policies at all non-proxy backend
start and reload seams. It adds exhaustive WebSocket protocol handling. It
keeps runtime gateway authorization free of terminal audit and quota side
effects. It also records the vendored `lazy_static` lint repair accurately.

## Documentation Gates

Candidate binding: Nimbus
`ae63f18798b2d020c029a4d65443c45c0acf347f`, Deno
`1c17e86b296af380f67c48f3b9a89876db154604`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

- `bash scripts/check-docs.sh`: 109 pages checked with no error.
- `bash scripts/verify-nimbus-docs-site.sh`: 17 of 17 checks passed.
- The current-capabilities contract, private proof files, and release matrix
  distinguish provisional evidence from exact-candidate evidence.

## Security and Dependency Gates

Candidate binding: Nimbus
`ae63f18798b2d020c029a4d65443c45c0acf347f`, Deno
`1c17e86b296af380f67c48f3b9a89876db154604`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

- `make deny`: passed.
- Third-party attribution gate: one crate and eight vendored patches passed all
  legal and provenance checks.
- Third-party attribution helper: 11 of 11 passed.
- Nimbus and desktop clean-install npm audits reported zero vulnerabilities.
- Repeated secret scans in the structured reviews were clean.

## Independent Reviews

Candidate binding: Nimbus
`ae63f18798b2d020c029a4d65443c45c0acf347f`, Desktop
`bbc103f84b2a88e2baa4b522e45447bed31e04c7`, Deno
`1c17e86b296af380f67c48f3b9a89876db154604`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

The final uncommitted Nimbus integration received two independent structured
reviews after the last accepted correction:

- Claude Opus 5 at high reasoning: clean, with no accepted or actionable P0
  through P3 finding.
- GPT-5.6 Sol at xhigh reasoning: clean, with no accepted or actionable P0
  through P3 finding.

The reviewers checked DNS rebinding, fail-closed proxy paths, custom clients,
gateway purity, and supervisor-proxy rejection. They also checked WebSocket
protocol coverage, the one-line lockfile delta, vendored provenance, and
verification-script accuracy. The Deno fork had its own Opus review loop. It
closed clean after all P3 hardening fixes.

## Remaining Release Blockers

1. The Deno revision
   `1c17e86b296af380f67c48f3b9a89876db154604` is not reachable from an
   immutable remote reference. This plan does not authorize a push or tag.
2. Nimbus therefore cannot commit the dependency update or produce a clean
   v0.1.46 candidate. It also cannot run `make ci` or the exact smoke lanes.
3. Apple notarization and stapling remain unverified because the authorized
   API credentials are unavailable in this local release lane.
4. Public apt and COPR install proofs remain owned by the distribution plan and
   require separate publication authority.
5. Exact v0.1.46 archives and OCI artifacts are absent because the exact
   candidate build is absent.

## Matrix Decision

The fixed 46-condition matrix ends with 3 passes, 43 blocked conditions, zero
unverified conditions, zero failures, and zero structural errors. Documentation,
dependency security, and independent review are direct passes. Every product or
artifact condition stays blocked because its evidence is provisional or belongs
to an authorized publication lane. A skip or provisional result is not green.

## Cleanup

The cleanup removed the exact-test worktree and 13 GiB rebuildable Nimbus Cargo
target after both reviews. Free macOS filesystem space increased from 86 GiB
to 100 GiB. The cleanup preserved the 663 MiB candidate artifact, source
changes, proof files, and desktop release artifacts.

## Required Next Action

1. Publish the final Deno revision through an authorized immutable remote ref.
2. Update and commit the Nimbus dependency.
3. Build the exact v0.1.46 candidate from a clean tree.
4. Run `make ci` against that candidate.
5. Repeat all critical macOS, Linux, desktop, archive, and OCI lanes.
6. Complete the authorized notarization, apt, and COPR proofs.

The release can become GO only after the matrix reports 46 passes.
