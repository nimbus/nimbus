# NNC7.2 Service Endpoint Generation

Status: `done`

## Outcome

Preserve service-owned logical naming and readiness while service resolution
carries the stable endpoint identity and generation that `nimbus-network`
already owns. A stale endpoint generation must fail closed without moving name,
readiness, policy, socket, or provider-effect authority.

## Initial recovery checkpoint

| Field | Value |
| --- | --- |
| Dependency | NNC7.1a is complete at `43f07a5b5883e8d60ff2e1f38b8af516256482f3`. |
| Current scope | Candidate-frozen portable endpoint-handle carrier plus the accepted withdrawal identity-fence correction. |
| Dirty product paths | Nineteen NNC7.2-owned paths: seventeen Rust product/test paths plus this proof and the canonical plan. |
| Owned paths | Network endpoint vocabulary/export; compute workload projection; services catalog/projection/registry; directly affected compute, server, and CLI fixtures; this proof and the concise plan checkpoint. |
| Forbidden paths and seams | No `nimbus-network` effect, logical name provider, tenant policy, ingress transport, projection work from NNC7.4-NNC7.5, or machine/sandbox status work from NNC7.3. |
| Acceptance | Service resolution tests remain services-owned. A deterministic stale endpoint-generation test fails before correction and passes after the exact consumer carries and authenticates stable identity plus generation. |
| Last green | G1-G8 are green after the accepted review correction. Services passes `101/101`, compute passes `469` with one ignore, server and CLI test targets compile, workspace check and strict quality pass, and architecture passes `36/36`. |
| Next action | Commit this completed item and continue the NNC7.3 read-only audit. |
| Blocker | none |

## Frozen seam

The audit found one exact information-loss path. Compute authenticates ingress
observations against the retained plan ID, digest, endpoint ID, listener ID,
lease ID, generation, lifetime, address, and provenance. It then reduces each
result to an address-only `PublishedEndpoint`. `ServiceManager` stores that
address-only sandbox handle, and both logical lookup paths lower it directly to
the runtime binding. Stable endpoint identity and generation do not survive the
compute-to-services projection.

The target seam is:

1. `nimbus-network` owns a transport-free `PublishedEndpointHandle` composed
   of `PublishedEndpointId`, `NetworkResourceGeneration`, and the existing
   observed `PublishedEndpoint` location.
2. Compute constructs this handle only after the existing exact ingress
   validation passes. It carries the handles through `WorkloadObservedProjection`
   without granting services provider-effect authority.
3. `nimbus-services` stores the handles with its service observation. It
   requires each handle generation to equal the exact workload execution
   generation. It rejects stale or crossed input before mutation. Logical name
   and readiness fencing remain authoritative.
4. `ServiceInstanceCatalog` returns a validated services-owned instance
   observation rather than an address-only sandbox handle. Both snapshot and
   exact lookup therefore consume the same stable handles.
5. The runtime adapter lowers a validated service observation to the existing
   address/protocol payload. `nimbus-runtime` gains no workspace dependency.
   It does not receive control-plane identity.

This is one earned portable handle with two current service-resolution
consumers and the named NNC7.3 sandbox/machine-status consumer. It does not add
a provider trait or move naming, readiness, sockets, forwarding, or policy.

## Frozen acceptance matrix

| ID | Requirement | Proof |
| --- | --- | --- |
| G1 | Stable endpoint ID, generation, and observed location stay one portable network-owned value. | Network construction/accessor/wire tests. |
| G2 | Exact compute ingress validation produces the portable handles without weakening any existing plan, bind, or provenance check. | Existing workload-projection rejection matrix plus handle assertions. |
| G3 | A service projection whose endpoint generation is older than its exact execution fails before mutation. | Deterministic services-owned fail-before and corrected test. |
| G4 | Crossed endpoint identity or handle/location mismatch fails closed; exact replay is idempotent. | Services-owned projection tests. |
| G5 | Snapshot and exact logical lookup use the same validated instance observation and retain service-owned readiness/withdrawal fences. | Services registry and manager resolution tests. |
| G6 | Address-only catalog input cannot become a runtime binding. | `ServiceInstanceCatalog` type substitution and compile/source census. |
| G7 | Runtime payload behavior stays unchanged and `nimbus-runtime` keeps zero workspace dependencies. | Runtime/service focused tests and metadata/static checks. |
| G8 | `nimbus-network` remains transport-free and depends only on `nimbus-core`. | Dependency/effect verifier. |

Owned product paths include the endpoint vocabulary and export in
`nimbus-network`. They also include the compute workload projection and its
tests. The services catalog, projection, registry, and tests are in scope.
Directly affected compute, server, and CLI fixtures are in scope. This item
does not own unrelated lifecycle, transport, provider, system projection,
machine-status, or runtime implementation paths.

## Fail-before

`cargo test -q -p nimbus-services
service_projection_rejects_stale_endpoint_generation_before_mutation --
--nocapture` ran one test and failed one. The uncorrected carrier accepted
network generation `0` for exact execution generation `1` and inserted the
stale service observation. This result proves the missing services-owned
rejection. It is not a compile-only red.

## Candidate acceptance

| ID | Result | Evidence |
| --- | --- | --- |
| G1 | `pass` | The network handle construction, accessor, and wire test passes. Full network passes `275` with one declared ignore. |
| G2 | `pass` | Compute workload projection passes `12/12`. Full compute passes `469` with one declared ignore. Existing plan, bind, lease, generation, address, and provenance rejections remain green. |
| G3 | `pass` | The real fail-before is `0/1`. The same stale-generation test passes `1/1` after correction and leaves no observed service state. |
| G4 | `pass` | Services rejects crossed stable identity and mismatched location. It retains the authenticated nonempty identity fence across withdrawal, rejects a crossed republish, and permits the original identity to return. Exact replay stays idempotent. Full services passes `101/101`. |
| G5 | `pass` | Snapshot and exact lookup consume `ServiceInstanceObservation`. The exact lookup test proves the same handle vector survives resolution. Existing readiness and withdrawal tests pass. |
| G6 | `pass` | `ServiceInstanceCatalog` returns `ServiceInstanceObservation`. The old address-only lowering symbol has zero source matches. |
| G7 | `pass` | Runtime bindings retain their existing payload. Full server passes `685` with `35` declared ignores. Full CLI passes `1,007` with four declared ignores. Metadata reports zero workspace dependencies for `nimbus-runtime`. |
| G8 | `pass` | The live verifier passes `36/36`. Metadata reports `nimbus-network -> nimbus-core` as its only workspace edge. |

The full server result consists of `658` unit tests and `27` integration
tests. After the final API grouping correction, the affected server retirement
set passes `10/10`. Compute retirement passes `33/33`, and full compute passes
again at `469` with one declared ignore. The correction moves the already
validated handle/location pair into one `ServiceInstanceObservation` argument.
It does not change runtime payload behavior.

Quality gates pass:

- `cargo check --workspace --all-targets -q`.
- Strict all-target Clippy for network, services, compute, server, and CLI.
- Warning-denied Rustdoc for the same five crates.
- `cargo fmt --all --check` and `git diff --check`.
- The live network-control-plane verifier at `36/36`.
- Strict proof lint with zero diagnostics.
- docs at `108` pages and the site verifier at `17/17`.

The compiler emitted only the existing vendored Brotli warnings. No Nimbus
diagnostic survived the strict gates. The item adds no effect, socket, policy,
transport, runtime dependency, machine-status, or system-projection authority.

## Item review and accepted correction

The complete pre-review item used staged tree
`362790d1b58880465ac623853927172e622ce349` and binary patch SHA-256
`2df8b4d41dfc0475fc6173fdd7f9126bac366c89972f0ff52e39acd25ffcf2b3`.
It contained nineteen paths, including seventeen Rust paths, with no unstaged
change. One manual `nimbus-autoreview` pass used GPT-5.6 Sol, `xhigh`
reasoning, and fast service tier. TruffleHog was clean.

The review reported one P2 finding at confidence `0.93`. A same-generation
withdrawal erased the only stable endpoint identity set. A later crossed
republish could then bypass G4. We accepted the finding.

The deterministic regression
`service_projection_retains_endpoint_identity_fence_across_withdrawal` failed
`0/1` before correction. `ServiceDefinitionObservation` now retains the last
authenticated nonempty endpoint identity set for its exact execution
generation. Empty withdrawal keeps that fence. A crossed republish fails
without mutation. The original identity can republish. A later execution
generation can establish a new fence.

The regression passes `1/1`. Full services passes `101/101`. Full compute
passes `469` with one declared ignore. Server and CLI test targets compile.
Workspace all-target check, strict affected Clippy and Rustdoc, format and
diff checks, and the live architecture verifier at `36/36` pass.

The executable correction authorizes one narrow correction review. The
completed item does not need another full review.

The corrected narrow-review input used staged tree
`42cdf9827b42515467ee1ffd0825609984bbf229` and binary patch SHA-256
`70ef5e0c5209bbd31ffd7b2ca36696360065b9795b53624821b74458eb156516`.
It contained nineteen paths, including seventeen Rust paths, with no unstaged
change. One GPT-5.6 Sol, `xhigh`, fast review pass reported no accepted or
actionable finding at confidence `0.96`. TruffleHog was clean. Review cadence
permits no further review.

NNC7.2 meets G1-G8. The commit that contains this proof and its completed
ledger row is the durable item checkpoint.
