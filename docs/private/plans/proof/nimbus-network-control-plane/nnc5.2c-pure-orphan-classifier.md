# NNC5.2c Pure Exhaustive Orphan Classifier

Status: `complete; durable at ae29108f3bd2037557727e0036cf0f7ebfc039c0`

Owner: `NNC5.2c`

Starting commit: `3f783ec4c450924a5f0418e01f0a90a530e50909`

Starting tree: `df426953898af5fd60c111ef0b5c8058c73c5e78`

## Purpose

NNC5.2c turns the immutable evidence snapshot delivered by NNC5.2b into one
total, deterministic disposition for every candidate and every unjoined
observation. It does not read durable state or the filesystem, invoke a
provider, mutate an authority, apply quarantine, clean an artifact, release a
reservation, finalize a generation, or make capacity reusable.

The only dispositions are:

- `Adopt`, when all current desired, provider-attempt, allocator, and required
  artifact evidence authenticates the same tenant-qualified generation; and
- `Quarantine(<named reason>)` for every missing, stale, conflicting,
  incomplete, unmatched, or unknown evidence shape.

There is deliberately no `Remove` disposition. NNC5.2d owns exact,
CAS-fenced quarantine application and startup wiring. NNC8.3 remains the sole
owner of provider cleanup, artifact removal, allocation release, finalization,
and repeated convergence.

## Read-only audit

The durable NNC5.2b snapshot has four evidence classes:

1. tenant-qualified candidates formed only from portable desired attachment
   authority and/or sandbox provider-attempt authority;
2. exact claim-qualified allocator observations for every desired/provider
   claim;
3. tri-state manifest, persistent-netns, and status observations beneath the
   authenticated provider realm; and
4. unmatched provider evidence, unmatched artifacts, and artifact-scan
   unknowns that are retained outside the canonical candidate union.

The current ignored NNC0.7 matrix remains the required fail-before baseline,
but it is not a pass-after acceptance test:

- it writes synthetic desired/effect JSON that production does not read;
- it derives a string result from files left after the mutating filename
  reaper;
- it cannot express exact tenant/attachment/claim/segment/epoch/backend/realm
  equality; and
- five rows accept the weaker `removed-or-quarantined` result.

NNC5.2c therefore retains that test as historical expected-red evidence and
adds an item-local pure classifier matrix over the real NNC5.2b evidence
vocabulary. NNC5.2d, not this item, replaces the old mutating startup path and
turns the integration-level orphan baseline green.

## Required matrix

| Row | Exact durable shape | Required disposition |
| --- | --- | --- |
| hold + desired + effect | One current desired generation, one live provider attempt, the same exact claim/segment/epoch from every allocator observation, and present required effect evidence | `Adopt` |
| hold + no desired + effect | Live provider/effect and exact allocator hold, but no portable desired generation | named `Quarantine` |
| hold + no netns | Desired/provider/allocator ownership exists, but the durable provider phase requires a namespace and its exact observation is absent | named `Quarantine` |
| effect + no hold | Desired/provider effect exists, but exact allocator inspection reports no current hold | named `Quarantine` |
| manifest + no hold | An unmatched manifest observation has no canonical desired/provider identity and no hold | named `Quarantine` |
| hold + netns + no manifest | The same exact current generation and required provider/netns evidence exists; the non-authoritative manifest projection is absent | `Adopt` |
| stale-generation evidence | Desired, provider, or allocator claim/segment/epoch evidence does not describe the same current generation | named `Quarantine` |
| unknown inspection | Any required allocator, artifact, provider-realm, or scan observation is unknown | named `Quarantine` |

The item-local test must aggregate and print all eight rows before its terminal
assertion. Additional truth-table tests must cover every named reason and prove
deterministic precedence when more than one unsafe condition is present.

## Frozen seam constraints

- Classification consumes immutable evidence values or immutable references
  only.
- Classifier modules may not import `std::fs`, `cap_std`, socket APIs, provider
  runners, mutable authorities, allocator mutation traits, or effect helpers.
- The classifier cannot construct identity from a path. Unmatched artifacts
  remain untrusted artifact subjects.
- IP addresses, filenames, netns paths, and status paths are never workload or
  attachment identity.
- Manifest absence alone cannot veto adoption when canonical desired,
  provider, allocator, and required effect evidence is exact.
- Any typed unknown fails closed without being flattened to absence.
- The result vocabulary contains no remove/cleanup/release/finalize/reuse
  capability.
- `nimbus-network -> nimbus-core` remains the only workspace dependency edge.

## Acceptance criteria

1. The historical NNC0.7 matrix is rerun before executable edits and remains
   expected red with all eight rows mismatched.
2. A fail-before item-local test proves that the pure classifier contract is
   absent before implementation.
3. The pure pass-after matrix executes all eight required rows and returns only
   the exact `Adopt` or named `Quarantine` result.
4. Every candidate field and every report-level unmatched/unknown collection
   is consumed or deliberately retained by the classifier; no evidence class
   is silently dropped.
5. Exact tenant, attachment, reservation claim, segment, lease epoch, desired
   generation/digest, provider lifecycle, provider phase, and required
   artifact observations are authenticated before `Adopt`.
6. Unknown allocator/artifact/realm/scan evidence always returns a named
   quarantine reason; no error becomes absence.
7. Manifest absence is proven non-authoritative; missing required provider
   effect evidence is proven fail closed.
8. The classifier is deterministic, total over the closed evidence enums, and
   I/O-free. Source checks reject filesystem, socket, provider-effect,
   mutation, cleanup, release, finalization, and reuse imports or calls.
9. Focused happy, edge, mismatch, stale-generation, unknown, unmatched, and
   deterministic-precedence tests assert concrete outcomes rather than
   compilation or non-panic.
10. Full affected tests, all-target/all-feature check, strict Clippy,
    warning-denied rustdoc, dependency/effect scans, the network-control-plane
    verifier, format/diff checks, and both docs gates pass with exact counts.
11. Exactly one GPT-5.6 Sol/xhigh/fast structured review runs only after every
    preceding criterion is green and the complete item diff is
    candidate-frozen. A material accepted executable-code correction permits
    one narrow correction review after affected proofs; no other repeat review
    is allowed.
12. The exact item code, tests, proof, and recovery ledger are committed
    together without push or PR.

## Planned ownership

- the concept-owned pure desired-plan compiler beneath
  `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/`
- `crates/nimbus-sandbox/src/backends/oci/network/orphan_evidence.rs`
- one concept-owned pure classifier child and its tests beneath
  `crates/nimbus-sandbox/src/backends/oci/network/orphan_evidence/`
- `docs/private/plans/proof/nimbus-network-control-plane/nnc5.2c-pure-orphan-classifier.md`
- `docs/private/plans/nimbus-network-control-plane-plan.md`

No NNC5.2d startup, backend wiring, durable quarantine mutation, admission
fence, filename-reaper deletion, provider cleanup, artifact removal, release,
finalization, or capacity-reuse path is owned by this item.

## Implemented seam

The item has one closed classifier result vocabulary: `Adopt` or
`Quarantine(<one of 19 named reasons>)`. Each classification retains an
immutable reference to the exact evidence subject, so NNC5.2d cannot pair a
disposition with a different candidate by parallel-vector position.

The classifier authenticates:

- tenant and attachment identity from desired and provider authority;
- selected backend registration and the provider locator's derived sandbox
  attachment;
- the live OCI desired-plan ID, generation, digest, resource ID, and epoch by
  calling the same pure plan compiler used during normal attachment creation;
- the exact stable provider handle, when present, by calling the same pure
  handle compiler used during normal attachment creation;
- exact claim, segment, and lease epoch from both desired and provider-derived
  allocator observations;
- live provider-attempt lifecycle and a completed Netavark setup phase; and
- one manifest, network-namespace, and status observation, preserving typed
  unknown separately from absence.

`Adopt` means the exact ownership/effect generation is current enough for
NNC5.2d to retain without mutation. It does not claim complete service
readiness: NNC5.3 still owns firewall/pin, forwarding, PEP, and other readiness
composition. A `Provisioning` desired state may therefore adopt exact
provider-ready evidence after a crash either before or after the portable
handle checkpoint; when a handle is present in any adoptable phase, it must
exactly equal the current stable handle. `Ready`, `Publishing`, and `Active`
additionally require that exact handle to be present.

Manifest absence is explicitly non-authoritative. Netns or provider-status
absence after a durable ready provider attempt is inconsistent evidence and
quarantines. Any unknown observation outranks known lower-level absence, while
missing desired identity and stale generation fences take deterministic
precedence before observational evidence.

The pure desired-plan compiler is a separate concept-owned module. Live
attachment creation and startup classification both call it; recovery does not
duplicate plan ID, generation, content digest, or capability-requirement
rules. Static tests reject I/O, sockets, provider effects, mutable authority,
cleanup, release, finalization, reuse, `unsafe`, and panic shortcuts from both
the classifier and that compiler.

## Evidence ledger

| Checkpoint | Command/result |
| --- | --- |
| Starting state | `HEAD=3f783ec4c450924a5f0418e01f0a90a530e50909`, tree `df426953898af5fd60c111ef0b5c8058c73c5e78`; the NNC5.2c recovery-header truth-up was the sole dirty path before this proof was created. |
| Historical NNC0.7 fail-before | `timeout 300 cargo test -p nimbus-sandbox --lib nnc0_7_orphan_recovery_must_classify_the_complete_evidence_matrix -- --ignored --nocapture` exited `101`: `0 passed; 1 failed; 828 filtered out`. The terminal mismatch map contained all eight required names. |
| Item-local pure fail-before | `timeout 300 cargo test -p nimbus-sandbox --lib nnc5_2c_pure_orphan_classifier_covers_complete_evidence_matrix -- --nocapture` exited `101`: `0 passed; 1 failed; 830 filtered out`. The immutable real-evidence matrix executed all eight rows; the non-classifying scaffold returned `UnknownInspection` for every subject, so only the unknown row matched and the other seven were printed together. |
| Pass-after matrix and focused classifier | Final `timeout 300 cargo test -p nimbus-sandbox --lib orphan_evidence::classifier::tests -- --nocapture`: `10 passed; 0 failed; 0 ignored; 829 filtered out`. The required matrix executes all eight rows; all 19 named quarantine reasons have distinct behavioral witnesses; exact, missing, and substituted stable provider handles are proven across `Provisioning`, `Ready`, `Publishing`, and `Active`; exact Container/Krun current phases adopt; plan ID/generation/digest and stable-handle substitutions quarantine; unknown/unmatched/precedence paths fail closed; two candidates and every report collection retain exact ordered evidence references; two durable authorities remain byte-identical. |
| Complete orphan evidence | Final `timeout 600 cargo test -p nimbus-sandbox --lib orphan_evidence -- --nocapture`: `28 passed; 0 failed; 1 ignored; 810 filtered out`. The ignored test is the declared fresh-process child entry point. |
| Live compiler and provider evidence lanes | Post-correction attachment lifecycle: `44 passed; 0 failed; 794 filtered out`. IPAM/provider evidence: `17 passed; 0 failed; 821 filtered out`. |
| Full affected behavior | Final `timeout 1800 cargo nextest run -p nimbus-network -p nimbus-sandbox`: `1,058 tests run: 1,058 passed; 26 skipped`. Nextest annotated one passing test as leaky; no test failed or timed out, and both final phase-matrix corrections completed normally in the same run. |
| Affected quality | `cargo check -p nimbus-sandbox --all-targets --all-features`, strict `cargo clippy -p nimbus-sandbox --all-targets --all-features -- -D warnings`, and `RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-sandbox --no-deps --all-features` all exit `0`. Only the repository's pre-existing vendored Brotli warnings appear before the strict workspace crate. |
| Dependency/effect boundary | Post-correction Cargo metadata exits `0` and reports exactly `nimbus-network workspace dependencies: nimbus-core`; the live verifier passes `18/18`. Its unchanged adversarial self-test passed `67/67` before the correction. A redundant post-correction `timeout 1200 ... --self-test` rerun passed every emitted case but reached the outer bound during the final NNCV017 group; the complete attachment-ordering mutation contract then passed directly `5/5`. No verifier source changed. |
| Format/diff | `cargo fmt --all` and `git diff --check` pass. |
| Documentation | `scripts/check-docs.sh` passes `108` pages. `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |
| Full structured item review | One complete GPT-5.6 Sol/xhigh/fast item review ran as thread `019fb4fc-36c1-7c03-8fc3-43aa5c774887`. It found one P1 at confidence `0.96`: Ready/Publishing/Active checked provider-handle presence but did not authenticate the stable opaque handle. Accepted; the same-provider substituted handle could incorrectly reach `Adopt`. No other finding was reported. |
| Accepted-finding fail-before | With the exact regression and shared pure handle compiler present but before classifier comparison, `timeout 300 cargo test -p nimbus-sandbox --lib substituted_provider_handle_cannot_adopt_current_generation -- --nocapture` exited `101`: `0 passed; 1 failed; 837 filtered out`; actual `Adopt` differed from required `Quarantine(DesiredProviderHandleMismatch)`. |
| Accepted-finding correction | Live creation and recovery now call one pure `oci_attachment_provider_handle` compiler. Classification compares full provider ID plus opaque value whenever a handle exists, accepts exact/no-handle `Provisioning`, requires presence from `Ready` onward, and gives mismatch its own closed reason. Exact regression passes `1/1`, 837 filtered; the complete classifier and every affected proof above pass. |
| Narrow correction review | The one permitted GPT-5.6 Sol/xhigh/fast correction review ran as thread `019fb523-fa38-79e1-9c6d-e341f424a6e3`. It found one P2 test-proof gap at confidence `0.99`: the implementation correctly handled all desired phases, but the tests explicitly exercised substituted and missing handles only at `Ready`. Accepted as a proof gap, not a production defect. The final test-only correction proves exact, missing, and substituted handles across `Provisioning`, `Ready`, `Publishing`, and `Active`: exact always adopts; missing adopts only while provisioning; substituted always quarantines. Focused tests pass `1/1` for each new matrix, the classifier passes `10/10`, orphan evidence passes `28/28` plus its declared child skip, and affected crates pass `1,058/1,058` with 26 declared skips. The review cadence is exhausted; no third review ran or is warranted. |
| Exact item commit | `ae29108f3bd2037557727e0036cf0f7ebfc039c0` (`feat(network): classify OCI orphan evidence`), tree `528b4f10a2dd6c765d986dee4c292b7a63ba7455`; exactly 11 owned paths, no push or PR. |

The first item-local command attempt also exited `101`, but compilation stopped
on unused imports left by the test-support extraction and expected-red enum
variants. It is rejected as behavioral evidence. After correcting only that
test-harness hygiene, the command above compiled, ran the named test, exercised
all eight evidence rows, and failed at the intended terminal disposition
assertion.

## Candidate-freeze baseline

The frozen review unit is the complete NNC5.2c item, not an implementation
chunk:

- target branch: `codex/nimbus-network-architecture-audit`;
- starting commit: `3f783ec4c450924a5f0418e01f0a90a530e50909`;
- owner boundary: pure OCI orphan classification plus the shared pure desired
  plan compiler and its test support;
- staged paths: 11;
- production Rust delta: 497 additions and 56 deletions across six paths;
- executable patch SHA-256:
  `385697a35da8ab1599af0a49db2057fbfabacc41d5cdb9d7870fa96d6008495e`;
- no NNC5.2d application/wiring, provider effect, cleanup, release,
  finalization, capacity reuse, push, or PR.

The full item review's only accepted production defect and the narrow
correction review's test-proof gap are corrected, every affected proof is
green, and the review cadence is exhausted. The exact 11-path item is durable
at `ae29108f3bd2037557727e0036cf0f7ebfc039c0`; NNC5.2d now owns startup
application and filename-authority deletion.
