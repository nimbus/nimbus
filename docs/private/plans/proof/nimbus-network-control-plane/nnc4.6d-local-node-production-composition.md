# NNC4.6d Local-Node Production Composition

Status: `complete; exact item commit payload frozen`

Owner: NNC4.6d in
`docs/private/plans/nimbus-network-control-plane-plan.md`.

## Outcome

NNC4.6d makes the CLI the outer composition owner for a local Nimbus process
that combines server ingress with a host-managed container or krun attachment.
It resolves one logical-node root, claims one staged `LocalNetworkManager`,
derives every portable authority from that claim, obtains truthful reports
from the existing effect owners, freezes exactly the complete compatible
attachment/ingress bundles, and only then permits listener or provider effects.

This item does not make `nimbus-network` an application bootstrap crate.
`nimbus-cli` owns the composition because it already depends on
`nimbus-network`, `nimbus-server`, `nimbus-sandbox`, and `nimbus-services`.
The lower crate remains effect-free and dependent only on `nimbus-core`.

## Prospective Scope Split

The read-only audit found two different units of value before executable work:

| Item | Coherent authority/lifecycle |
| --- | --- |
| NNC4.6d | CLI start/dev/standalone Compose, server main/sibling listeners, local OCI process, and exact complete registry freeze. Dev prebinding and start consumption must share the same staged claim, so these paths are one unit. |
| NNC4.6g | Standalone KV root parity, retained manager-derived authority, server/KV conflict, honestly empty KV-only registry, and post-bind observability. KV has no local attachment source and does not participate in start's registry composition. |

NNC4.6g is a later canonical item with its own fail-before proof, candidate
freeze, one structured review, and one commit. It is not a review chunk.

## Current Ownership And Defects

```text
start config ---------> raw control-data path --------------------+
dev wire plan --------> raw path -> prebound server authority ----+--> later replacement
server ServeOptions --> raw engine/path -> reopen port authority -+

Compose project ------> project workload root == network root
                         -> concrete backend erased
                         -> no truthful attachment report retained

result: no production LocalNetworkManager and no production registry freeze
```

| Current owner/site | Audited behavior | Defect NNC4.6d closes |
| --- | --- | --- |
| `nimbus-operator/paths.rs` | Resolves stable platform-local auth, discovery, and audit paths. | No typed logical-node network-root policy exists yet. |
| `nimbus-cli/start/config.rs` | Resolves engine/control data using flag/environment/config/data precedence. | Workload/data roots are currently reused as network authority even though they do not identify the OS node. |
| `nimbus-cli/dev/plan.rs` and `dev/wire.rs` | Reserve and bind sibling listeners before `run_start_command`. | Dev opens a raw-path authority before start can own a manager. |
| `nimbus-cli/start/boot.rs` | Creates `ServeOptions`, retargets its root, then may replace its authority from prebound dev listeners. | Builder order can split or silently replace durable authority. |
| `nimbus-server/construction.rs` | Owns main/sibling listener composition and an honest effect-free ingress report. | Manager presence and exact frozen evidence are not required before prepare/adopt/serve. |
| `nimbus-server/listener_lease.rs` | Reopens `LocalPortLeaseAuthority` from a stored path during each preparation/adoption. | Alias retargeting and divergent roots can redirect later operations; ownership checks name only a server incarnation. |
| `nimbus-cli/compose/project.rs` | Uses a project-local krun root for both workload and network state. | Separate projects mint separate node-network authorities. |
| `nimbus-cli/compose/execution.rs` | Erases the concrete backend behind `Arc<dyn SandboxBackend>`. | The source-owned attachment report is lost before upper composition can freeze it. |
| `nimbus-sandbox/backends/capabilities.rs` | Reports a conservative attachment only after healthy reconciliation and correct machine mode. | Production never collects this truthful report. |
| `nimbus-network/capability_registry.rs` | Accepts immutable complete attachment+ingress bundles only. | Production constructs no exact bundle and freezes no manager. |

## Target Ownership And Ordering

```text
typed logical-node root
        |
        v
LocalNetworkManagerBootstrap --derive--> LocalNetworkAuthority
        |                                  |             |
        |                                  |             +--> server listener authority
        |                                  +--> one OciNetworkProcess
        |                                                |
        |                                                +--> local backend/report
        |
        +-- exact server ingress report + exact attachment report
        |
        v
NetworkCapabilityRegistry::new(complete compatible bundles only)
        |
        v
bootstrap.freeze(registry) -> retained LocalNetworkManager
        |
        v
provider bind / Netavark / serve effects remain in existing owners
```

Mandatory effect order:

1. Resolve and authenticate one typed logical-node root through the
   operator-owned platform policy.
2. Claim one staged manager before dev preparation or any durable/effectful
   listener work.
3. Derive and retain one `LocalNetworkAuthority`.
4. Construct the local OCI process and concrete backend without provider
   effects.
5. Finalize TLS and desired server adapters.
6. Obtain the exact source-owned attachment and ingress reports.
7. Build only complete, compatible bundles and consume the one-shot freeze.
8. Prepare durable port leases.
9. Bind/adopt real sockets in the existing CLI/server owner.
10. Reconcile attachment/provider effects in the existing sandbox owner.
11. Publish observed listener state only after activation.

The manager, authority, OCI process, prepared backend, registry, and live
listeners must remain retained for the process lifetime. Desired plan,
durable lease/provider handle, and observed status remain distinct.

## Frozen Architecture Decisions

1. `nimbus-operator` owns the typed platform path policy; `nimbus-cli` owns a
   small concept module for staged local-node composition; `start/boot.rs`
   remains a thin orchestration root.
2. The root type means the network authority of one logical OS node. Its
   default is stable across projects and working directories. A dedicated
   explicit network-root flag/environment/config value may select a different
   realm; engine data, control data, project workload roots, and dev state are
   never silently promoted to node authority.
3. Dev creates the staged composition before prebinding and transfers that
   exact in-process authority to start. Start never reopens it.
4. Server main and every sibling listener share one manager-derived authority.
   `WireProtocolAdapter` remains unaware of network management.
5. Production server code does not reopen `LocalPortLeaseAuthority` from a
   path. Explicit test/embedder reconstruction is separately named and is
   classified later by NNC4.6f.
6. Prebound ownership authenticates both manager provenance and server
   incarnation before transferring sockets.
7. The local concrete backend is retained through capability collection before
   trait-object erasure.
8. Only actual, healthy, source-owned reports may enter the registry.
   Attachment-only, ingress-only, crossed, stale, unavailable, or failed
   sources produce no bundle.
9. Server-only, Compose-attachment-only, forwarded-machine, and unavailable
   local shapes freeze an empty registry. Empty is honest; no partial provider
   is fabricated.
10. TLS is finalized before the ingress report is collected. Frozen evidence
    must equal the exact server report used for the subsequent serve path.
11. Socket binding, Axum/router/protocol bytes, TLS certificates, Netavark,
    nftables, gvproxy, and system projections remain with their present owners.
12. NNC7.1a still owns structured listener-group atomic unwind and supervision.
    NNC4.6d preserves the existing NNC3.5 lifecycle and does not consume that
    expected-red owner.

## Frozen Acceptance Criteria

| ID | Criterion |
| --- | --- |
| D1 | One operator-owned typed logical-node-root resolver feeds start, dev, and standalone Compose. Dedicated explicit flag, environment, config, Linux XDG, macOS Application Support, Windows Local AppData, lexical alias, symlink alias, and default cases have deterministic tests. The default is invariant across project working directories; engine/control/project/dev roots do not affect it. |
| D2 | Production claims one `LocalNetworkManagerBootstrap` before dev prebinding, listener durable work, socket bind, attachment reconciliation, or positive capability evidence. A second same/alias/divergent composition fails with typed active/attempted evidence before attempted-root mutation or any effect. |
| D3 | Retargeting a symlink after bootstrap cannot redirect later server, port, Compose, or backend work. All operations retain the authenticated canonical authority and leave the retargeted foreign root unchanged. |
| D4 | Dev prebound MongoDB, DynamoDB, and S3 listeners plus the later main listener carry the same manager provenance and primitive authority. A divergent start root fails before any new bind, guard, projection, spawn, or provider effect and settles the held prebound bundle under its original authority. |
| D5 | `ServeOptions` and `PreboundServerListeners` cannot be constructed for production from a raw path. Every main, sibling, and external-main prepare/adopt path retains the manager-derived authority; no production server site calls `LocalPortLeaseAuthority::open`. |
| D6 | Prebound transfer authenticates manager provenance before server incarnation/name/address. A bundle from authority A cannot be combined with B regardless of former builder order, and rejection precedes all effects. |
| D7 | Distinct Compose project workload roots remain distinct, while their local container/krun configs use the exact pointer-shared OCI process and canonical node-network authority. No network-authority file is created below either project workload root. |
| D8 | Two project/server/sandbox consumers requesting the same exact host binding conflict in the durable authority before the losing binder or provider is invoked. The port remains kernel-bindable after a synthetic prepare-only loser, proving the losing effect did not run. |
| D9 | A healthy real local container/krun attachment report and the finalized real server ingress report form the exact complete bundle before effects. Selection returns those exact registrations and provider-neutral plan identity. |
| D10 | Failed startup reconciliation, wrong machine mode, unavailable local provider, TLS/report drift, incomplete source set, crossed source pair, or stale source evidence cannot freeze positive capability or reach listener/provider effects. Dropping the failed bootstrap permits deterministic reopen. |
| D11 | Server-only, attachment-only, forwarded-machine, and no-Compose shapes freeze exactly zero bundles. No placeholder attachment, ingress, DNS, name, forwarding, certificate, or provider capability is synthesized. |
| D12 | Production lifetime retention is mechanical: dropping temporary outer variables while the prepared/live composition remains cannot admit an independent manager; after final composition drop, deterministic reopen succeeds without durable mutation. |
| D13 | Existing NNC3.5 listener bytes, bind/adopt/activate/withdraw/release behavior, external-binder evidence, ambiguous cleanup fencing, and observed projection behavior remain green. NNC7.1a's named partial sibling-start expected red is neither weakened nor claimed complete. |
| D14 | Boundary and quality gates prove `nimbus-network -> nimbus-core` is still the sole workspace edge; no effect/transport/policy/naming/cluster dependency enters it; production raw-root/reopen and fabricated-provider scans are empty; affected tests, check, strict Clippy, warning-denied rustdoc, format/diff, verifier/self-test, docs/site, one candidate-frozen Sol/xhigh/fast review, ledger closeout, and one exact item commit all pass. |

## Expected-Red Packet

Expected-red tests are added before the production behavior they demand:

| Packet | Fail-before behavior | Corrected proof |
| --- | --- | --- |
| R1 manager-required server construction | Production construction still accepts a raw root or cannot accept the manager-derived authority. No socket is bound in the test. | Manager-derived construction is required and retains the claim. |
| R2 divergent prebound authority | A prebound bundle from A can be retargeted/replaced by B while retaining its server incarnation. | Typed authority mismatch occurs before socket/durable/observer effects; A settles exactly. |
| R3 alias retarget | A configured symlink retargets later server preparation to B. | Retained canonical handle continues on A; B is untouched. |
| R4 project-independent root and dev staged handoff | Two projects resolve different authority roots, dev prebind occurs before a manager exists, or start reopens a second authority. | One operator-owned root policy is invariant across projects; one staged composition crosses plan/prebind/start and owns all listeners. |
| R5 split Compose roots | Two project configs use their workload roots as separate network roots. | Workload roots differ; exact canonical network root and OCI process are shared. |
| R6 pre-effect conflict | Two projects or server/sandbox reach a bind/provider callback before conflict. | Durable typed conflict is returned while a sentinel effect counter remains zero. |
| R7 exact positive registry | Production cannot retain the concrete backend report or freezes no exact bundle. | Exact healthy attachment plus finalized ingress freezes and selects before effects. |
| R8 negative registry matrix | Partial/crossed/stale/failed/TLS-drift facts can appear positive or trigger effects. | Every named negative yields zero bundles or a deterministic typed refusal with zero effects. |
| R9 manager lifetime | Dropping a temporary bootstrap/manager permits a second composition while listeners/backend still exist. | Prepared/live owners retain authority; final drop alone permits reopen. |
| R10 static construction census | Production server/start/dev/Compose paths still open raw primitive authorities or independently resolve node roots. | Every site is manager-owned/derived; explicit reconstruction remains named for NNC4.6f. |

An expected-red compile failure counts only when it is caused by the missing
target seam. Privacy mistakes, stale imports, malformed fixtures, or unrelated
compile failures must be corrected before recording fail-before evidence.

## Behavioral And Failure Proof Matrix

| Scenario | Required observation |
| --- | --- |
| Same root and stable aliases | One composition; pointer-identical authority/process; no duplicate durable mutation. |
| Divergent missing or existing root | Typed active/attempted roots; attempted root absent or byte-identical; no effects. |
| Alias retarget after claim | All later durable records remain under original canonical root; foreign root untouched. |
| Dev conventional and provider-assigned siblings | Exact stable identities and shared authority; real sockets/adoption preserved. |
| Prebound authority mismatch | Rejected before main/sibling bind, guard, projection, spawn, registry claim, or provider call; held resources settle under source authority. |
| Two local projects | Distinct workload artifacts, one authority/process, deterministic pre-effect port conflict. |
| Healthy local attachment + server ingress | One exact complete bundle; selection succeeds before effect/observed evidence. |
| Attachment only or ingress only | Empty registry; no fabricated counterpart. |
| Crossed provider/realm/topology facts | Deterministic compatibility error or empty registry; no best-effort pairing. |
| Reconciliation failure or unavailable provider | No positive registration, no freeze side effect, no listener/provider effect, deterministic reopen after drop. |
| TLS false/true drift | Frozen report mismatch refuses serve before bind. |
| Outer temporary dropped | Prepared/live composition retains singleton claim. |
| Final composition dropped | Same canonical root reopens without durable mutation. |

## Acceptance Disposition

The D1-D14 contract is complete through its pre-review boundary. D14's final
review, ledger transition, and item commit are closeout actions rather than
inputs to review eligibility; every executable, behavioral, static, quality,
and documentation condition that the review consumes is green.

| ID | Status | Evidence |
| --- | --- | --- |
| D1 | PASS | `LocalNodeNetworkRoot` owns explicit/environment/config/platform resolution. Fourteen operator path tests cover stable defaults, platform locations, lexical and symlink aliases, overrides, and project/CWD independence. Start, dev, and standalone Compose expose and parse the dedicated root; the dev and Compose CLI suites pass 77/77 and 43/43. |
| D2 | PASS | `StagedLocalNetworkComposition` claims the singleton manager before local preparation. Same-root, alias, divergent-root, and typed duplicate-claim cases prove active/attempted evidence, zero attempted-root mutation, and deterministic reopen after the retained owner drops. |
| D3 | PASS | The Unix retarget regression proves a claimed alias remains pinned to the authenticated canonical root for the backend and listener paths while the retargeted foreign root remains untouched. Server integration independently proves the same property through final manager-derived listener drop. |
| D4 | PASS | Dev transfers one staged composition and exact prebound bundle into the real start path. `manager_derived_dev_bundle_and_main_share_one_primitive_authority` proves MongoDB, DynamoDB, S3, and main are the four exact active records with binding evidence. `divergent_prepared_start_root_settles_dev_listener_before_other_effects` proves a typed root mismatch settles the held bundle before any later startup effect. |
| D5 | PASS | Production `ServeOptions` construction requires `LocalNetworkAuthority`; raw-path construction is explicitly named for tests/embedders. Main, sibling, external-main, and retained-lifetime integration tests prove one manager-derived authority. The exact production-construction census rejects raw reopen sites. |
| D6 | PASS | Prebound adoption authenticates canonical path and manager provenance before incarnation/name/address. Divergent-authority integration refuses the handoff before main preparation, and original-authority cleanup remains exact. |
| D7 | PASS | Two distinct Compose workload roots retain separate workload artifacts while the prepared local composition supplies the same pointer-shared `OciNetworkProcess` and canonical node-network root. No durable authority is created beneath either workload root. |
| D8 | PASS | Durable exact-binding contention rejects the losing sandbox-shaped/server request with `AddrInUse` before a kernel/provider effect. The winner remains durable, the loser records truthful failure, and the requested port remains kernel-bindable after the synthetic prepare-only loser. |
| D9 | PASS | On Linux, the real local krun source plus finalized server ingress freezes and selects one exact complete bundle with the provider-neutral plan identity. The case is compiled by the all-target/all-feature gate on macOS; local execution is not claimed on this non-Linux host. |
| D10 | PASS | Tests cover unavailable source, wrong mode, incomplete/crossed evidence, finalized-ingress/TLS drift, incompatible second krun supernet/prefix configuration, and stale source context. Each refuses positive freeze or later effects and final drop permits deterministic reopen. |
| D11 | PASS | Generic empty, server-only, forwarded-container, and Linux attachment-only cases freeze exactly zero registrations. No DNS, name, forwarding, certificate, ingress, or attachment counterpart is synthesized. |
| D12 | PASS | Prepared CLI composition and live server bundles retain the manager independently of temporary outer variables. Duplicate claim remains fenced until final drop; reopen then succeeds against byte-identical durable state. |
| D13 | PASS | Listener lifecycle 14/14, construction 8/8 with the one NNC7.1a-owned case still ignored, capability 2/2, manager composition 4/4, targeted server corrections 7/7, and the full affected server suite 601/601 under the two exact inherited exclusions preserve NNC3.5 behavior and observed evidence. |
| D14 | PASS (pre-review) | All-target/all-feature check, strict `-D warnings` Clippy, warning-denied rustdoc, format, diff, exact `nimbus-network -> nimbus-core` metadata, raw-root/effect/composition scans, live verifier 16/16, adversarial self-test 51/51, docs 108 pages, and site 17/17 pass. The complete owned diff is now eligible to freeze for its sole full Sol/xhigh/fast item review. |

### Inherited Post-Merge Baseline Exceptions

The first unfiltered `nimbus-server` aggregate executed 603 tests: 601 passed,
2 failed, and 26 were skipped. Both failures reproduce individually at the
exact execution base containing PRs #238 and #239,
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`, in a disposable detached
worktree:

- `tests::local_server_security::deploy_admin_requires_local_admin_header_even_with_deploy_bearer`
  expects HTTP 200 but receives 400;
- `tests::runtime_owner_conformance::cloud_functions_passes_runtime_owner_lifecycle_conformance`
  expects the inner invocation to return 200 but receives 409.

Each baseline reproduction exits 101. The older local `main` pointer predates
the two trust-boundary merges and is not a valid comparison point. NNC4.6d
does not alter either auth owner, so its canonical server aggregate excludes
only those two exact tests and passes 601/601 with 28 skips. This is a scoped
baseline classification, not a weakened assertion or a claim that the auth
failures pass.

## Verification Evidence

| Gate | Result |
| --- | --- |
| Focused behavior | Operator root 14/14; dev 77/77; start 84/84; Compose 43/43; CLI composition 7/7 on macOS with two Linux-only cases compiled; server manager composition 4/4; listener 14/14; construction 8/8 plus one expected NNC7.1a ignore; capability 2/2; targeted server set 7/7; divergent-root cleanup 1/1; dev shared authority 1/1; durable pre-effect conflict 1/1; root-flag filter 2/2. |
| Full network/operator | `cargo nextest run -p nimbus-network -p nimbus-operator`: 242 passed, 0 skipped. |
| Full sandbox | `cargo nextest run -p nimbus-sandbox`: 722 passed, 24 skipped. |
| Full CLI | `cargo nextest run -p nimbus-cli`: 894 passed, 1 skipped; nextest reported one non-failing slow/leaky diagnostic. |
| Full server | Exact two-test baseline exclusion expression: 601 passed, 28 skipped. The unfiltered and baseline-reproduction evidence is recorded above. |
| Build/lint/docs | Affected all-target/all-feature check, strict affected Clippy with `-- -D warnings`, all-feature warning-denied rustdoc, `cargo fmt --all --check`, `git diff --check`, docs 108 pages, and site 17/17 pass. |
| Boundary verifier | Exact workspace metadata reports only `nimbus-core`; source bind census and exact 24-key NNC4.6d composition allowlist pass; aggregate verifier 16/16 and adversarial self-test 51/51 pass. |

## Structured Review Disposition

The sole full item review ran once over the 59-path frozen candidate with
GPT-5.6 Sol, xhigh reasoning, and fast service tier. Review thread
`019fadf3-f7a8-7171-9fa8-949319748feb` reported four findings and judged the
uncorrected patch incorrect at 0.97. All four are accepted as direct NNC4.6d
acceptance defects:

| Finding | Disposition and proof |
| --- | --- |
| P1 — activated systemd bind policy ran after forwarded-machine construction | Accepted. Forwarded service-manager construction can start the default VM, so the actual inherited bind is now acquired and validated before forwarded-service or machine-lifecycle construction. The new source-order gate failed before with `activated systemd bind policy must precede...` and passes after correction. |
| P1 — dev bypassed start-family config precedence | Accepted. Dev now resolves through the same start-family CLI > `NIMBUS_NETWORK_STATE_DIR` > `network.state_dir` > platform policy before claiming the manager. `dev_network_root_honors_discovered_start_config_before_claim` failed 0/1 against the reviewed code with the platform default instead of the configured root, then passed 1/1. |
| P2 — manager dropped before scheduler termination | Accepted. The prepared composition remains retained through the shutdown signal, scheduler join, first-boot task settlement, and engine quiescence. The new source-order gate failed before with `prepared local-network composition must remain retained through scheduler shutdown` and passes after correction. |
| P2 — POSIX absolute-path validation used host semantics | Accepted. Linux/macOS validation now recognizes POSIX leading-slash syntax independently of the compilation host; Windows keeps explicit drive/UNC rules. The target-platform syntax regression passes 1/1 and the complete operator/network suite passes 243/243. A Windows-target check was attempted but this macOS host lacks the Visual Studio CMake generator, `windows.h`, and Windows C SDK; no Windows execution claim is made. |

The accepted executable correction spans seven files; its pre-staging
executable SHA-256 is
`e62fae0f382234b6ccf8c9a0a7e504ea32e39431cb64095ed3f6173f92ce18c0`.
After correction, full CLI passes 895/895 with one declared skip;
network/operator passes 243/243; affected check, strict Clippy,
warning-denied rustdoc, live verifier 16/16, verifier self-test 51/51,
format, and diff pass. The cadence permits exactly one narrow correction
review focused on these four accepted defects.

The one permitted narrow correction review then ran with GPT-5.6 Sol, xhigh
reasoning, and fast service tier in thread
`019fae14-1935-7a32-ac53-5e9b745469ab`. It confirmed the systemd-effect
ordering, dev precedence, and shutdown retention corrections, and accepted one
P2 in the fourth correction: the target-independent Windows parser still
treated incomplete UNC forms such as `\\` and `\\server` as absolute. The
new positive/incomplete UNC matrix failed 0/1 against that reviewed correction,
then passed 1/1 after requiring a non-empty server/share pair. Full
network/operator returned to 243/243; affected check, strict Clippy,
warning-denied rustdoc, live verifier 16/16, format, and diff remain green.
This accepted finding is fully dispositioned. No third review runs: the
owner-mandated cadence permits one full item review and one narrow correction
review, and the final UNC change has an exact failing regression plus complete
affected proof.

## Owned Paths

Permitted production/test paths:

- `crates/nimbus-cli/src/network_composition.rs` and concept-owned tests;
- `crates/nimbus-operator/src/paths.rs` for the typed platform policy and its
  deterministic platform matrix;
- narrow integration in `crates/nimbus-cli/src/start/`, `dev/`, and `compose/`;
- `crates/nimbus-server/src/network_composition.rs` or a concept-owned
  listener-authority child;
- narrow integration in server construction/listener lifecycle and their
  concept-owned tests;
- a read-only exact registry lookup only if final server evidence validation
  proves it is necessary;
- this proof, the canonical plan, and routing index.

Forbidden:

- `nimbus-kv` and CLI KV implementation (NNC4.6g);
- machine host/guest authority (NNC4.6e);
- constructor/root census closure beyond touched-path expected red (NNC4.6f);
- listener-group atomic startup/supervision (NNC7.1a);
- cluster transport, tenant policy, service naming, proxy forwarding,
  certificate ownership, provider effects, or system projection ownership;
- new effects or upper-crate dependencies in `nimbus-network`.

## Modularity Disposition

| File | Audited size/risk | Required disposition |
| --- | --- | --- |
| `nimbus-cli/start/boot.rs` | About 1,214 lines; existing orchestration root. | Add only narrow calls; put state and invariants in `network_composition.rs`. |
| `nimbus-server/construction.rs` | About 1,340 lines with substantial inline tests. | Keep as a thin composition root; move manager/freeze validation to a concept owner. |
| `nimbus-server/listener_lease.rs` | About 1,593 lines; 1,500-1,999 justification band, with roughly 662 inline-test lines. | Do not add a second ownership story. Prefer moving the intact private tests to a child and retaining lifecycle code in the parent; otherwise record a concrete deep-lifecycle exception here before closeout. |
| Compose execution | Concrete backend is erased before report collection. | Return a prepared typed result containing the trait object, process retention, and optional source registration; do not add a god provider interface. |

## Verification Commands

Focused fail-before and corrected commands will be recorded with exact counts.
At minimum they cover:

- manager/root/alias/retarget tests in CLI/server composition;
- server listener lifecycle and construction suites;
- dev plan/adoption and start boot/listener suites;
- Compose project/execution/backend tests;
- server capability-registration tests;
- sandbox production-composition and network source-report tests;
- network manager and capability-registry tests.

Closeout additionally requires:

- full affected `nimbus-network`, `nimbus-sandbox`, `nimbus-server`, and
  `nimbus-cli` suites with exact pass/fail/ignored counts;
- affected all-target/all-feature check;
- strict affected Clippy and warning-denied rustdoc;
- `cargo fmt --all --check` and `git diff --check`;
- exact dependency/effect/raw-root/reopen/fabricated-provider scans;
- `bash scripts/verify-nimbus-network-control-plane.sh` plus its self-test when
  verifier code changes;
- `bash scripts/check-docs.sh`;
- `bash scripts/verify-nimbus-docs-site.sh`.

Only after D1-D14 and all pre-review gates are green is the executable diff
frozen and reviewed once with GPT-5.6 Sol, xhigh reasoning, fast mode. An
accepted executable defect permits one narrow correction review after its
focused proofs rerun. Proof/ledger wording, formatting, elapsed time, or
internal bundle chunking never trigger another review.

## Recovery Ledger

| Field | Value |
| --- | --- |
| Status | Complete. D1-D14 pass; both authorized review runs are fully dispositioned; final executable SHA-256 `bdd7536009ff2f205377690a814c3c852d02986dca18cc17032f1eae80fdf064` identifies the exact implementation payload. |
| Last green | NNC4.6c commit `d90199e94dfb722b1e80aa4e937cbbbf701d0364`, tree `1ce12ff27fcec13c393586e300b966d58368d263`. |
| Owned dirty paths | Canonical plan/routing and NNC4.6b truth-up; bind-census anchor refresh; this proof; `nimbus-operator` typed local-node root; `nimbus-cli` network composition plus narrow start/dev/standalone-Compose integration and directly related tests; `nimbus-sandbox` OCI configuration authentication visibility; `nimbus-server` retained authority/composition plus listener/construction/capability tests; structural composition-census output plus aggregate verifier integration. New concept owners are `crates/nimbus-cli/src/network_composition.rs`, `crates/nimbus-server/src/network_composition.rs`, `crates/nimbus-server/src/listener_lease/tests.rs`, `crates/nimbus-server/tests/network_manager_composition.rs`, and `scripts/verify-nimbus-network-composition-census.mjs`. Standalone KV and machine-realm changes remain absent. |
| Current evidence | D1-D14 are dispositioned above. Full aggregates: network/operator 242/0, sandbox 722/24, CLI 894/1, and server 601/28 under two exact execution-base exclusions that reproduce at `9c2d4f15`. Affected all-target check, strict Clippy, warning-denied rustdoc, exact dependency/census scans, live verifier 16/16, self-test 51/51, format/diff, docs 108, and site 17/17 pass. Two Linux-only positive/attachment-only cases compile locally but are not claimed executed on macOS. |
| Candidate identity | Pre-ledger-metadata staged tree `030fbc01af628f99d8e7901d754f74c7a0de2437`; pre-ledger-metadata full-diff SHA-256 `eaea8d9c8d245b139fea16d027982bd49faba8f02179f0459b64ea3af02a87a7`; frozen executable (`crates/` + `scripts/`) SHA-256 `a553219f0253703ffdbe892ed2c37da3e28984f4dc960db939b633f07dbede2f`; 59 staged paths; `git diff --cached --check` passes. Ledger/proof metadata changes do not alter the executable digest. |
| Corrected candidate identity | Pre-ledger-metadata staged tree `ca691a6251363341bf2fd9bf98842044f1397918`; corrected executable SHA-256 `99ad3aae08c0fb24bfbe7ab9bce29bccd55c1b02d9602261d44918330d926b27`; 59 staged paths; no unstaged paths; `git diff --cached --check` passes. |
| Final candidate identity | Narrow-review UNC correction SHA-256 `c84682cfaa0204b83ef73a564a0fd8383eee0dd3c467ec023821c0c2d8509953`; final executable SHA-256 `bdd7536009ff2f205377690a814c3c852d02986dca18cc17032f1eae80fdf064`; pre-ledger final staged tree `dd5e6545b47b7006e599d07739e5c760a0952348`. The subsequent recovery-header truth-up records the commit identity without making this proof self-referential. |
| Next | Commit this exact NNC4.6d payload, then begin NNC4.6g with a read-only standalone-KV seam audit. |
| Review | Full thread `019fadf3-f7a8-7171-9fa8-949319748feb`: four accepted and corrected findings. Narrow thread `019fae14-1935-7a32-ac53-5e9b745469ab`: three corrections confirmed and one incomplete-UNC P2 accepted, reproduced, and corrected. Both authorized reviews are fully dispositioned; no third review is warranted under the item cadence. |
| Blocker | None. |
