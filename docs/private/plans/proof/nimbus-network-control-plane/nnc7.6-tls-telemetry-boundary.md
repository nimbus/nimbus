# NNC7.6 TLS and Telemetry Boundary

Status: `done`

## Outcome

Nimbus keeps the operator ingress identity separate from the workload PEP
interception certificate authority. Network telemetry uses a closed label set.
Stable resource identities and untrusted protocol values do not create metric
keys.

## Frozen scope

| Field | Contract |
| --- | --- |
| Item | `NNC7.6` |
| Baseline | `8dbe562ca4b55147eb92ba01922e9d0fa1043ec0` |
| TLS owners | `nimbus-server` owns the operator ingress certificate and key. `nimbus-proxy` owns each ephemeral workload PEP interception authority. `nimbus-sandbox` publishes only the public PEP trust anchor. |
| Telemetry owners | Each listener or effect crate owns its counters. This item restricts only labels derived from network resources or untrusted network input. |
| Product paths | `nimbus-kv/src/metrics.rs` and `nimbus-proxy/src/fairness.rs`; `nimbus-kv/tests/resp_server.rs` proves the untrusted protocol seam. |
| Proof paths | This proof and one concept-owned static contract under `scripts/nimbus-network-control-plane/`. |
| Forbidden | No certificate-provider abstraction, TLS effect in `nimbus-network`, CA unification, telemetry exporter, global telemetry redesign, provider-handle export, or NNC8 recovery work. |
| Review | Run one GPT-5.6 Sol, xhigh, fast item review after all criteria and affected gates pass. Run one narrow correction review only if an accepted finding changes executable code. |

## Source-derived census

| ID | Source | Current contract | Finding |
| --- | --- | --- | --- |
| T1 | `nimbus-network/src/capability.rs` | `NetworkTlsBehavior` and `HostedCertificate` are capability evidence only. The crate has no TLS dependency, certificate type, key material, or effect. | Correct. Add a regression contract. |
| T2 | `nimbus-server/src/tls.rs` | `TlsConfig` holds operator certificate and key paths. The server validates the pair at startup and terminates the main HTTP listener. | Correct. The server has no direct `nimbus-proxy` dependency. |
| T3 | `nimbus-proxy/src/tls_authority.rs` | `WorkloadPepTlsAuthority` creates an ephemeral CA and per-host leaves. The private CA key stays in memory. Only the public trust anchor is exported. The untrusted hostname cache is capped at 64 entries. | Correct. Existing custody and per-workload separation tests remain acceptance evidence. |
| T4 | `nimbus-sandbox/src/backends/oci/egress.rs` | The PEP registration path creates one authority per workload and writes only `trust_anchor_pem()` below the sandbox-owned trust-anchor root. | Correct. Sandbox has no `nimbus-server` dependency. |
| M1 | `nimbus-network` | No metric store or exporter exists. Closed capability enums return static labels. Stable IDs and provider handles appear only in state, logs, traces, or redacted projections. | Correct. Add a static regression contract. |
| M2 | `nimbus-server/src/latency.rs` | The latency trace uses three closed `LatencySegment` variants and `&'static str` labels. | Correct. This is structured trace data, not an unbounded metric-key map. |
| M3 | `nimbus-proxy/src/fairness.rs` | Production registration uses a pinned `TenantLease` and evicts state at zero pins. A public unpinned `tenant()` accessor can create persistent tenant entries, and a public `TenantFairness::tenant()` returns the raw tenant label. Production does not call either method. | Hardening gap. Restrict the test accessor and remove the unused raw-label accessor. |
| M4 | `nimbus-kv/src/metrics.rs` | `record_command` inserts `name.to_ascii_uppercase()` into `BTreeMap<String, _>`. `handle_connection` passes the client-supplied command name, including unknown commands. | Defect. An unauthenticated client can create an unbounded metric-key set. |
| M5 | Runtime, service-usage, and storage metrics | These metrics do not describe network resources or listener protocol input. Active runtime-isolation and operating telemetry owners retain them. | Out of scope. Do not fork their authority. |

## Acceptance

| ID | Falsifiable success criterion |
| --- | --- |
| A1 | A static contract proves that `nimbus-network` has no certificate owner, TLS key type, TLS dependency, TLS effect, or metric exporter. |
| A2 | The contract proves that `nimbus-server::TlsConfig` and `nimbus-proxy::WorkloadPepTlsAuthority` remain in distinct crates with no direct dependency path that permits substitution. |
| A3 | Existing proxy tests prove that two workload PEPs mint different CAs, public export contains a certificate and no private key, and the hostname leaf cache never exceeds 64 entries. |
| A4 | Existing server tests prove that the operator certificate and key pair loads at startup, HTTPS works, plain HTTP is refused, and an invalid path fails boot. |
| A5 | `NimbusKvMetrics` uses a closed command-label type. Every supported command has one stable label, and every unsupported command uses one `UNKNOWN` label. |
| A6 | A regression submits at least 128 distinct unknown command names. The metric map grows by one unknown bucket, preserves the total call count, and contains no submitted name. |
| A7 | Existing known-command output remains byte-compatible for `SET`, `GET`, and the other supported labels. `NIMBUS.METRICS` does not recursively create unbounded labels. |
| A8 | Production proxy code can create tenant counter state only through `checkout()`. Dropping the last `TenantLease` evicts the entry. No public method exposes a raw tenant value as a metric label. |
| A9 | The static contract fails named mutations for a network certificate owner, a crossed server/proxy authority reference, a dynamic KV metric-key type, an unpinned production tenant lookup, and a resource-ID metric label. |
| A10 | Affected behavior, strict quality, static architecture, docs, proof lint, and the one item review pass. `nimbus-network -> nimbus-core` remains its only workspace edge. |

## Fail-before plan

1. Add a KV metrics regression that records 128 distinct unknown commands and
   requires one `UNKNOWN` bucket. The baseline must fail because it retains all
   128 names.
2. Run the static contract before the product fix. It must report the dynamic
   `String` command-key map and the two public proxy tenant accessors.
3. Record exact failure output here before changing product behavior.

## Verification

```text
cargo test -p nimbus-kv metrics -- --test-threads=1
cargo test -p nimbus-proxy tls_authority -- --test-threads=1
cargo test -p nimbus-proxy fairness -- --test-threads=1
cargo test -p nimbus-server tls -- --test-threads=1
node scripts/nimbus-network-control-plane/tls-telemetry-contract.mjs --self-test
node scripts/nimbus-network-control-plane/tls-telemetry-contract.mjs
cargo clippy -p nimbus-kv -p nimbus-proxy -p nimbus-server --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-kv -p nimbus-proxy -p nimbus-server --all-features --no-deps
cargo fmt --all --check
git diff --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

## Evidence ledger

| Gate | Result |
| --- | --- |
| Fail-before | `cargo test -p nimbus-kv metrics::tests::unknown_client_commands_share_one_bounded_metric_label -- --exact --test-threads=1` failed as required: `0 passed; 1 failed`; the assertion reported `left: 128`, `right: 1`. The live static contract then failed on the open command-label map and production tenant accessors while its five named mutation tests passed. |
| Focused behavior | KV metric state `3/3`; untrusted RESP protocol regression `1/1`; proxy fairness `7/7`; proxy TLS authority `5/5`; server TLS and capability separation `6/6`. |
| Static contract | Final self-test `15/15`; live contract passes. Cargo metadata supplies canonical dependency names. The contract recognizes hyphenated and underscored metric-package families, fails closed on missing dispatcher boundaries and every string-literal command arm, attributes method and UFCS fairness-map insertions to their Rust function, and scans qualified or imported proxy metric macros for resource identities. |
| Full affected suites | KV `30 passed / 3 declared ignores`; proxy `164 passed / 0 ignored`. Server product code did not change; its six TLS-boundary tests pass. |
| Strict quality | Affected all-target/all-feature Clippy passes with warnings denied. Warning-denied Rustdoc passes with all features. Format, Prettier, Node syntax, and diff checks pass. The first combined all-feature Clippy command stopped on the repository V8 pointer-compression cache guard before Nimbus linting. A package-scoped `cargo clean -p v8 --target-dir target/ptrcomp` removed `38` build files; the separated KV/proxy and pointer-compressed server gates then passed. |
| Architecture and docs | Static network verifier `36/36`; docs `108` pages; site `17/17`; strict proof lint one file with zero diagnostics. `nimbus-network` still has only the `nimbus-core` workspace edge. |
| Structured review | The full Sol/xhigh/fast review, thread `019ffcd0-95a8-71e2-bfbe-9694c6913ca5`, reported five P2 static-proof holes and one P3 stale route at confidence `0.98`. The sole narrow review, thread `019ffcdd-8ed7-77b1-825f-a0fdc67bc3c9`, found four remaining P2 proof mutations at confidence `0.98`. All ten findings are accepted and corrected. Review cadence is exhausted. |

## Acceptance disposition

| Criterion | Result | Evidence |
| --- | --- | --- |
| A1 | `pass` | The live static contract rejects certificate, key, TLS-effect, transport, and metric-exporter ownership in `nimbus-network`. |
| A2 | `pass` | The contract requires server-owned `TlsConfig`, proxy-owned `WorkloadPepTlsAuthority`, and no crossed direct dependency or source reference. |
| A3 | `pass` | Proxy TLS tests `5/5` prove distinct authorities, public-only export, and the 64-entry leaf-cache bound. |
| A4 | `pass` | Server TLS tests `6/6` prove fixture loading, HTTPS, plain-HTTP refusal, invalid-path boot failure, and transparent workload-ingress capability truth. |
| A5 | `pass` | `CommandMetricLabel` is a closed 19-variant map key. State tests cover every supported case-insensitive label and `UNKNOWN`. |
| A6 | `pass` | Both unit and RESP-listener tests submit 128 distinct unknown names and observe one `UNKNOWN` bucket with 128 calls and no submitted name. |
| A7 | `pass` | All 18 supported output labels remain exact and sorted through the public `BTreeMap<String, _>` snapshot. Existing RESP tests retain `SET`, `GET`, and `NIMBUS.METRICS` output. The metrics command classifies to one closed label. |
| A8 | `pass` | The production scan removes test items before it proves that only `checkout()` creates tenant state. Fairness tests `7/7` prove shared pins and last-lease eviction. The raw tenant accessor is deleted. |
| A9 | `pass` | Static self-test `15/15` rejects the five named mutations plus canonical crossed and underscored dependencies, metric sources and qualified or imported macros, missing dispatcher boundaries, punctuation/digit command growth, method or UFCS tenant insertion, and a raw tenant accessor. |
| A10 | `pass` | Product behavior, strict quality, static architecture, docs, proof lint, one full review, and the sole narrow correction review pass their frozen closeout contract. |

## Full review dispositions

| Finding | Disposition and correction |
| --- | --- |
| P2: the tenant check recognized only a method named `tenant`. | Accepted. The production scan attributes every `.entry()` or `.insert()` to its Rust function and requires exactly one insertion owned by `checkout()`. A valid renamed `lookup()` mutation fails. |
| P2: proxy metric sinks were absent from the resource-identity scan. | Accepted. The scan includes all proxy production source and recognizes label, metric, counter, and metric-macro sinks. A valid proxy label-map mutation fails. |
| P2: dependency matching accepted only one inline Cargo form. | Accepted. Live checks use parsed `cargo metadata` package names, including aliases, workspace inheritance, target sections, and table dependencies. A crossed canonical dependency mutation fails. |
| P2: missing KV dispatcher boundaries could pass vacuously. | Accepted. Invalid boundaries fail directly. The derived command set must equal all 18 supported labels. Missing-boundary and extra-command mutations fail. |
| P2: metric dependency and macro detection was incomplete. | Accepted. Canonical dependency names reject the metrics, Prometheus, and OpenTelemetry families. Production source rejects `metrics::` and standard metric macros. Both dependency and macro mutations fail. |
| P3: the recovery route still pointed to fail-before work. | Accepted. The recovery header and NNC7.6 row now route to correction gates and the narrow review. |

## Narrow review dispositions

| Finding | Disposition and correction |
| --- | --- |
| P2: underscored OpenTelemetry package names were not metric dependencies. | Accepted. Dependency and source-family patterns now recognize hyphens and underscores. Separate `opentelemetry_sdk` dependency and source mutations fail. |
| P2: the dispatcher census omitted commands with digits or punctuation other than dots. | Accepted. The census now collects every string-literal match arm. A `NIMBUS.V2` mutation fails. |
| P2: fairness insertion ownership omitted UFCS calls. | Accepted. The census joins method and UFCS `entry` or `insert` calls before function attribution. A `HashMap::entry` lookup mutation fails. |
| P2: proxy resource-label detection omitted imported metric macros. | Accepted. Qualified and unqualified standard metric macros share one sink scan. An imported `counter!` tenant-label mutation fails. |

## Review identity

- Full-review staged tree: `6dea180182f9fb38f18833053550ac06aa9eeed2`.
- Full-review patch SHA-256:
  `4667cc468e656694e11c9b4bbe588a930c7dad3c5741c622ef713bfedfa8569f`.
- Reviewer: GPT-5.6 Sol with xhigh reasoning and fast service. One bundle ran.
  TruffleHog was clean. We accepted six findings and rejected none.
- Narrow-review staged tree: `45f49930f12bac46504965af58da080296f5c613`.
- Narrow-review patch SHA-256:
  `6a2ff5400c723e0d804116773df79ec8601ef4ff106b429d843b8937e698cc76`.
- The narrow review used GPT-5.6 Sol with xhigh reasoning and fast service.
  It accepted four proof findings. The corrections changed no product Rust,
  so the affected static and documentation gates are the final closeout proof.
