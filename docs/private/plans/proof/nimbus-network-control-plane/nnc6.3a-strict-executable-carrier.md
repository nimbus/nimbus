# NNC6.3a Strict Executable Carrier And Closed Desired Digest

Status: `complete; E1-E20 green; full and narrow reviews dispositioned`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.3 proved that the durable saga contains a complete compiled network plan
but no executable workload source. NNC6.3a adds that missing durable value. It
does not select a provider, call an effect, cut over a caller, split the service
registry, or introduce provision decisions.

The carrier belongs to `nimbus-workloads`. It remains opaque to that crate.
Only `nimbus-compute` converts between the carrier and `SandboxSpec`.
`nimbus-server` persists the validated carrier through the existing
Engine-owned saga adapter.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| E1 | `nimbus-workloads` owns one `WorkloadExecutableIntent` and one content-digest type; no duplicate carrier exists. |
| E2 | The wire carries exact format version `1`, the closed `sandbox_spec_canonical_json_v1` encoding, bounded UTF-8 canonical content, and a domain-separated SHA-256 content digest. |
| E3 | Empty content and content above `1,048,576` bytes reject at construction and deserialization. |
| E4 | Missing, duplicate, unknown, wrong-version, unknown-encoding, and crossed-digest fields reject without producing a carrier. |
| E5 | `Debug` reports format, encoding, byte length, and digest but never executable content or environment values. |
| E6 | A compute-owned codec is the only product `SandboxSpec` encoder/decoder for this carrier. |
| E7 | The compute encoder emits exact compact `serde_json` bytes; decode must deserialize and reproduce byte-identical content, so duplicate, unknown, default-expanded, or otherwise noncanonical inner JSON rejects. |
| E8 | Encode/decode preserves every `SandboxSpec` field, including tenant, owner, backend, root source, process, resources, lifecycle, ports, mounts, and egress. |
| E9 | `WorkloadSagaIntent::new` accepts executable content and derives `WorkloadDesiredDigest`; no caller supplies that digest. |
| E10 | The desired digest binds kind, desired state, generation, executable envelope, complete network intent, activation, publication, and admission evidence in one domain-separated canonical payload. |
| E11 | Deserialization recomputes the desired digest and rejects any crossed field before a record exists. |
| E12 | Equal-generation replay remains byte-stable; changing any E10 field changes the desired digest and produces an exact conflict. |
| E13 | Queued-successor validation retains the exact executable envelope and rejects a crossed successor before transition. |
| E14 | The server physical record contains one required `executable` object and no flattened executable tuple or cache reference. |
| E15 | A genuinely fresh process reopens only the Engine root and reconstructs byte-identical carrier content, content digest, desired digest, and decoded `SandboxSpec` before effects. |
| E16 | Missing, unknown, duplicate, crossed, oversized, and digest-divergent physical content fails before store mutation or provider effects; the prior record stays byte-identical. |
| E17 | Existing saga-v2 is changed cleanly in place; there is no compatibility decoder, optional executable, legacy cache, feature flag, or migration shim. |
| E18 | `nimbus-workloads` gains no `nimbus-sandbox` edge, and `nimbus-network -> nimbus-core` remains its sole workspace edge. |
| E19 | Static verification rejects a missing carrier, caller-supplied desired digest, optional physical field, compatibility path, sandbox dependency, provider effect, missing fresh-process proof, or unclassified product-path change. |
| E20 | Focused behavior, affected crate suites, dependency/effect scans, format, Clippy, docs, and exactly one candidate-complete structured review pass with exact evidence. |

## Fail-Before Source Census

| Concern | Current result |
| --- | --- |
| Product `WorkloadSagaIntent::new` caller | `0`; the two product matches are `ConfirmedWorkloadSagaIntent::new` wrappers. |
| Test `WorkloadSagaIntent::new` callers | `11` helper/case sites across workloads, compute, and server. |
| Executable field in `WorkloadSagaIntent` | `0`. |
| Executable physical field or schema entry | `0`. |
| Compute executable carrier codec | `0`. |
| Caller-supplied desired digest | Every current intent constructor accepts one. |
| Current physical shape | `17` required fields plus `2` optional fields. |
| Current saga format | Version `2`; NNC6.3a changes the v2 shape in place because Nimbus is pre-launch. |
| `nimbus-workloads -> nimbus-sandbox` | `0`, and it must remain zero. |
| Product provision caller or provider effect in this item | `0`, and it must remain zero. |

## Implemented Source Census

| Concern | Candidate result |
| --- | --- |
| Portable carrier authority | One `WorkloadExecutableIntent` and one domain-separated `WorkloadExecutableContentDigest`, both exported by `nimbus-workloads`. |
| Product carrier construction | One call in the compute-owned `encode_sandbox_spec`; all other calls are fixtures or corruption tests. |
| Desired-digest authority | One internal `derive_desired_digest`; no constructor accepts a caller digest. |
| Physical shape | `18` required fields plus `2` optional fields; exact schema has `20` fields and one required `executable` object. |
| Fresh-process authority | The recovery child opens only `Engine::new(state_root)`, loads the saga, then decodes the retained executable. No record, content, or snapshot crosses the process boundary. |
| Dependency profile | `nimbus-network -> nimbus-core`; `nimbus-workloads` has no `nimbus-sandbox` dependency. |
| Provider effects | Zero socket, sandbox-provider, network-provider, service-start, or network-apply effects in the new carrier/codec/recovery path. |
| Complexity | `saga.rs` is `1,441` lines; executable carrier `138`; compute codec `55`; durability proof `462`. No file crosses a modularity threshold. |

## Target Value Model

```text
WorkloadExecutableIntent
  formatVersion = 1
  encoding = sandbox_spec_canonical_json_v1
  content = compact canonical JSON UTF-8 string, 1..=1_048_576 bytes
  contentDigest = SHA-256(domain, exact content bytes)

WorkloadSagaIntent
  kind
  desiredState
  generation
  desiredDigest = SHA-256(domain, all fields below except itself)
  executable = WorkloadExecutableIntent
  network = complete WorkloadNetworkIntent
  activation
  publication
  admission
```

The carrier validates only portable envelope invariants. It does not import or
interpret `SandboxSpec`. Compute converts the typed value as follows:

```text
SandboxSpec
  -> serde_json::to_vec
  -> WorkloadExecutableIntent::new
  -> durable saga

durable saga
  -> WorkloadExecutableIntent validation
  -> serde_json::from_slice::<SandboxSpec>
  -> serde_json::to_vec(decoded) == original content
  -> SandboxSpec
```

The final equality check is the strict inner-content gate. A decoder that
silently ignores an unknown key or accepts a duplicate/default-expanded form
cannot bless different bytes as canonical desired state.

## Desired Digest Contract

The closed digest derives inside `WorkloadSagaIntent`.

Callers never provide the digest.

The digest includes admission evidence in addition to the fields named by
NNC6.3.

This prevents a crossed tenant decision, workload UID, or node assignment from
retaining a valid digest.

The canonical payload uses fixed struct-field order and validated nested
values. No map with unstable iteration order enters the digest input. The
digest domain advances for the clean-breaking closed semantic.

## Physical Durability Contract

The server codec adds one required `executable` object. The physical shape
becomes `18` required plus `2` optional fields, and the exact table schema
becomes `20` fields. The codec copies the value from
`activeIntent.executable`. A successor remains nested in `successorIntent` and
uses the same strict portable decoder.

Malformed documents fail `decode_workload_saga_record` before a
`WorkloadSagaRecord` exists. An Engine execution unit injects each representable
physical case in the corruption matrix. The installed schema rejects a missing
required object without changing the valid document, indexes, or journal. The
test first persists nested corruptions that satisfy the shallow object schema.
The real Engine-backed store then returns `Corrupt`. It does not change the
exact document, any of its four index projections, or the durable journal.

A `Document` uses a map and therefore cannot represent duplicate object keys.
The duplicate-envelope proof runs against raw portable JSON before a carrier
exists. The physical-store matrix covers every corruption class the map can
represent. It tests missing and unknown fields, crossed content and digest, and
oversized content with a recomputed digest. It also crosses an independently
valid executable envelope with a stale desired digest.

## Failure Matrix

| Failure | Required outcome |
| --- | --- |
| Missing envelope field | Typed portable or store corruption error. No record or effect. |
| Unknown envelope field | Strict rejection. No record or effect. |
| Duplicate envelope field | Strict rejection. No record or effect. |
| Unsupported format or encoding | Strict rejection. No compatibility fallback. |
| Empty or oversized content | Reject before digest acceptance or persistence. |
| Crossed content digest | Reject before typed decode or persistence. |
| Noncanonical inner JSON | Compute decode rejects after exact re-encode comparison. |
| Crossed desired digest | Intent/record decode rejects before transition. |
| Equal-generation different executable | `EqualGenerationConflict`. Current record unchanged. |
| Crossed queued successor | Reject before withdrawal/successor transition. |
| Fresh-process recovery | Exact carrier, digests, and `SandboxSpec`. No snapshot handoff. |
| Debug/error path | Content and environment values absent from diagnostics. |

## Path Boundary

NNC6.3a may edit:

- `crates/nimbus-workloads/src/saga.rs` and a concept-owned executable child.
- Workloads saga/store tests and exports.
- One concept-owned compute codec plus its tests and module export.
- The server saga codec, exact schema, and saga-store tests.
- One NNC6.3a static contract and the aggregate verifier.
- This proof, the canonical plan, and the plan routing index.

It may not edit sandbox provider effects, services activation, CLI/Compose,
Machine API, node backends, Cloud Functions, proxy/egress, system projection,
cluster transport, or `nimbus-network` product code.

The only path-boundary exception is test composition. The full server suite
showed that one saga test opened a second `LocalNetworkManager`. The router's
process-wide test manager was still live. The test now reuses the router-owned
shared fixture through a `pub(crate)` test-only accessor. No product
construction, network semantics, or provider effect changed.

## Acceptance Disposition

| Criteria | Status | Proof |
| --- | --- | --- |
| E1-E5 | `green` | Carrier unit suite proves single ownership, exact v1 encoding, bounded content, strict wire failures, domain digest, and redacted `Debug`. |
| E6-E8 | `green` | Compute codec round-trip preserves every `SandboxSpec` field; whitespace, unknown, duplicate, and default-expanded aliases fail the strict re-encode gate. |
| E9-E12 | `green` | Constructor derives the v2-domain desired digest over the fixed eight-field payload; crossed executable/admission values fail and equal-generation divergence conflicts. |
| E13 | `green` | Queued successor retains exact carrier bytes/digest; a crossed successor fails deserialization before transition or promotion. |
| E14-E16 | `green` | Required physical object, byte-exact fresh-process Engine recovery, redacted diagnostics, and the no-mutation corruption matrix pass. Duplicate physical keys are rejected at the raw portable boundary as explained above. |
| E17-E19 | `green` | No compatibility path, optional field, cache, provider effect, sandbox dependency, or unclassified path exists; NNCV031 is `25/25`, its mutation suite `13/13`, and the live aggregate `32/32`. |
| E20 | `green` | Focused/full behavior, seams, strict Clippy, format/diff, proof lint, docs/site, and exact retained-plus-additive aggregate `241/241` are green. The sole full and sole narrow reviews are dispositioned. No further review is warranted. |

## Evidence Ledger

| Checkpoint | Evidence |
| --- | --- |
| Durable base | NNC6.3 commit `81e4f2f9c`; owner worktree clean; post-commit verifier `31/31`; original checkout unchanged. |
| Current census | Exact `rg -n` and Cargo metadata results recorded above. |
| Acceptance freeze | E1-E20 and the failure matrix are frozen before product edits. |
| Fail-before | Historical NNCV030 remains green at `10/10` and `13/13`. The new direct executable contract exits `1` and names every absent carrier/codec/physical/process/test seam. Aggregate verification is exactly `31 passed, 1 failed`, with NNCV031 as the sole red condition. Bash syntax and ShellCheck pass for the new contract and the corrected historical source-range owner. |
| Implementation | The workloads-owned strict carrier, compute-only `SandboxSpec` codec, internally closed desired digest, exact successor retention, required server physical object, and distinct-process Engine recovery are implemented. A directly related server-test composition collision now reuses the router's process-wide manager. |
| Focused behavior | Compute codec and redacted-error matrix `2/2`; exact successor crossing and nested strictness `1/1`; executable Debug redaction `1/1`; real Engine/store physical corruption/no-mutation matrix `1/1`; fresh-process executable recovery remains covered by the full server suite. |
| Affected behavior | `nimbus-workloads` `106/106`; `nimbus-compute` `122/122` with one declared ignore; `nimbus-server` `549/549` with 30 declared ignores. |
| Static and seams | Direct NNCV031 `25/25`; adversarial mutations `13/13`; live aggregate `32/32`; Cargo metadata proves `network -> core` and no `workloads -> sandbox`; provider-effect scan is empty; `cargo fmt --all`, `git diff --check`, and owner manual audit pass. The verifier now includes staged paths, mutates the real Debug implementation, and fails a relaxed legacy-format predicate closed. |
| Aggregate adversarial proof | The bounded prefix passed the exact first `178` cases through NNCV026 `direct-cli-reconcile`. The disjoint continuation passed the prior remaining `62/62`: ten NNCV026 cases, NNCV027 `10/10`, NNCV028 `7/7`, NNCV029 `10/10`, NNCV030 `13/13`, and the original NNCV031 `12/12`. The additive strict legacy-format mutation passes `1/1`, so the complete declared contract is `178 + 62 + 1 = 241/241` with zero failed assertion. |
| Quality and docs | Strict all-target/all-feature Clippy passes for workloads, compute, and server. Proof lint passes one file with zero diagnostics. Bash syntax and scoped ShellCheck pass. Docs pass `108` pages and the site passes `17/17`. |
| Candidate identity | Sole full review inspected exact 30-path staged tree `567401c569424243749186c01fe5097e32fb0401`; complete binary patch SHA-256 `c9a68a7379b0b0638c4e4c16f5c7488f7a237cfac0f84ea241bd7a94f1bc3c99`. Sole narrow review inspected corrected tree `fdbc2e6d8484eeddf253e18579ecf0cd821e0ad1`; complete binary patch SHA-256 `5601df1dce4ae29621133c353f73210b39cb0086419bff998d48ab2d4fc6fac0`. Only ledger-only closeout wording differs at commit. |
| Candidate review | Sole complete-item GPT-5.6 Sol/xhigh/fast review thread `019fc43f-afaa-79b1-8e7d-68175eb8d694` returned five P2 and three P3 findings at confidence `0.98`; seven are accepted/corrected and one is source-rejected. Sole narrow correction thread `019fc45c-f0b9-7e62-abc3-e2c6822bb47b` validated all executable corrections and returned one accepted ledger-only P3 at confidence `0.98`. No further review is warranted. |
| Final commit | This carrier, codec, store/schema proof, migrated fixtures, verifier, plan, proof, and routing checkpoint commit together as the NNC6.3a item. Resolve the exact durable commit with `git log -1 --format=%H -- docs/private/plans/proof/nimbus-network-control-plane/nnc6.3a-strict-executable-carrier.md`; the NNC6.3b checkpoint promotes that hash on its first edit. |

## Structured Review Disposition

The complete-item review ran once, after the written acceptance matrix was
green. It inspected the exact candidate identity recorded above. The following
table records each disposition:

| Finding | Priority | Disposition and proof |
| --- | --- | --- |
| Decode diagnostics retain secret-bearing serde errors | P2 | Accepted. `Decode` is now an opaque unit error; invalid enum content containing `NNC63A_SECRET` is rejected and absent from `Debug` and `Display`. |
| Nested successor accepts unknown fields | P2 | Source-rejected. `WorkloadSagaIntentWire` already has `#[serde(deny_unknown_fields)]`; an exact nested `compatibilityCache` regression now proves rejection. |
| Static path census omits staged files | P2 | Accepted. NNCV031 unions committed, working, staged, and untracked paths before applying its frozen allowlist. |
| Physical corruption test does not exercise the mutation path | P2 | Accepted. The matrix now uses the installed schema and real Engine execution-unit/store operations, then compares the exact physical document, four index projections, and journal. |
| Compatibility-path acceptance lacks a mutation | P2 | Accepted. A mutation relaxing exact version equality to a future-only rejection fails with the expected diagnostic. |
| Debug mutation only appends a second implementation | P3 | Accepted. The mutation rewrites the real Debug field to canonical content, and the verifier inspects that implementation body for the leak. |
| Codec fixture exercises only default egress | P3 | Accepted. One HTTPS artifact-registry rule with method and path constraints round-trips exactly; default-expanded noncanonical content remains rejected. |
| Recovery ledger claims nothing is staged | P3 | Accepted. The plan now distinguishes the staged reviewed candidate from dirty correction edits and names the exact next freeze/review action. |

Because accepted findings changed executable code, the cadence permitted one
narrow Sol/xhigh/fast correction review after every affected proof was green.
Thread `019fc45c-f0b9-7e62-abc3-e2c6822bb47b` validated the executable-codec,
strictness, staged census, Engine/store proof, verifier mutation, and fixture
corrections as complete and non-vacuous. It found one P3: the recovery header
still described the corrected staged candidate as dirty and awaiting restage.
The header now reports the staged state and exact reviewed identity. This
ledger-only correction does not change executable code or require another
review.
