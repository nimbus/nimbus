# NNC4.6f Production Network-Authority Census

Date: 2026-07-28

Status: `complete; F1-F13 and all accepted review corrections green`

Owner: NNC4.6f in
`docs/private/plans/nimbus-network-control-plane-plan.md`.

Source commit:
`32d13652261fb7832edd7afe7695af9c6cc04a01`

Source tree:
`be4a5179b1d799f6c979bcfe6c9119be285c53d2`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Unit Of Value

NNC4.6f closes the repository-wide construction boundary after NNC4.6b-e and
NNC4.6g introduced the concrete local-node, parent-host, and guest-node
compositions. The result is one machine-readable, source-derived census that
classifies every production manager constructor, typed root resolver,
manager-derived handle, and raw primitive reconstruction as exactly one of:

1. `owning-manager`;
2. `manager-derived-handle`;
3. `admitted-cross-process-reconstruction`; or
4. `test-fixture`.

The census is exact source evidence, not Rust name-resolution or runtime
evidence by implication. A row may claim behavioral runtime proof only when it
links a named criterion in an existing proof artifact. Test fixtures and
source-only future seams cannot claim runtime exercise.

This item does not create a second manager, persist a capability registry,
move provider effects, broaden a direct constructor, or absorb cluster
transport. It closes substitution and evidence around the seams already
implemented.

## Read-Only Audit

The initial audit found that NNCV015 is not yet the NNC4.6f result:

| Area | Current source truth | NNC4.6f gap |
| --- | --- | --- |
| Composition helper | `scripts/verify-nimbus-network-composition-census.mjs` contains a hard-coded `Set` for start/dev/Compose/server/KV sites. | There is no standalone machine-readable census schema or exact repository-wide classification. |
| Machine realms | The structural scanner observes the parent `LocalNetworkManager::open` and guest `bootstrap`, but the helper excludes both machine composition modules. | Parent-host and guest-node construction can drift without NNCV015 failing. |
| Typed roots | `LocalNodeNetworkRoot::resolve_for_current_platform` is the canonical policy seam for start, Compose, KV, and direct host-machine composition. | The scanner does not classify root resolvers, so a divergent resolver is invisible. |
| Primitive state handles | Manager, port authority, sandbox direct/runner adapters, and future cluster lease/cleanup boundaries deliberately open `LocalNetworkStateStore` or `LocalPortLeaseAuthority`. | Only port opens are partially recognized; state-store and named runner/cluster reconstruction are not closed. |
| Direct/test seams | Public direct server, KV, container, and krun constructors remain compiled as explicit lower-level embedder/test adapters. | The census must call only exact approved fixture entrypoints `test-fixture`; a broad `direct` name match cannot bless a new production caller, and compilation alone cannot claim runtime proof. |
| Test ownership | Path conventions and `#[cfg(test)]` items are skipped, but a nested child of a path-owned test module can be walked independently. | Transitive path-owned test provenance must be proved or the child must be explicitly classified; it cannot silently look like production. |
| Parent identity | Parent launch mints `MachineForwarderAuthority`; guest Machine API only authenticates parent-issued evidence. | The scanner does not reject a new guest-side mint. |
| AST test harness | Composition output was added to the scanner, but its unit-test fixture did not add the corresponding vector/field. | The scanner's own unit tests do not compile, so its structural behavior is not presently unit-proved. |

The existing source scan reports 61 composition occurrences across 19 kinds.
That number is not an acceptance baseline: it omits root resolvers, state-store
opens, named runner/cluster boundaries, manager-derived handoffs, and
parent-authority minting. The corrected scanner establishes the first complete
baseline; the census then freezes it by path, kind, enclosing symbol, ordinal,
and diagnostic line.

### Preserved authority model

```text
typed OS-node root
  -> owning LocalNetworkManager
       -> manager-derived LocalNetworkAuthority / state / port handles
       -> manager-derived server, KV, sandbox, parent-host, or guest-node adapter

serialized exact root + named process boundary
  -> admitted child/recovery reconstruction

#[cfg(test)] or mechanically path-owned fixture
  -> test fixture only
```

An address, port, PID, provider handle, workload artifact root, or machine
guest fact never selects or identifies the node authority.

## Frozen Acceptance Criteria

| ID | Verifiable success criterion |
| --- | --- |
| F1 | A versioned JSON census exists outside the verifier implementation and validates its own scope, allowed classes, realms, occurrence identity, and evidence vocabulary. Missing, malformed, duplicate, or stale rows fail NNCV015. |
| F2 | The structural scanner recognizes every production manager constructor, `LocalNodeNetworkRoot` resolver, manager-derived composition handoff, raw store/port open, named direct/runner/cluster reconstruction, and parent-forwarder identity mint in the audited source families. |
| F3 | Every observed occurrence has exactly one census row keyed by path, structural kind, enclosing symbol, and ordinal. Every census row has exactly one observed occurrence and a current diagnostic line. |
| F4 | Every row uses exactly one allowed classification: `owning-manager`, `manager-derived-handle`, `admitted-cross-process-reconstruction`, or `test-fixture`. No generic allowlist, unclassified raw root, implicit fallback, or compatibility seam remains. |
| F5 | Parent-host and guest-node entries carry distinct OS-node realms. Direct host composition resolves and owns the parent root; Machine API owns the guest control root; runner reconstruction remains relative to the current OS node. |
| F6 | `MachineForwarderAuthority` is minted only by parent-owned lifecycle code or portable type tests. Guest Machine API/config paths may authenticate or copy parent-issued evidence but cannot mint or replace it. |
| F7 | Behavioral evidence is honest. A behavioral row links an existing proof artifact and named passing criterion; a test fixture or source-only future seam cannot claim runtime proof. Source evidence is explicitly labeled as such. |
| F8 | Test-only code is excluded only through mechanically proven `#[cfg(test)]`, path convention, test-support crate, or transitive path-owned module provenance. A production inclusion edge into exempt code fails closed. |
| F9 | The verifier self-tests all six named escapes: missing census, second constructor, divergent resolver, wrong OS-node realm, guest-minted parent identity, and false runtime-proof claims. Each produces an exclusive NNCV015 failure with the expected diagnostic. |
| F10 | The census helper and structural scanner have focused happy/edge/error tests, including cfg-test adjacency, transitive test ownership, duplicate/stale classification, realm mismatch, and proof-claim validation. |
| F11 | Live NNCV015 and the aggregate verifier are green, while the verifier self-test count increases by exactly the new named cases. Existing bind/allocation census and source-contract conditions remain green. |
| F12 | Cargo metadata still proves `nimbus-network -> nimbus-core` is the sole workspace edge. No socket, provider effect, policy, naming, forwarding, cluster transport, cloud SDK, or upper-owner type enters `nimbus-network`. |
| F13 | Focused scanner tests, affected production tests, all-target/all-feature checks, strict Clippy, warning-denied rustdoc, format/diff, docs, and site gates pass with exact counts. One Sol/xhigh/fast structured review runs only after F1-F13 are candidate-green; a material accepted executable correction permits one narrow correction review. |

## Expected-Red Evidence

### R1 — scanner unit contract is not green

Command:

```text
timeout 600 cargo test \
  --manifest-path scripts/nimbus-network-bind-census-ast/Cargo.toml \
  --locked
```

Result: exit `101`, zero tests executed. The test fixture calls `scan_source`
without the new composition vector, constructs `ScanOutput` without its
`composition` field, and consequently infers the wrong vector types. This is a
directly related harness defect; it is not an unrelated workspace failure.

### R2 — required census does not exist

No `nnc4.6f` machine-readable composition-authority JSON exists. NNCV015 reads
the NNC0.1 bind inventory only to obtain scanner exclusions and compares a
local hard-coded JavaScript set. It therefore cannot self-test a missing
composition census.

### R3 — machine and root escapes are invisible

The current `--print-composition` output includes the parent manager open and
guest manager bootstrap, but `inLocalCompositionScope` drops their paths.
`LocalNodeNetworkRoot::resolve_for_current_platform`,
`LocalNetworkStateStore::open`, named runner/cluster reconstruction, and
`MachineForwarderAuthority::new` are not structural composition kinds at all.
The named second-root/realm/identity failures therefore cannot be expressed
by the current verifier.

## Implementation Bands

| Band | Work | Gate |
| --- | --- | --- |
| A | Repair and deepen the structural scanner; add its focused fail-before and green unit cases. | Scanner tests compile and prove exact cfg/test and constructor recognition. |
| B | Add the versioned machine-readable census and replace the hard-coded local set with schema/exactness/evidence validation. | Every live observed occurrence is classified once; missing/stale/duplicate/invalid rows fail. |
| C | Add the six named aggregate verifier self-tests and exact diagnostics. | Each mutation fails only NNCV015 for the intended reason; the unmodified verifier remains green. |
| D | Resolve any census-exposed production raw-root seam, run affected behavioral/static/quality gates, freeze the item, review once, and close the ledgers. | F1-F13 all pass with exact evidence and one exact item commit. |

## Implemented Census And Verifier

The source-derived census now contains 105 exact occurrences:

| Classification | Count | Meaning |
| --- | ---: | --- |
| `owning-manager` | 22 | A typed root resolver, process composition claim, or manager-owned primitive open establishes the one OS-node authority. |
| `manager-derived-handle` | 37 | A composition receives authority, provider capabilities, listeners, backends, or parent-machine authority from the owning manager. |
| `admitted-cross-process-reconstruction` | 23 | A named runner, cluster, cleanup, or primitive recovery boundary reconstructs from an exact serialized root and remains independently fenced. |
| `test-fixture` | 23 | A compiled direct seam is contained by the verifier's exact approved fixture-entrypoint policy and claims source-contract evidence only. |

Every row records path, structural kind, enclosing symbol, ordinal, current
line, classification, OS-node realm, and evidence ID. The verifier derives
classification and realm independently from the structural source occurrence,
then requires one exact row in each direction. Missing, duplicate, malformed,
stale, wrongly classified, wrongly rooted, or falsely evidenced rows fail
NNCV015.

`MachineBootAuthorityEvidence` construction is intentionally absent from the
authority census. It transports and authenticates already-issued evidence; it
does not mint, own, reconstruct, or delegate a network authority.
`MachineForwarderAuthority::new` remains the exact parent-identity mint and is
fully covered in both source declaration and parent lifecycle construction.

The test-exemption inventory now proves transitive path ownership for the
container launch-cleanup child and exact configured-segment test children.
Conventional modules must resolve to the exact Rust `module.rs` or
`module/mod.rs` target for their declaration context; a merely related
descendant path is rejected. A production inclusion edge into any such path
still fails NNCV006.

### Modularity disposition

The structural scanner's 372-line private test module moved to
`scripts/nimbus-network-bind-census-ast/src/tests.rs`; its production
composition root is 1,425 lines. The six NNC4.6f mutation cases moved to
`scripts/nimbus-network-control-plane/composition-census-self-tests.sh`; the
three exact bind-exemption mutations moved to
`scripts/nimbus-network-control-plane/bind-exemption-self-tests.sh`. The
aggregate verifier is 1,439 lines, the composition child is 120 lines, and the
bind-exemption child is 109 lines. The bind helper is 848 lines and the
composition helper is 797 lines. No
changed implementation or test composition root reaches the 1,500-line
justification threshold.

## Structured Review Findings And Corrections

The sole full item review ran over the initial frozen
executable/policy-input SHA-256
`fd8b2375015df00be2b8c43cd5f8b7a68844e3a2a257fe0ac51ed02f1f31013b`
with GPT-5.6 Sol, xhigh reasoning, and fast mode. It returned five findings
and classified the patch incorrect at `0.98`. Source inspection accepted all
five because each exposed a real fail-open path against F4 or F6-F10:

| Finding | Disposition | Correction and proof |
| --- | --- | --- |
| P1: parent-forwarder minting was rejected only for a guest-looking path or test symbol. | Accepted. | The verifier now admits exactly `crates/nimbus-cli/src/machine/manager/launch.rs\|machine-forwarder-authority-mint\|next_machine_forwarder_authority\|1`; every other production mint fails. The declaration remains portable source evidence. The guest-mint mutation exercises the exact negative policy. |
| P1: a conventional `mod child;` exemption admitted any descendant of the declaration directory. | Accepted. | Explicit `#[path]` declarations remain exact; conventional declarations now resolve to the exact Rust `child.rs` or `child/mod.rs` target in the declaration context, including a path-attributed parent. A new mutation substitutes an existing unrelated descendant and must fail NNCV006. |
| P1: any kind containing `direct` could be blessed as a test fixture by adding a census row. | Accepted. | Fixture classification now comes from an exact path-plus-entrypoint policy. Primitive manager and reconstruction entrypoints are separately exact. A new mutation adds both an unapproved direct constructor and a matching `test-fixture` row; NNCV015 still rejects it for lacking authority policy. |
| P2: source-only future cluster/cleanup seams could cite behavioral proof. | Accepted. | Every future realm, cluster, or cleanup-handle occurrence and its raw open must cite `source-contract`; the two primitive cluster opens were corrected. The false-proof mutation now corrupts both a fixture and a future seam and requires both diagnostics. |
| P2: the six named mutations did not prove NNCV015 was the only failed condition. | Accepted. | A shared assertion now requires exactly one `FAIL` line, requires that line to be NNCV015, and forbids an NNCV015 pass for all six named cases. |

These corrections materially changed executable verifier behavior, so the
review cadence permitted exactly one narrow correction review after all
affected proofs were green. That GPT-5.6 Sol/xhigh/fast review ran over
corrected digest
`192ec449f2e0de73183655d945f80687eac72725a569c783aa5db8813aa61d55`,
returned three P1 findings, and classified the correction patch incorrect at
`0.98`. Source inspection accepted all three:

| Narrow-review finding | Disposition | Final correction and proof |
| --- | --- | --- |
| Path-plus-function fixture keys could collide across impl methods. | Accepted. | Every direct fixture, manager primitive, primitive reconstruction, and segment reconstruction is now admitted by full census identity: path, structural kind, enclosing symbol, and ordinal. The blessing mutation fabricates the same approved path/kind/symbol at ordinal 2 and remains rejected. |
| `segment-primitive-reconstruction` bypassed the primitive occurrence policy. | Accepted. | Segment reconstruction has its own exact occurrence policy. The blessing mutation adds an arbitrary segment raw-root call plus a matching admitted census row; NNCV015 still rejects it. |
| An explicit `#[path]` module could fall through to a conventional target. | Accepted. | The helper first detects any exact module-associated path override and forbids conventional fallback when present. A synthetic owner containing both the explicit and conventional files proves only the explicit file can be exempt. |

The exact review budget is now exhausted. Per the owner cadence, no second
narrow or full review ran. Owner inspection plus the focused mutation checks,
the isolated three-case bind-exemption child, and the final serialized 60-case
aggregate sweep prove all three accepted findings closed.

## Affected-Behavior Baseline Disposition

The first broad all-feature Nextest lane ran 2,587 tests: 2,584 passed, three
failed, and 54 were skipped. It is not represented as a green gate:

- `deploy_admin_requires_local_admin_header_even_with_deploy_bearer` still
  expects HTTP 200 but receives 400;
- `cloud_functions_passes_runtime_owner_lifecycle_conformance` still expects
  HTTP 200 but receives 409; and
- the all-feature-only runtime-metrics case expects `not_linked` while the
  current built artifact truth is `missing_artifact`.

All three reproduce individually. The first two are the exact PR #238/#239
post-merge trust-fixture regressions already reproduced and owned outside this
network item. The third is an all-feature expectation mismatch; F13 requires
all-target/all-feature compilation separately, not a false claim that every
feature-union behavior expectation is the default runtime contract.

The canonical default-feature lane therefore disabled implicit external
provider fixtures, excluded only the two previously documented exact
trust-boundary names, bounded concurrency at four threads, and ran the seven
census-covered crates. It passed 2,585/2,585 with 56 declared skips. No NNC4.6f
production Rust changed; this lane proves the source families classified by
the census remain behaviorally green without weakening any assertion.

## Recovery Ledger

| Field | Current checkpoint |
| --- | --- |
| Item | `NNC4.6f` |
| Status | `complete` |
| Base commit/tree | `32d13652261fb7832edd7afe7695af9c6cc04a01` / `be4a5179b1d799f6c979bcfe6c9119be285c53d2` |
| Owned dirty paths | This proof; `nnc4.6f-production-network-authority-census.json`; the NNC0.1 exact test-exemption inventory; the structural scanner and its child tests; bind/composition census helpers; aggregate verifier plus its composition-census and bind-exemption self-test children; canonical plan header and recovery row. |
| Last green | Corrected exact 105-row census: 22 owning managers, 37 manager-derived handles, 23 admitted reconstructions, 23 exact-occurrence fixtures. Scanner 11/11; bind/composition helpers exit 0; live verifier 16/16; isolated bind-exemption child 3/3; final serialized aggregate self-test 60/60; exact core-only edge; Node/shell/Prettier/Cargo format/diff; website 109 outputs; docs 108; site 17/17. The accepted corrections changed no production Rust, so the already-green affected behavior 2,585/2,585 with 56 declared skips, seven-crate all-target/all-feature check, workspace strict Clippy, and warning-denied rustdoc remain the exact candidate evidence. |
| Expected red | Scanner unit binary exits 101 before tests because its composition fixture is incomplete; required standalone census is absent; current NNCV015 cannot express the six named escapes. |
| Next action | Commit the exact NNC4.6f checkpoint, then begin NNC4.7's read-only sovereignty-tripwire audit. |
| Blocker | None. |

## Closeout Ledger

The initial candidate was frozen at executable/policy-input SHA-256
`fd8b2375015df00be2b8c43cd5f8b7a68844e3a2a257fe0ac51ed02f1f31013b`.
Its sole full structured item review produced the five accepted findings above.
The first corrected candidate reviewed narrowly had executable/policy-input
SHA-256
`192ec449f2e0de73183655d945f80687eac72725a569c783aa5db8813aa61d55`.
The final candidate is frozen at executable/policy-input SHA-256
`4e0d223c0eb814569c2c1909ac45ebf060cf439602546fe9c00c95fd20a39cfa`,
computed over the exact staged binary diff for the two
JSON policy inputs and seven scanner/verifier source paths. The eleven-path
staged item has no unstaged diff.

| Evidence | Result |
| --- | --- |
| Machine-readable census | PASS: 105 exact rows; 22 owning, 37 manager-derived, 23 admitted reconstruction, 23 test fixture; standalone bind and composition helpers exit 0. |
| Scanner focused tests | PASS: 11 passed, 0 failed, 0 ignored. |
| Six named verifier self-tests | PASS within the corrected aggregate result: missing census, second constructor, divergent resolver, wrong realm, guest mint, and false proof each produce exactly one failed condition, NNCV015, with the intended diagnostic. |
| Live verifier | PASS after correction: 16 passed, 0 failed. |
| Aggregate verifier self-test | PASS final serialized run: 60 passed, 0 failed. The pre-review 51-test baseline gained exactly the six F9 cases plus three accepted-review regressions: exact conventional resolution, census-blessing rejection for direct/raw-root collisions, and explicit-path override precedence. |
| Affected behavioral suites | PASS: canonical default-feature lane 2,585/2,585 with 56 declared skips. The separate all-feature lane's three exact failures are dispositioned above and are not represented as green. |
| Metadata/effect/dependency proof | PASS: Cargo metadata reports only `nimbus-core`; NNCV004/NNCV007/NNCV012 and direct forbidden-dependency/effect scan pass. |
| All-target/all-feature check | PASS for `nimbus-network`, `nimbus-operator`, `nimbus-sandbox`, `nimbus-server`, `nimbus-kv`, `nimbus-cli`, and `nimbus-machine`. |
| Format/Clippy/rustdoc/diff | PASS: workspace format, workspace strict Clippy, warning-denied seven-package rustdoc producing eight outputs, shell/Node syntax, and diff check. Vendored Brotli warnings are inherited and outside the owned diff. |
| Modularity | PASS: scanner 1,425 + child tests 372; aggregate 1,439 + composition mutations 120 + bind-exemption mutations 109; helpers 848 and 797. |
| Docs/site | PASS after correction: website build emitted 109 HTML files and all LLM artifacts; `check-docs` reports 108 link-clean content pages with source map, private fence, and titles green; site verifier 17/17. |
| Structured item review | PASS cadence: one full review produced five accepted findings; the one permitted narrow review produced three accepted findings; all eight are corrected and proven. No further structured review ran or is warranted. |
| Exact item commit | This NNC4.6f commit contains the proof, policy inputs, verifier implementation, and recovery-ledger checkpoint together. |
