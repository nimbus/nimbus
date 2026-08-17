# NNC5.2d Startup Quarantine Application

Status: `candidate complete; every pre-commit acceptance criterion green; exact item commit pending`

Owner: `NNC5.2d`

Starting commit: `ae29108f3bd2037557727e0036cf0f7ebfc039c0`

Starting tree: `528b4f10a2dd6c765d986dee4c292b7a63ba7455`

## Purpose

NNC5.2d replaces the filename-derived startup live-set authority with one
sandbox-owned reconciliation adapter over NNC5.2b's immutable evidence report
and NNC5.2c's pure classifier. Container and Krun inject the same durable
attachment, provider-attempt, allocator-inspection, and artifact realm into
that adapter.

The adapter may:

- preserve exact `Adopt` evidence without mutation;
- transition an exact current desired generation to `CleanupPending` using
  `AmbiguousEffect`;
- quarantine an exact adopted allocator hold using its tenant-qualified
  reservation claim; and
- return a stable fail-closed startup diagnostic so unmatched, conflicting,
  or unknown evidence remains durable and blocks new admission on every
  restart.

It may not invoke a provider, remove an artifact, release a hold or IPAM
record, finalize an allocation, free capacity, or derive attachment identity
from an IP address, path, filename, or netns entry. NNC8.3 remains the sole
cleanup-convergence owner.

## Read-only audit

The current startup call graph is:

```text
ContainerSandboxBackend::with_network_authorities
KrunSandboxBackend::with_segment_allocator_and_process
                    |
                    v
reconcile_startup_network_state
  +-> reconcile_terminal_container_ipam_releases
  `-> reconcile_network_segment_orphans
        +-> live_netns_holds
        `-> NetworkSegmentAllocator::reconcile_orphans(live_set)
```

The first child retires terminal IPAM evidence. The second walks
`<workload-root>/tenants/<tenant>/networks/netns/<sandbox>`, converts filenames
into attachment identities, and bulk-quarantines every allocator hold absent
from that set. Both behaviors are outside the NNC5.2d seam: terminal
retirement belongs to later cleanup convergence, and a projection filename
cannot be canonical liveness or workload identity.

NNC5.2b and NNC5.2c already provide the replacement read side:

- `collect_oci_orphan_evidence` joins durable desired/provider authority,
  exact claim-qualified allocator observations, and pinned artifact
  observations without mutation; and
- `classify_oci_orphan_evidence` returns exact retained subjects paired only
  with `Adopt` or one of 19 named `Quarantine` reasons.

The missing seam is a narrow application adapter plus both-backend injection.

## Acceptance criteria

1. NNCV018 first fails with the exact legacy live-set and missing startup
   adapter/injection diagnostics; its five mutation cases fail exclusively.
2. One concept-owned startup module collects and classifies the complete report
   exactly once per reconciliation attempt.
3. Container and Krun pass the same opened attachment authority, IPAM
   authority, allocator capability, and workload artifact root to that module.
4. Every `Adopt` disposition is byte-preserving across desired, IPAM,
   allocator, and artifact evidence.
5. A current desired generation is moved to `CleanupPending` only through its
   exact `NetworkResourceVersion` and `AmbiguousEffect`; replay is idempotent
   and stale generation/version substitution is byte-preserving.
6. An adopted allocator hold is quarantined only with the exact
   tenant/attachment/reservation claim. Missing, reserved, cleanup-pending,
   conflicting, and unknown observations never fabricate quarantine
   authority.
7. If a classification cannot safely mutate either authority, its evidence is
   preserved and startup returns a deterministic diagnostic that fences new
   Container and Krun admission on every fresh reconstruction.
8. Unmatched provider evidence, unmatched artifacts, and scan unknowns with no
   allocator hold remain present and durably fence admission; no path deletes
   or normalizes them into absence.
9. Partial desired-then-allocator application is crash-safe: restart
   reclassifies the exact retained evidence and converges idempotently to the
   same fenced state without provider, cleanup, release, finalization, or
   capacity-reuse effects.
10. `NetworkSegmentAllocator::reconcile_orphans`, every implementation/wrapper,
    `reconcile_network_segment_orphans`, and `live_netns_holds` are deleted.
    No filename-derived replacement is introduced.
11. Existing cleanup/inspection entry points remain available while planning,
    launch, restart, registration, and live policy mutation stay fenced after
    startup reconciliation failure.
12. Focused happy, edge, stale-generation, unknown, unmatched, replay,
    fresh-process, two-backend, and no-effect tests assert exact outcomes.
13. Full affected tests, all-target/all-feature check, strict Clippy,
    warning-denied rustdoc, dependency/effect scans, NNCV018 plus all verifier
    conditions and mutation cases, format/diff, and both docs gates pass with
    exact counts.
14. Exactly one GPT-5.6 Sol/xhigh/fast structured review runs only after every
    preceding criterion is green and the complete item diff is frozen. A
    material accepted executable-code correction permits one narrow correction
    review after affected proofs; no other repeat review is allowed.
15. The exact item code, tests, verifier, proof, and recovery ledger are
    committed together without push or PR.

## Expected-red ledger

| Checkpoint | Command/result |
| --- | --- |
| Starting state | `HEAD=ae29108f3bd2037557727e0036cf0f7ebfc039c0`, tree `528b4f10a2dd6c765d986dee4c292b7a63ba7455`; only the NNC5.2d recovery-header/routing/proof truth-up was dirty before the verifier scaffold. |
| Read-only audit | The two constructors share `reconcile_startup_network_state`, but it receives no durable attachment authority. It retires terminal IPAM evidence and invokes the filename-derived `reconcile_network_segment_orphans -> live_netns_holds -> NetworkSegmentAllocator::reconcile_orphans` path. The NNC5.2b collector and NNC5.2c classifier are production-dead and unwired. |
| NNCV018 expected red | `timeout 1200 bash scripts/verify-nimbus-network-control-plane.sh` exits `1`: NNCV000-NNCV017 pass `18/18`; NNCV018 alone fails. Its exact diagnostics name the missing startup module/export, all required collection/classification/exact-quarantine seams, both missing attachment-authority injections, and all three legacy authorities: `NetworkSegmentAllocator::reconcile_orphans`, `reconcile_network_segment_orphans`, and `live_netns_holds`. |
| NNCV018 mutation contract | The isolated contract runs five child verifier mutations; `legacy-live-set`, `missing-container-injection`, `missing-krun-injection`, `cleanup-capability`, and `missing-exact-quarantine` each fail exclusively as NNCV018. Result: `5 passed; 0 failed`. Bash parse passes. |
| Backend fail-before | The two real composition-root cases initially fail `0/2`: unmatched no-hold evidence still permits fresh Container and Krun admission under the legacy startup path. |
| Executable implementation | A 227-line concept-owned adapter collects and classifies once per attempt, keeps `Adopt` read-only, applies only exact version-fenced desired and claim-fenced allocator quarantine, and returns deterministic retained-evidence admission fences. Container and Krun inject the same opened attachment authority, IPAM authority, allocator capability, and workload root. The portable trait, fixed/single-node/configured/cluster/cleanup wrappers, sandbox helper, filename walker, recording substitute operation, and obsolete bulk tests no longer expose the old live-set API. |
| Behavioral convergence | The startup state-machine matrix passes `8/8`; the classifier passes `12/12`; and four Container/Krun unmatched-provider and desired-only startup authority cases pass `4/4`. The first full affected run passed 1,059 and failed nine cases because it exposed a legitimate desired-absent/live-`Reserved`/bound-`Reserved`/manifest-only restart state that the pure classifier fenced. Its real fail-before failed `0/1`; the exact read-only correction passes with a six-case negative matrix that preserves quarantine for missing hold, missing manifest, present namespace/status, unknown artifact, and incomplete allocator evidence. The second affected run passed 1,067 and failed only two legacy desired-only tests that expected a later planning fence; both now prove the correct earlier startup fence and byte-preserving desired authority. Final `cargo nextest run -p nimbus-network -p nimbus-sandbox` passes `1,070/1,070` with 24 declared skips. All-target/all-feature check, strict Clippy, warning-denied rustdoc, the exact `nimbus-network -> nimbus-core` workspace edge, live verifier `19/19`, aggregate mutation self-tests `72/72`, format/diff checks, docs `108`, and site `17/17` pass. |
| Stale-verifier audit | `timeout 300 bash scripts/verify-multi-tenant-network.sh` first passed 10 and failed six because the retained verifier still searched pre-deep-module locations and, for MTN6, required the deleted filename/live-set authority. The source-only correction now anchors tenant-qualified placement, persisted optional config, the shared attachment teardown lifecycle, shared DNS-off config, both production placement calls, and NNC5.2d's evidence-aware no-cleanup startup seam. It passes `16/16`; `bash -n`, Node syntax validation, and ShellCheck with only the aggregate script's pre-existing SC2034/SC1091 exclusions pass. No production behavior changed for this cleanup. |
| Modularity | The new adapter is 227 lines with a 693-line concept-owned test child. Moving the coherent Krun network-composition group reduced the parent test composition root from 2,030 to 1,552 lines and created a 488-line child with explicit imports; no broad parent-import coupling remains for the moved generation-fencing test. |
| Candidate freeze | The complete 38-path item was staged at tree `fbf5fb030acd3dfc3abf31ebef3e89fa0bcc5ffb`; its staged diff SHA-256 was `8fe8c14b1a790001787c342a49e76f55cb19ee9feb82a591940efe78f7129a05`. There were no unstaged or untracked paths. |
| Structured review | The one full GPT-5.6 Sol/xhigh/fast review ran against the complete frozen item in thread `019fb58a-cf6a-7ad3-92c7-d8856064a1cb`. TruffleHog passed. The review returned zero findings, `patch is correct`, and confidence `0.87`; no accepted defect or executable correction exists, so no narrow correction review is warranted. |
| Exact item commit | Durable at `fc4827b06c672fae7b5f68c9e718100cec3ba83b`, tree `25a7acfb8c9a5bf63f302cd9ac8563266fb7a92d`; 38 paths changed together with no push or PR. |

## Acceptance ledger

| Criterion | Status | Verifiable evidence |
| --- | --- | --- |
| 1. Exact fail-before | `green` | NNCV018 alone failed after NNCV000-NNCV017 passed `18/18`; all five NNCV018 mutations failed exclusively and their contract passed `5/5`; the two real Container/Krun unmatched-evidence cases failed `0/2` before wiring. |
| 2. One collect/classify owner | `green` | `startup_reconciliation.rs` is the sole 227-line application adapter. It calls `collect_oci_orphan_evidence` once, calls `classify_oci_orphan_evidence` once over that immutable report, then consumes its four ordered classification sets. NNCV018 rejects a missing module/export or either missing call. |
| 3. Same injected authorities | `green` | Container and Krun pass the opened `LocalNetworkAttachmentAuthority`, `OciIpamAuthority`, injected `OciSegmentAllocator`, and exact workload root to the same function. The four backend startup authority cases pass `4/4`; missing injection in either backend fails NNCV018 exclusively. |
| 4. Read-only adoption | `green` | `exact_adoption_is_byte_preserving_across_every_durable_authority` proves desired, allocator, IPAM, manifest, namespace, and status bytes are unchanged. The exact reserved pre-effect restart row is also read-only; its positive proof and six negative variations pass inside classifier `12/12`. |
| 5. Exact desired CAS | `green` | `missing_namespace_quarantines_exact_authorities_without_cleanup_or_reuse`, its replay, the partial-application cut, and `stale_desired_version_is_rejected_without_mutating_replacement_authority` prove exact `NetworkResourceVersion` plus `AmbiguousEffect`, idempotence, and byte-preserving stale rejection. |
| 6. Exact allocator claim | `green` | The application obtains quarantine authority only when every allocator observation is `Adopted` with the same exact association/claim and the desired/provider sources agree. Missing, `Reserved`, cleanup-pending, conflicting, and unknown cases cannot mint that claim; the state-machine and classifier matrices pass `8/8` and `12/12`. |
| 7. Durable admission fence | `green` | When neither exact transition is safe, the reconciler preserves evidence and returns ordered tenant-qualified diagnostics. Fresh Container/Krun constructions fence unmatched no-hold evidence on two consecutive opens, while stale/conflicting/unknown replay tests remain deterministic and byte-preserving. |
| 8. Unmatched/unknown preservation | `green` | Unmatched provider-without-hold, unmatched artifact, corrupt manifest, absent-root provider, and scan-unknown proofs preserve the original IPAM/artifact/authority bytes or paths and fence every reconstruction. Provider evidence cannot authenticate an absent artifact root. |
| 9. Crash/restart convergence | `green` | `desired_then_allocator_partial_failure_converges_after_restart` cuts between desired and allocator quarantine, proves the exact first transition durable, retries the retained state to allocator cleanup-pending, and proves replay byte-preserving without provider/cleanup/release/finalization/reuse effects. The pre-effect process-restart regression that initially failed `0/1` now passes without false quarantine. |
| 10. Filename authority deleted | `green` | `NetworkSegmentAllocator::reconcile_orphans`, every wrapper/implementation, `reconcile_network_segment_orphans`, `live_netns_holds`, and their bulk tests are absent from production. NNCV018's `legacy-live-set` mutation fails exclusively; the corrected multi-tenant verifier also requires all three names absent. |
| 11. Existing-work cleanup and inspection | `green` | Container and Krun direct launch and restart-capable inspection tests prove admission/provider relaunch remain fenced before mutation. Exact explicit stop, natural-exit cleanup, and Container plan-only cleanup remain available and publish terminal authority. Container egress reload and all normal planning/launch entry points retain their startup readiness checks. |
| 12. Behavioral breadth | `green` | Focused classifier `12/12`, startup state machine `8/8`, and backend startup authority `4/4` cover happy, edge, stale, unknown, unmatched, replay, backend, and no-effect behavior. Final affected behavior passes `1,070/1,070` with 24 declared skips. |
| 13. Full gates | `green` | All-target/all-feature check, strict Clippy, warning-denied rustdoc, exact `nimbus-network -> nimbus-core` workspace edge, live verifier `19/19`, aggregate mutations `72/72`, corrected multi-tenant verifier `16/16` after real `10/16` fail-before, Bash/Node/ShellCheck, format/diff, docs `108`, and site `17/17` pass. |
| 14. One candidate-frozen review | `green` | One full GPT-5.6 Sol/xhigh/fast review ran against frozen tree `fbf5fb030acd3dfc3abf31ebef3e89fa0bcc5ffb` and staged diff SHA-256 `8fe8c14b1a790001787c342a49e76f55cb19ee9feb82a591940efe78f7129a05`; thread `019fb58a-cf6a-7ad3-92c7-d8856064a1cb` returned zero findings, `patch is correct`, and confidence `0.87`. No correction review is warranted. |
| 15. Exact item commit | `green` | Code, tests, verifiers, proof, routing, and recovery ledger are durable together at `fc4827b06c672fae7b5f68c9e718100cec3ba83b`, tree `25a7acfb8c9a5bf63f302cd9ac8563266fb7a92d`; no push or PR occurred. |

## Planned ownership

- `crates/nimbus-sandbox/src/backends/oci/network/startup_reconciliation.rs`
- the narrow composition/export surface in
  `crates/nimbus-sandbox/src/backends/oci/network.rs`
- Container and Krun startup composition roots
- deletion of the portable/sandbox/cluster/test filename-live-set method and
  its obsolete tests
- NNCV018 helper, contract, aggregate wiring, and exact behavioral tests
- this proof, the canonical plan, and routing truth-up

No provider effect, cleanup, artifact removal, IPAM retirement, hold release,
allocation finalization, capacity reuse, service naming, policy, proxy,
cluster transport, or `nimbus-network` dependency expansion belongs to this
item.
