# RRC8 Release Verdict

Date: 2026-09-04

## Current Status Update

Result: **NO-GO**.

RRC8 tracks upstream Deno 2.9.6 and V8 150.4. Public fork releases
`v2.9.6-nimbus.4` and `v150.4.0-nimbus.1` are non-draft and non-prerelease.
Their annotated tags peel to reviewed commits
`ded7d15771894d157b6369b8193d6e5bd055ce9e` and
`961a76d0cee88efdecfa9224c519fd153c404b51`. Their branch and tag workflows
and local release gates pass. A fresh rusty_v8 download verifies all 44
payloads and 44 checksum sidecars.

The current committed Nimbus product-code candidate is
`eed137ed7`. Its normal Cargo graph resolves
41 Deno packages from `v2.9.6-nimbus.4` and rusty_v8 from
`v150.4.0-nimbus.1`. It pins the Bun/JSC adapter to immutable tag
`bun-v1.4.0-nimbus.8` at
`38531f191dd11149d07bcc9fb0c5c7e2b40c89ba`. This candidate also contains the release test-toolchain,
runtime admission, local Node grant, and target-specific dgram corrections.
The earlier full `make ci` replay applies to the superseded
`a5869adbbf36278f4a9b2bd193a8a399f91e38fc` graph.

RRC8 has not completed full local and hosted proof for the exact current
candidate. The result remains **NO-GO**.

Bun run `33926366044` passed the complete adapter build, package, and
verification lane on macOS arm64 and Linux x86_64. Annotated tag object
`5c9fc02b723cce0efd2673efe64e1cf9a62ce499` peels to the pinned commit.
However, the fork-standardization gate now detects upstream `bun-v1.4.1`.
Tag `bun-v1.4.0-nimbus.8` is an exact tested checkpoint, not the final
tracks-latest release candidate.

The active Deno branch now ends at `853f81792a`. Local Deno stream tests and
the exact Nimbus Node 22 and Node 24 WHATWG batches pass 33 of 33 and 34 of 34
fixtures. Hosted fork run `33933279302` is in progress. Nimbus has not yet
published or pinned a Deno tag containing this correction.

A source-free Linux arm64 package from the previous branch head passed health
and application deployment. Its first WebStandard function invocation then
panicked when runtime construction opened a Deno build source that the archive
did not contain. The new Deno tag provides a construction-only source
interface.
Nimbus now embeds the required WebStandard and Node build-only source union in
both companion blobs. It validates every active descriptor, uses the packaged
source for ordinary and service-snapshot construction, and fails closed on a
provider miss.

Focused feature-off and pointer-compressed tests and the final Sol xhigh review
pass. An OS-sandboxed macOS release package passed all four `nimbus/agent-chat`
runtime anchors while every Deno source checkout was unreadable. A production-
configuration integration test also invoked a Node22 service-bearing runtime
under the same denial policy.

The exact Linux arm64 release artifact at `cb84dfec8` has SHA-256
`d8e670b289a6cf6ae092b3fdac2d69cc02ca2f68502706a3ac5e133124f3d0e7`.
A fresh Ubuntu 24.04 container received only its read-only archive directory.
Health, deployment, the four application anchors, post-smoke health, and
graceful `SIGTERM` exit status 0 passed. No runtime panic or missing-source
error appears in the log. This result closes the source-free runtime packaging
blocker.

Final-head shard run `33514162769` then found that the redb PITR wall-clock
test had no nextest isolation. K=1 and K=2 exceeded the unchanged 1 second
limit. K=3 and K=4 passed.

The repair reserves all test threads for this test
and does not weaken its budget. Ten focused runs and the full CI-profile
storage suite pass. Sol xhigh accepted the isolation and rejected a temporary
budget increase. The correction removes that increase.

The exact replay found and closed two dependency-graph defects.

Wasmtime 46.0.2 had two fixed security advisories. Nimbus now consumes 46.0.3.
The workspace V8 asset override and digest manifest still named the prior
149.4 release. Both files now bind the 150.4 release and its exact published
asset digests.

Nimbus commit `d6636b980c6f638bf9e9cd1fe75c437fbd51bd37` records the runtime
cleanup and public-tag repin. Four Sol-only review rounds then reported 33
items. The audit accepted and fixed 21. It refuted 12 after source and test
verification. A non-TTY workspace replay also found an oversized nested
retirement future that caused two default-stack server tests to abort. The
final candidate boxes that future at the retained-supervisor boundary. Both
regressions and all 39 resource-retirement tests pass.

The first exact Linux arm64 release smoke at `979d2687d` found a release
defect. `SIGTERM` ended `nimbus start` with status 143 and bypassed graceful
cleanup. The repair now routes process termination through the existing
server shutdown channel. Signal supervision remains active through discovery,
scheduler, engine, and network cleanup. A second signal cancels a stalled
drain.

TLS also honors shutdown requested before its accept loop subscribes.
RRC8 boxes the large server lifecycle at the supervision boundary. This
prevents an overflow on the standard main-thread stack.

The repaired code passes 765 of 765 server tests with 35 declared skips. It
also passes 1,076 of 1,076 CLI and launcher tests with 4 declared skips. The real
child-process `SIGTERM`, pre-requested TLS shutdown, shutdown-handle, and
two-signal escalation regressions pass. Formatting, whitespace, and
warning-denied Clippy pass.

A final Sol xhigh review reports no accepted or
actionable P0 through P3 finding. TruffleHog is clean. No Opus 5 or Fable
review ran.

One server replay observed `admin_whatsmyuri_over_wire` pass its assertions and
then receive `SIGSEGV` during process teardown. The exact test passed 10 of 10
isolated runs. The complete 23-test MongoDB binary passed five consecutive
runs, and the final 765-test server replay was clean. This is an unconfirmed
load-only runner observation, not an accepted product defect.

The current Linux VMM tuple uses crun `v1.29.1-nimbus.2`, libkrun
`v1.19.4-nimbus.3`, and libkrunfw 5.5.0. The exact `c565e89fd` validation
bundle passed LH1 through LH6 on `minicloud.local`. Its copied OCI rootfs kept
the fixture ownership `1234:2345`. Exact cleanup removed the runtime process,
private state, port, Buildah container, and fixture image. It preserved the
unrelated LibSQL container.

The VMM review chain closed root-host installation, rootfs ownership, and
source-identity failures. The optional source record now fails closed when Git
cannot read the checkout and requires the supplied path to be the worktree
root. Live checks accepted a valid Git root and rejected a nested source path.
Commit `907a3d050` passes the focused helper, Bash syntax, ShellCheck,
formatting, whitespace, and the final Sol xhigh review. No Opus 5 or Fable
review ran.

Hosted Bun run `33568361939` proved the corrected full-GC memory oracle on
macOS. It measured 5,334,961 bytes of retained live-heap growth and a
5,243,789-byte drop after release. The next probe then found stale test logic:
it expected errors from generated binding maps that the current generated
program wrapper no longer uses.

Bun commit `1322dc50d7718dcf8ad6adc379921c0659e09886` now awaits the
generated Node builtin and external-package helper functions and requires both
imports to fail through the resolver policy. The probe prints each policy
result before it checks the verdict. Formatting, whitespace, TruffleHog, and
the Sol xhigh review pass. Exact replacement run `33572160639` passed every
native probe on macOS and Linux, including these generated helpers. The macOS
probe measured 5,332,760 bytes of retained live-heap growth and a 5,243,342-byte
drop after release. The Linux probe measured 5,296,672 bytes of retained
live-heap growth and a 5,203,260-byte drop after release.

The next Nimbus test in that run failed before it reached Bun. The process-init
proof used a tenant-affine invocation without a tenant label. The blocking
router then replaced the specific locality error with `runtime executor
unexpectedly closed`.

Nimbus commit `248ab891c` preserves the dispatch error and rolls back failed
dispatch accounting. The process-init proof now uses four valid, non-affine
one-worker executors with a shared host rendezvous. Two focused router tests and
48 executor tests pass. Formatting, Clippy with warnings denied, whitespace,
TruffleHog, and the exact Sol xhigh review also pass. No Opus 5 or Fable review
ran.

Corrected run `33576740616` passed the process-global concurrent-init proof on
Linux. Its eight linked-adapter integration tests then failed on both platforms
before dispatch because their tenant-affine runtimes still used the tenantless
convenience method. The native Bun probes had already passed.

Nimbus commit `3dba5ebb6` gives every linked-adapter integration invocation one
stable tenant owner lease and uses the production tenant-affine entry point.
Forced-cfg check and Clippy with warnings denied type-check the gated test.
Formatting, whitespace, TruffleHog, and the exact Sol xhigh review pass. No
Opus 5 or Fable review ran. GitHub started replacement run `33580557154` at the
exact Nimbus and Bun revisions.

Run `33580557154` passed every native probe, the process-global concurrent-init
proof, and seven of eight linked-adapter tests on both platforms. The remaining
test expected a guest JSON response after its host callback cancelled the
invocation. The Bun cancellation watcher intentionally turns any cancellation
transition into terminal adapter status 314. Nimbus therefore returned the
exact top-level `Cancelled` result. Guest code must not swallow platform
cancellation.

Nimbus commit `3c6436ee2` corrects the test to prove that terminal contract and
retains the assertion that exactly one host call occurred. Forced-cfg check and
Clippy with warnings denied, formatting, whitespace, TruffleHog, and the exact
Sol xhigh review pass. No Opus 5 or Fable review ran. Replacement run
`33583722755` uses that Nimbus commit and Bun commit `1322dc50d7` on macOS and
Linux.

Run `33583722755` passed the corrected linked cancellation test and reached the
package manifest gate on both platforms. The manifest correctly recorded Bun
commit `1322dc50d7`, but Nimbus still declared the earlier reviewed commit
`40d63a6879`. The fail-closed package check rejected both archives.

Nimbus commit `b0737a784` pins the reviewed Bun commit
`1322dc50d7718dcf8ad6adc379921c0659e09886` across the workflow, runtime
contract, installer, verifier, and tests. The seven-part runtime contract, 71
Rust and UI tests, 63 installer-helper tests, action lint, formatting,
whitespace, TruffleHog, and the exact Sol xhigh review pass. No Opus 5 or Fable
review ran. Replacement run `33589420091` passed on macOS arm64 and Linux
x86_64 with publishing disabled.

The downloaded macOS archive has SHA-256
`3b11cee4898b787eeada3db72daad1a07915bd136f55ad390b3b5bf23d600daf`.
The downloaded Linux archive has SHA-256
`0f8cd653f332af25e22589ed77aa46f52c606f09b8254348b5a7889c5d899de4`.
Both values match their hosted summaries. The macOS archive passed an
independent local package audit. The Linux archive passed the same manifest,
checksum, SBOM/provenance, export, and native-symbol audit on
`minicloud.local`.

Annotated tag `bun-v1.4.0-nimbus.6` and branch `nimbus/bun-v1.4.0` resolve to
the reviewed Bun commit. The fork default branch and canonical clone use that
release branch. Nimbus commit `a5869adbb` repins every active consumer to the
immutable tag and adds an assertion for the exact revision in the UI fixture.
Its focused fork, runtime, package, release-asset, installer, format, and
whitespace gates pass. The exact Sol xhigh review reports no actionable P0
through P3 finding, and TruffleHog is clean. GitHub has no Bun Release or new
Nimbus product release from this work.

The desktop repository now has all seven required Apple signing and
notarization secret names. Its prior manual workflow accepted an unused tag
input and did not explicitly prevent publication. It also ignored the
rotation-owned signing-identity secret and hard-coded the current certificate
owner. Desktop commits `69a6f10`, `b8cbaaf`, and `2d35ae4` make manual dispatch
use publish mode `never` while it signs, notarizes, staples, and uploads
workflow artifacts. They validate and use the complete configured Developer ID
identity. Action lint, all 186 unit tests, lint, typecheck, TruffleHog, and the
Sol xhigh follow-up pass.

Desktop run `33592790284` proved the non-publishing guard and passed Windows
packaging. Its Linux job generated AppImage and deb artifacts, then failed RPM
because electron-builder used the spaced visible product name as the package
name. The same log reported a missing stable desktop identity. Run
`33593241210` then rejected the first repair because Electron Builder does not
permit `packageName` at the Linux configuration root.

Desktop commits `b23223f`, `b22f572`, and `8dc9eaa` move the package identity
to the deb and RPM targets. They keep `nimbus-desktop` as the stable desktop
name and add package author metadata. They also update first-party actions,
pin third-party actions, and correct the release runbook. Actionlint and lint
across 40 files pass. Typecheck, all 186 tests, and a local Linux package
schema check pass. Strict prose lint, whitespace, TruffleHog, and the exact Sol
xhigh review also pass.

Non-publishing Desktop run `33593752690` passed at exact commit
`8dc9eaa7b858e50f40751b51526e585a87953b83` on macOS 14, Windows 2022, and
Ubuntu 24.04. The macOS lane signed, notarized, stapled, and validated the
application. Windows built x64, arm64, and universal installers. Linux built
AppImage, deb, and RPM packages. Every platform passed its fuse and size
audit, and all workflow artifacts uploaded. The logged publish mode is
`never`. The repository still has only its pre-existing `v0.1.0` release.

The first local `make ci` at `a5869adbb` passed 510 runtime tests and declared
94 runtime ignores. The workspace lane passed 7,730 tests and declared 111
skips. One CLI test failed because process scheduling consumed its one-second
test deadline. One sandbox test passed but received a Nextest leak marker.

The first repair gave every test a five-second leak timeout. Sol rejected that
change because Nextest starts this timer after the test process exits. The
global allowance could hide a short-lived descendant that retains a capture
handle. RRC8 accepted this P2 finding.

The next configuration made the default 100-millisecond leak result a hard
failure. Cargo-nextest 0.9.138 then failed 3 of 100 concurrent stress
iterations. Two reported tests start no child process. A separate subprocess
oracle ran the three sandbox tests 300 times and observed immediate pipe
closure after every process exit.

This behavior matches open Nextest issue 1469. Cargo-nextest 0.9.143 passed the
same 100-iteration focused stress case. A full package run still assigned a
leak failure to a 0.00-second CLI unit. A one-second macOS override then
assigned the same result to a server test that starts no child process.

Nimbus now requires cargo-nextest 0.9.143 in every local and hosted consumer.
Linux and Windows use a 100-millisecond hard leak failure. macOS uses the
documented platform override and fails after five seconds. The CLI wall-clock
test reserves the complete runner. Its one-second assertion remains unchanged.
The final full `make ci` replay remains required.

Runs `33594909729`, `33594912184`, `33594915367`, `33594917976`,
`33594920291`, `33594923052`, `33594925657`, and `33594928725` pass at
`a5869adbb`. They cover shard scaling, desktop UI, Windows, CodeQL, container
egress, KV, krun, and docs. Full CI, Bun, and Node compatibility remain in
progress.

Dual-target run `33594934854` passes all four Nimbus targets. Its four public
cloud targets fail closed because their URLs and credentials are absent. The
workflow ran with `NIMBUS_DUAL_TARGET_DRY_RUN=0` and required live proof. The
failures leave evidence debt rather than a product regression.

Exact application and desktop binding, smoke, archive, OCI, and Linux full-LTO
replay remain on the final branch head.

Public apt and COPR proofs still need verification. At the owner's direction,
current reviews use Sol only. No Opus 5 or Fable review ran during this uplift.

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
`ae63f18798b2d020c029a4d65443c45c0acf347f`, Desktop
`bbc103f84b2a88e2baa4b522e45447bed31e04c7`, Deno
`1c17e86b296af380f67c48f3b9a89876db154604`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

- `bash scripts/check-docs.sh`: 109 pages checked with no error.
- `bash scripts/verify-nimbus-docs-site.sh`: 17 of 17 checks passed.
- The current-capabilities contract, private proof files, and release matrix
  distinguish provisional evidence from exact-candidate evidence.

## Security and Dependency Gates

Candidate binding: Nimbus
`ae63f18798b2d020c029a4d65443c45c0acf347f`, Desktop
`bbc103f84b2a88e2baa4b522e45447bed31e04c7`, Deno
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

1. The corrected full local and hosted CI gates have not passed on the final
   branch head.
2. Release-critical Linux x86_64, archive, OCI, and remaining application and
   distribution lanes have not replayed from the final branch head. The
   source-free macOS and Linux arm64 runtime package oracles pass.
3. Live dual-target cloud proofs remain blocked because four provider URLs and
   their credentials are not configured in the repository secrets.
4. Public apt and COPR install proofs remain owned by the distribution plan and
   require separate publication authority.
5. Upstream Bun 1.4.1 supersedes the verified 1.4.0 fork checkpoint. The Bun
   carries need an uplift, dual-host proof, immutable tag, and Nimbus repin.

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

1. Finish hosted Deno run `33933279302` and close the remaining supported Node
   compatibility failures before a new immutable Deno fork tag.
2. Uplift the retained Bun carries to the current upstream release. The
   required version is 1.4.1. Repeat both host proofs before a new immutable
   Bun tag.
3. Repin Nimbus to both exact fork releases. Build the resulting v0.1.46
   candidate on a Linux runner with enough memory for full LTO.
4. Repeat all critical macOS, Linux, application, desktop, archive, and OCI
   lanes on that clean candidate.
5. Complete the authorized notarization, apt, and COPR proofs.

The release can become GO only after the matrix reports 46 passes.
