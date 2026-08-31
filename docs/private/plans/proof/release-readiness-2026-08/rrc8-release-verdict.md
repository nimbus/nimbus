# RRC8 Release Verdict

Date: 2026-08-31

## Current Status Update

Result: **NO-GO**.

RRC8 tracks upstream Deno 2.9.6 and V8 150.4. Public fork releases
`v2.9.6-nimbus.1` and `v150.4.0-nimbus.1` are non-draft and non-prerelease.
Their annotated tags peel to reviewed commits
`6c37e683a3199e873a9ce93f4c7ee4f58ab9b6a3` and
`961a76d0cee88efdecfa9224c519fd153c404b51`. Their branch and tag workflows
pass. A fresh rusty_v8 download verifies all 44 payloads and 44 checksum
sidecars.

The normal Nimbus Cargo graph now resolves 41 Deno packages and rusty_v8 from
those immutable tags. Fork provenance, upstream policy, standardization, the
all-target runtime check, and the canonical runtime lane pass. Full `make ci`
also passes without a local V8 override. It includes 499 canonical runtime
tests, 7,722 workspace tests, 846 UI tests, the required harness, and the
release proof helpers.

The exact replay found and closed two dependency-graph defects.

Wasmtime 46.0.2 had two fixed security advisories. Nimbus now consumes 46.0.3.
The workspace V8 asset override and digest manifest still named the prior
149.4 release. Both files now bind the 150.4 release and its exact published
asset digests.

The paired Nimbus runtime cleanup and repin are not yet committed. Exact
release-critical smoke, archive, OCI, and higher-memory Linux full-LTO replay
therefore remain. Apple notarization and public apt and COPR proofs also remain
pending. At the owner's direction, current reviews use Sol only. No Opus 5 or
Fable review ran during this uplift.

The sections below preserve the earlier v2.9.3 checkpoint evidence. The U3
uplift proof is the current source for Deno 2.9.6 and V8 150.4 identity and
local verification.

## Prior Unpublished-Graph QA Replay

Candidate binding: Nimbus baseline
`e0cbb5937d5390d44a597b6ef45ed7003e267a03` plus the local source bundle,
Deno `8d48dc4a68df8e083ed4b17855440b1df6405620`, whose final commit contains
correction SHA-256
`6bcaa1948d86ef65af2a7ccb65f4a1d21ee7687fbd98d9730e35ab6de1d57b55`,
and runtime-equivalent rusty_v8 candidate
`961a76d0cee88efdecfa9224c519fd153c404b51`.

- The macOS candidate binary has SHA-256
  `6be740f7bc43ffd4dd1256b674b20aab2d5198ed6b7633fd28ceb58d90e11b2b`.
  Fresh-root smoke passes health, local-admin rejection, tenant creation, and
  schema creation. Indexes, CRUD, query pagination, WebSocket delivery, and the
  scheduler pass. Diagnostics, graceful shutdown, restart durability, and
  deletion also pass. The server log has SHA-256
  `ff9b67bfd8e5c535dd866a6fab1b90356e0bbbc15d40050263f089574085dddc`.
- The embedded UI Chromium smoke passes its 10-step product walk. The exact
  application lane passes all 9 applications and 37 anchors in 46.683 seconds.
  Its report has SHA-256
  `3714b49a8c37f0306ebfdc5ff655910341631d19fcf733a3a6298bf3cf583f53`.
- Desktop static checks pass 40 linted files, 17 test files, and 186 tests. All
  5 packaged Electron and Playwright tests pass. The signed universal binary
  contains arm64 and x86_64 slices and has SHA-256
  `59f9a458b2777a5149773ac751c1c87360994364c0ac136c18b27981fc326473`.
- The isolated Debian 13.4 x86_64 debug-profile build on `minicloud.local`
  produces Nimbus 0.1.46 with SHA-256
  `f49d5d59297835196cabaee728ff9a9112a43924b6547516728b6dfe4a5d3536`.
  Its fresh-root native smoke passes on unused ports. Its nine-application lane
  passes every application and anchor in 126.054 seconds. The Linux report has
  SHA-256
  `bce219cd315f119556465e9f810ed96b0d7fc74f52555c8e914efa1b74314b57`.
- The corrected full `make ci` exits zero. Formatting, warning-denied workspace
  Clippy, dependency policy, and production snapshot provenance pass. The gate
  passes 498 focused runtime tests and 7,722 workspace tests. It declares 94
  focused runtime ignores and 111 workspace skips. Doctests, required liveness
  campaigns, and protocol campaigns pass. JavaScript builds, typechecks, 846 UI
  tests, proof helpers, and 60 installer checks also pass.
- A Sol xhigh review required U3 to restore a current-contract runtime
  completion entry point. The new compact gate runs 24 execution-classification
  checks and 19 tenant-isolation checks. It also runs 32 tenant-autoscaling
  checks and 8 crossover trace tests. It includes a rejected-realm-symbol check.
- The gate validates the saved Node and Web benchmark traces. It checks actual
  construction mode and increasing measurement series. It also requires one
  shared run identity across both artifacts. Its focused replay passes. No Opus
  5 or Fable review ran.
- The final Sol xhigh follow-up reports no remaining trace-integrity defect.
  Its two findings repeat the known unpublished normal-graph blocker.
- The exact Linux release build used release optimization, full LTO, one
  code-generation unit, pointer compression, and one Cargo job. All
  dependencies completed. The final `rustc` process used only 11 minutes 29
  seconds of CPU time. Its wall time reached 9 hours 57 minutes. It held
  approximately 6.8 GiB resident memory while it waited on swapped LLVM pages.
- The build produced no release binary. Cleanup removed the temporary swap and
  restored the kernel setting. These measurements show that the 8 GiB host is
  too small for this release link. They do not show a product or compiler
  defect.

This is strong QA evidence for the unpublished graph. It is not immutable
release evidence: both fork references and the Nimbus source bundle still
contain uncommitted or unpublished state.

## Prior Checkpoint Candidate Identity

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
start and reload paths. It adds exhaustive WebSocket protocol handling. It
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

The earlier v2.9.3 integration received two independent structured reviews
after its last accepted correction:

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

1. The reviewed Nimbus runtime cleanup and immutable fork repin are not yet an
   exact committed candidate.
2. Release-critical macOS, Linux, application, desktop, archive, and OCI lanes
   have not yet replayed from that committed candidate.
3. The available 8 GiB Linux host cannot complete the exact full-LTO release
   link. The committed candidate needs a higher-memory Linux build runner.
4. Apple notarization and stapling remain unverified because the authorized
   API credentials are unavailable in this local release lane.
5. Public apt and COPR install proofs remain owned by the distribution plan and
   require separate publication authority.

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

1. Commit the reviewed Nimbus cleanup and repin to both immutable fork
   releases.
2. Build the exact v0.1.46 candidate from that commit. Use a Linux runner with
   enough memory for full LTO. Do not reuse the 8 GiB host for this lane.
3. Repeat all critical macOS, Linux, application, desktop,
   archive, and OCI lanes on that clean candidate.
4. Complete the authorized notarization, apt, and COPR proofs.

The release can become GO only after the matrix reports 46 passes.
