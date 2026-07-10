# Enterprise Crate Adoption Plan

Status: `proposed`
Owner: this plan
Created: 2026-07-07

## What This Plan Owns

The cross-workspace screen for places where Nimbus should use mature Rust
crates at commodity substrate seams instead of maintaining bespoke protocol,
crypto, policy, parser, transport, or filesystem implementations.

This plan is a routing and promotion control plane, not a mandate to add every
crate listed below. Each row must prove that the crate deepens an existing
Nimbus module by putting more behavior behind a smaller Nimbus-owned interface.
If the adoption only spreads a third-party interface through callers, reject it.

Why this is separate from `architecture-review-2026-07-plan.md`: the active
architecture plan owns review-remediation bands. Enterprise crate adoption is a
cross-cutting dependency policy and substrate-inventory lane; individual rows
may route into active or draft plans, but this file owns the inventory and
promotion criteria.

## Non-Goals

- No replacement of Nimbus domain semantics: tenant policy, admission,
  evidence, audit, mutation semantics, storage atomicity, and runtime host
  interfaces stay Nimbus-owned.
- No new product surface by dependency adoption alone.
- No dependency in `nimbus-core` that performs I/O.
- No workspace dependency from `nimbus-runtime`.
- No transport dependency moved into policy crates.
- No blanket FIPS, supply-chain, or security claim without primary-source
  evidence and local dependency-graph proof.
- No QUIC/UDP proxy bypass. `nimbus-masque-h3-egress-plan.md` remains the owner
  for any future QUIC/H3/MASQUE egress support.

## Adoption Gates

Every promoted row must satisfy these before implementation starts:

1. **Current-source refresh.** Re-check the crate's repository, docs, release
   notes, license, MSRV, maintenance posture, and security notes. README
   snippets are not enough when docs.rs/crates.io disagree.
2. **Seam proof.** Name the Nimbus interface and module that owns the seam. If
   the seam will persist, it needs at least two real adapters or a named
   near-term second adapter. One adapter is a hypothetical seam.
3. **Semantics stay local.** Nimbus policy/admission/evidence/error/redaction
   types remain the caller-facing interface. Third-party types do not leak into
   broad public surfaces unless that is the surface's purpose.
4. **Dependency proof.** Record `cargo tree` impact, enabled features, duplicate
   crypto/TLS/native deps, license/attribution impact, `make deny`, and
   `make verify-third-party-attribution`.
5. **Fail-closed tests.** Add parity tests against the current implementation
   plus negative tests for malformed input, wrong identity/policy, missing
   trust material, unavailable dependency/backend, and redacted diagnostics
   where secrets can appear.
6. **Owner routing.** If another active/proposed plan owns the substrate, this
   plan records the dependency choice and that plan owns code execution.

## Candidate Ledger

Status values: `inventory` | `proposed` | `routed` | `adopt` |
`reject` | `blocked`.

| ID | Candidate | Current Nimbus Shape | Disposition | Owner / Route | Status |
| --- | --- | --- | --- | --- | --- |
| EC0 | Full custom-substrate inventory | We have known hand-rolled surfaces, but not a complete census. | Before calling this lane complete, scan for custom protocol parsing, crypto/JOSE/JWKS, DNS, HTTP-over-local-socket, OCI JSON, policy engines, path validation, time/backoff, object storage abstractions, and transport stacks. Classify each as adopt/reject/routed with evidence. | This plan; may update the candidate ledger. | inventory |
| EC1 | `sigstore/sigstore-rust` modular crates: `sigstore-verify`, `sigstore-trust-root`, `sigstore-types` | `nimbus-artifacts` owns `ArtifactVerifierBackend`; `nimbus-server` currently has cosign/SLSA/SBOM verifier effects mostly as command-backed/test-side substrate. | Strong adoption candidate. Add a production `SigstoreRustVerifierBackend` behind `ArtifactVerifierBackend`; keep `ArtifactVerificationPolicy`, admission, evidence, digest-pinned image checks, and redaction Nimbus-owned. Start with OCI image signature verification from a Sigstore bundle. Do not move SLSA provenance or SBOM until attestation support is proven. | `nimbus-artifacts`, `nimbus-server/src/artifact_verifier_effects`, later `nimbus-compute` if CP1 lands first. | proposed |
| EC2 | Hickory DNS resolver/client crates | `nimbus-proxy/src/dns.rs` uses `ToSocketAddrs`; TTL clamps and alias-chain handling are documented seams but not implemented. | Strong adoption candidate when DNS-rebind/cache work starts. Use behind the existing resolver seam for bounded TTL/cache behavior and alias/CNAME chain evidence; do not move egress policy into the DNS layer. | `nimbus-proxy-policy-hardening-plan.md` or a promoted proxy DNS hardening row. | proposed |
| EC3 | OIDC/JWT/JWKS crates (`openidconnect`, focused JWT/JWK libraries) | `nimbus-convex` manually selects JWKS keys and verifies RS256/ES256/EdDSA with `ring`; `nimbus-workload-identity` intentionally hand-assembles local-development JWTs through `IdentitySigner`. | Worth adopting on the verification/discovery side where it reduces custom JOSE/JWKS parsing. Do not replace the SI minting path if it would bypass `IdentitySigner`; that local JOSE assembly is deliberate. | Convex/auth adapter work; SI verification rows if provider auth expands. Read Convex guidelines before Convex API-surface edits. | proposed |
| EC4 | `oci-spec` / `oci-spec-rs` | Container and krun backends manually build OCI runtime JSON with `serde_json::json!`. | Strong adoption candidate for spec-shaped bundle writing and validation. Keep Nimbus sandbox/resource policy as the input interface; use OCI types for serialization and conformance. | `nimbus-sandbox-plan.md`; may also satisfy architecture DE rows around bundle ownership. | proposed |
| EC5 | Unix-socket-capable HTTP client/parser substrate (`hyper`/`hyper-util` custom IO, or a maintained local-socket HTTP crate after refresh) | `nimbus-cli/src/machine/client.rs` hand-rolls HTTP/1.x request/response parsing over Unix sockets. | Adopt or route to a shared parser before Windows named-pipe work. Prefer existing `hyper`/`http` stack if it can avoid new dependency sprawl. | `architecture-review-2026-07-plan.md` DE6; coordinate with `windows-machine-support-plan.md` WIN4. | routed |
| EC6 | Cedar or Rego engine (`cedar-policy`, `regorus`) | `nimbus-tenant` already has an `OperatorExternalPolicyBackend` seam around external policy evaluation. | Candidate for operator/admission policy bundles, not a replacement for native fail-closed egress PDP semantics. Choose Cedar when Nimbus owns policy vocabulary; choose Rego only if OPA compatibility is the actual requirement. | `nimbus-tenant-admission-audit-plan.md`, `layered-admission-control-plan.md`, or tenant policy extension work. | proposed |
| EC7 | OpenTelemetry Rust stack | Nimbus has tracing/metrics-style concerns across proxy, server, runtime, sandbox, and node, but no single observability export plan in this ledger. | Candidate for export, context propagation, and trace/metric conventions. Do not add OTel just for internal logging; require an operating/telemetry consumer and cardinality policy. | Operating/observability plan if promoted; otherwise record as deferred. | inventory |
| EC8 | `tower-http` middleware reuse | Workspace already depends on `tower-http`; some HTTP behavior is still locally wired in handlers/routes. | Prefer existing middleware for CORS, tracing, compression, static file serving, request IDs, and similar commodity HTTP concerns when it reduces handler logic. | `nimbus-server` HTTP cleanup rows such as SR4/CO2-CO5. | inventory |
| EC9 | Object-storage abstraction breadth: `object_store` vs OpenDAL | Workspace already uses `object_store` with a local patch; research previously chose it for bundle/object paths. | No broad switch now. Re-evaluate OpenDAL only if many non-object-store backends become first-class and the current `object_store` seam gets shallow. | `nimbus-blob`, `nimbus-object-storage`, bundle-distribution work. | reject |
| EC10 | QUIC/H3/MASQUE transport crates (`tokio-quiche`/`quiche`, `h3`, `s2n-quic`, `quinn`) | Proxy-required egress currently denies QUIC/UDP bypass; native transport remains JSON-over-WebSocket by default until benchmark evidence. | Routed, not adopted here. Evaluate only inside transport plans with negative bypass tests and local certificate ergonomics. | `nimbus-masque-h3-egress-plan.md`, `native-transport-evolution-plan.md`. | routed |
| EC11 | Crypto/TLS provider posture (`aws-lc-rs`, rustls crypto providers, PQ/FIPS variants) | Workspace already uses `ring` and transitive aws-lc; `nimbus-server` explicitly chooses ring for `tokio-rustls` to avoid native symbol conflicts in test binaries. | Routed and high-risk. Do not opportunistically flip crypto providers for a verifier crate. Any aws-lc/FIPS move must prove native-link compatibility and avoid overclaiming certification status. | `nimbus-fips-iroh-ed25519-retrofit-plan.md`; verifier dependency rows must run crypto dependency proof. | routed |
| EC12 | Path/capability filesystem primitives (`cap-std`, camino/typed path helpers if needed) | `nimbus-fs` already uses `cap-std`; architecture GR5 found many traversal-defense implementations across crates. | Routed through GR5. Prefer consolidating on existing `FsCaps`/`cap-std` choke points before adding another path crate. | `architecture-review-2026-07-plan.md` GR5. | routed |
| EC13 | RustFS storage architecture and Apache-2.0 source ports | Nimbus currently owns `BlobStore`/`LocalPackStore` and `ObjectMetaStore`; RustFS offers stronger bare-metal disk, scanner, erasure, bitrot, and durability patterns but its internal storage contracts are S3/object-server shaped. | Routed. 2026-07-07 review: crate/git dependency ruled out (engine crates `rustfs-ecstore`/`-scanner`/`-heal` are git-only, unversioned, entangled); port narrow recipes/tests with attribution plus per-chunk adversarial security review (upstream shipped CVE-2025-68926, CVSS 9.8); pattern-borrow scanner/heal; erasure via `reed-solomon-simd` if ever activated; RustFS also usable as an external S3 target pinned ≥ `1.0.0-beta.8` (SeaweedFS is the permissive default target). Metadata split stays unless the plan's RFS1 memo gate says otherwise. | `rustfs-storage-hardening-plan.md`. | **done** — dependency ruled out; copy/adapt + pattern only. Outcome (2026-07-09): LocalPackStore hardened + kept (RFS2/3/5/6 merged); metadata split KEPT (RFS1 memo); erasure activated + shipped over `reed-solomon-simd =3.1.0` (RFS7 A/B/C); RustFS consumed as an external S3 target only (RFS8, ≥1.0.0-beta.8, SeaweedFS the recommended default); `disk.rs` is the only Adapted-from surface (attribution gate enrolled). |

## Immediate Recommendation

Promote EC1 first. It has the clearest existing seam (`ArtifactVerifierBackend`),
the highest supply-chain leverage, and a contained blast radius: OCI image
signature verification can be proven against the current cosign CLI behavior
without changing Nimbus admission semantics.

Then run EC0 before promoting more rows. EC0 exists because this initial list is
a focused screen, not a complete proof that every custom substrate has been
classified.

## Suggested Goal Prompt

```text
/goal Execute EC1 from docs/private/plans/enterprise-crate-adoption-plan.md. First refresh the current sigstore-rust crate facts from primary sources and record the exact crate versions/features to use. Then add a production SigstoreRustVerifierBackend behind ArtifactVerifierBackend without leaking sigstore types into Nimbus policy/admission/evidence interfaces. Preserve digest-pinned OCI image requirements, issuer+subject policy, fail-closed errors, and redacted diagnostics. Add parity and negative-path tests for valid bundle, tag-only ref, wrong digest, wrong issuer/subject, malformed bundle, missing trusted root, and unavailable verifier inputs. Record cargo tree impact, make deny, make verify-third-party-attribution, focused tests with counts, and update EC1 status with evidence. Do not move SLSA/SBOM verification unless the crate's attestation support is proven and the plan is amended first.
```
