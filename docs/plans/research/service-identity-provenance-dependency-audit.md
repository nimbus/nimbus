# Service Identity And Provenance Dependency Audit

Audit date: 2026-05-22

This note reviews whether the service-identity, provider-auth, provenance, and
horizontal-scaling plans are adding capabilities Nimbus should instead inherit
from existing libraries.

## Summary

The current direction is cohesive if Nimbus keeps these boundaries:

- **Iroh owns connectivity**, transport peer identity, relays, QUIC streams,
  gossip fanout, and BLAKE3-verified blob transfer.
- **OpenRaft owns consensus mechanics**, not Nimbus policy or identity.
- **SPIFFE/SPIRE owns production workload SVID issuance** when configured.
- **Cosign/SLSA tooling owns artifact verification**, not Nimbus crypto.
- **Nimbus owns policy, admission, tenant/workload subject shape, evidence
  normalization, and audit.**

The main correction is to avoid treating an iroh peer ID as a complete service
identity. Iroh authenticates endpoints to each other. Nimbus still has to bind
that endpoint to an admitted cluster node, tenant workload, placement decision,
grants, provider policies, and audit trail.

## What Iroh Already Covers

| Capability | Use iroh? | Nimbus ownership that remains |
| --- | --- | --- |
| Peer connectivity | Yes. `iroh` gives QUIC streams, direct connections, hole punching, relay fallback, and endpoint public-key identity. | Decide which authenticated peers are allowed to join the cluster and which streams/topics they may use. |
| Node transport identity | Partially. Iroh endpoints have `SecretKey`/`PublicKey`; the public key is the endpoint ID. | Map endpoint IDs to canonical `node_id` records in durable membership state; handle rotation and compromised-node recovery. |
| Topic fanout / membership overlay | Yes. `iroh-gossip` provides HyParView membership and PlumTree broadcast per topic. | Keep gossip payloads non-authoritative and non-secret; avoid high-cardinality topic explosion; use OpenRaft for durable state. |
| Blob transfer integrity | Yes. `iroh-blobs` provides BLAKE3 content addressing and verified streaming. | Verify publisher, provenance, signature, SBOM, and policy with Cosign/SLSA/SBOM tooling; BLAKE3 transfer integrity is not supply-chain trust. |
| Service identity / provider auth | No. | Mint short-lived credentials only from admitted workload identity and provider policy. |
| Runtime/sandbox grants | No. | Enforce grants at admission, HostBridge, runtime, sandbox, and provider adapters. |

Operational caveats from current upstream docs:

- `iroh` latest stable docs are 0.98.2; a 1.0.0 release candidate exists, but
  Nimbus should target stable iroh 1.0.0 for production adoption. Use the
  release candidate only for proof lanes and API-shape validation.
- `iroh-blobs` latest docs are on the 0.101.0 / iroh 1.0 release-candidate
  line, but they explicitly say that version is not yet production quality and
  point production users to 0.35. Do not promote it for production artifact
  distribution without a proof gate.
- `iroh-gossip` topics are separate swarms and broadcast scopes. More topics
  means more peer connections and larger routing tables, so tenant/table topic
  design is an admission and operations decision, not free fanout.

## Recommended Dependency Posture

| Area | Preferred dependency | Posture |
| --- | --- | --- |
| Cluster QUIC transport | `iroh` | Preferred once stable 1.0.0 lands. Hide behind `ClusterTransport`; use iroh endpoint IDs as transport peer IDs only. |
| Gossip fanout | `iroh-gossip` | Preferred on the stable iroh-compatible line for invalidation, liveness, and low-value control messages. Do not use for canonical state or secrets. |
| Blob distribution | `iroh-blobs` | Candidate on the stable iroh-compatible line, with a production proof gate. Treat BLAKE3 hashes as transport/content IDs, not artifact trust anchors. |
| Consensus | `openraft` | Preferred candidate behind an adapter. Current stable line is 0.9.x and API is not stable before 1.0; do not leak OpenRaft types into public APIs. |
| SPIFFE Workload API | `spiffe` | Best current Rust candidate. Use opt-in features (`x509-source`, `jwt-source`, or `workload-api`) and keep it behind a service-identity adapter because the crate is young and not CNCF-owned. |
| JWT/JWS | `jsonwebtoken` | Preferred for straightforward JWT/JWS mint/verify. Do not expand hand-coded `ring` JWT verification into new provider-auth code. |
| OIDC metadata/JWKS | `openidconnect` | Preferred for strongly typed OIDC provider metadata and JWKS handling. It is not a complete provider implementation. |
| Broad JOSE/JWE | `josekit` | Candidate only if Nimbus actually needs broader JOSE/JWE features beyond JWT/JWS. |
| mTLS | `rustls` | Preferred TLS stack. Use SPIFFE-provided SVIDs in production; use `rcgen` only for local-dev fallback certificates. |
| X.509 parsing | `x509-parser` | Candidate for inspection/audit when needed. Do not make Nimbus a general certificate validator. |
| Secret memory hygiene | Existing `zeroize`; consider `secrecy` for new token-bearing structs | Use wrapper types for short-lived credentials and avoid logging/exposing token strings. |
| AWS provider auth | `aws-config` + `aws-sdk-sts` | Add only with the AWS adapter. Use STS web-identity exchange rather than static keys. |
| Kubernetes API | `kube` | Add only if Nimbus needs Kubernetes TokenReview, TokenRequest, or API access. Do not require Kubernetes for non-Kubernetes deployments. |
| Vault | Vault HTTP API via `reqwest`; evaluate `vaultrs` per endpoint | There is no official HashiCorp Rust SDK. Keep Vault support adapter-local and feature-gated. |
| GCP provider auth | Official STS REST API via `reqwest`; monitor `google-cloud-auth` | The Google Rust auth crate is official but currently warns against production use. Do not make it the only production path yet. |
| Azure provider auth | `azure_identity` | Preferred Azure SDK path; use WorkloadIdentityCredential where it matches the deployment shape. |
| OCI reference parsing | `oci-client` / `oci_spec::distribution::Reference` | Preferred; already present in Nimbus crates. Remove hand-rolled digest and registry parsing before production provenance enforcement. |
| Image signatures | Cosign CLI first | Preferred reference verifier. Wrap output and normalize evidence; do not reimplement Fulcio/Rekor/DSSE verification. |
| SLSA provenance | `slsa-verifier` CLI first | Preferred for SLSA evidence. Require immutable digest references. |
| Sigstore Rust library | `sigstore-rs` later | Do not use as the only enterprise verifier while it remains pre-1.0 and lacks full attestation verification coverage. |

## Cohesion Rules

1. **Do not put cloud/provider SDKs in `nimbus-runtime`.**
   Runtime backends receive scoped identity projections only. Provider exchange
   lives in server-side adapters.

2. **Do not make iroh the authorization layer.**
   An authenticated iroh endpoint proves possession of an endpoint key. It does
   not prove tenant membership, workload grants, service authorization, or
   provider-auth eligibility.

3. **Separate provider subjects from audit correlation.**
   The full admitted workload identity is valuable evidence, but provider allow
   policies should normally key on a stable, low-cardinality workload subject.
   Decision IDs, node/machine placement, sandbox IDs, invocation IDs, and token
   instance IDs belong in signed credential claims and audit records unless a
   provider explicitly requires a placement-bound subject.

4. **Keep BLAKE3 and SHA-256 roles separate.**
   Iroh BLAKE3 hashes are excellent for transfer integrity and P2P addressing.
   OCI/Sigstore/SLSA workflows still use digest and signature semantics that
   should remain verifier-owned. Runtime bundles may carry both a BLAKE3
   distribution hash and a SHA-256/Sigstore provenance anchor.

5. **Use SPIFFE/SPIRE as the production issuer path, not a mandatory local-dev
   dependency.**
   Local development may use a Nimbus local issuer for short-lived test
   credentials, but production SPIFFE/SVID support should consume the Workload
   API through an adapter.

6. **Keep provider auth optional and feature-gated.**
   Adding AWS, GCP, Azure, Vault, or Kubernetes support should not drag every
   cloud SDK into the default binary unless the product distribution explicitly
   chooses that profile.

7. **Prefer command adapters for supply-chain verification first.**
   Cosign and SLSA verifier are reference tools. A library path can come later
   after fixture parity and security review.

## Current Codebase Observations

- Nimbus already has `oci-client = "0.16"` in `nimbus-bin` and
  `nimbus-sandbox`. The image-admission path in `nimbus-server` and Compose
  lowering still use hand-rolled `@sha256:` checks; AP0 should replace those
  before production provenance enforcement.
- Convex auth currently parses JWT/JWK and verifies signatures with `ring`
  directly. That path is tested compatibility code, but new service-identity
  minting should not copy it. Use `jsonwebtoken`/`openidconnect` or SPIFFE
  Workload API abstractions for new provider-auth code.
- `zeroize` is already in the workspace. New credential-bearing structs should
  use zeroizing/secret wrappers and must never derive logging output that
  exposes token material.

## Sources

- `https://docs.rs/iroh/latest/iroh/`
- `https://docs.rs/iroh-gossip/latest/iroh_gossip/proto/index.html`
- `https://docs.iroh.computer/protocols/blobs`
- `https://docs.rs/crate/iroh-blobs/latest`
- `https://docs.rs/openraft/latest/openraft/`
- `https://docs.rs/crate/spiffe/latest`
- `https://github.com/spiffe/spiffe/blob/main/standards/SPIFFE_Workload_API.md`
- `https://spiffe.io/docs/latest/spire-about/spire-concepts/`
- `https://docs.rs/jsonwebtoken/`
- `https://docs.rs/openidconnect/latest/openidconnect/`
- `https://docs.rs/rustls/latest/rustls/`
- `https://docs.rs/rcgen/latest/rcgen/`
- `https://docs.rs/oci-client/latest/oci_client/struct.Reference.html`
- `https://docs.sigstore.dev/cosign/verifying/verify/`
- `https://github.com/slsa-framework/slsa-verifier`
- `https://docs.sigstore.dev/language_clients/rust/`
- `https://github.com/sigstore/sigstore-rs`
- `https://docs.rs/aws-sdk-sts/latest/aws_sdk_sts/`
- `https://docs.rs/kube/latest/kube/`
- `https://developer.hashicorp.com/vault/api-docs/auth/jwt`
- `https://developer.hashicorp.com/vault/docs/auth/kubernetes`
- `https://cloud.google.com/iam/docs/workload-identity-federation`
- `https://docs.rs/google-cloud-auth/latest/google_cloud_auth/credentials/index.html`
- `https://docs.rs/azure_identity/latest/index.html`
