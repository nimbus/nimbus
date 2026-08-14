# NNC9.1 Static Verifier Closure

Status: `complete`

Starting checkpoint:
`23d75f4a4ecd255c11264990602ee9b6e520b585`.

## Scope

NNC9.1 closes the network control-plane dependency, authority, and recovery-
ledger verifier. It adds compiler-resolved and generated-code evidence where
the existing source census cannot provide Rust name resolution. It does not
move an effect, add a provider, change a product crate, or claim that a source
parser resolves aliases or macro expansion.

The verifier remains one fail-closed command:
`scripts/verify-nimbus-network-control-plane.sh`. We extracted the aggregate
mutation suite to its concept-owned sourced module because the former verifier
file exceeded the repository's 2,000-line decomposition threshold. A separate
Node contract and a versioned baseline under this proof directory own compiler
evidence.

## Acceptance Contract

| ID | Criterion |
| --- | --- |
| K1 | The live verifier prints one named `PASS` or `FAIL` for every condition and an exact summary. No input, tool, branch, provider, or generated-output absence can become a pass. |
| K2 | A missing required baseline fails exclusively. The complete aggregate mutation suite retains all earlier fail-closed cases and exercises the new compiler baseline. |
| K3 | Every `done` item has evidence. The sole `in_progress` item records its dependency, owned paths, last green result, next action, and blocker state. Invalid checkpoint identity and missing recovery state fail closed. |
| K4 | The exact source inventory remains authoritative for classified sites. Compiler MIR records resolved direct and expanded standard-library bind/adoption calls. The parsed scanner records qself, glob, operation-shaped macro, include, and conditional-module boundaries. Generated Rust outputs are scanned separately. |
| K5 | The compiler evidence records a current input digest, authority-inventory digest, compiler identity, exact owner roster, per-owner MIR digest and call counts, source boundaries, and generated-output digests. `nimbus-network` has zero resolved socket authority calls. |
| K6 | The resolved call aggregate equals the exact source inventory. Any stale input, missing owner, count mismatch, package-sum mismatch, missing generated output, generated authority, or unapproved generated boundary fails closed. |
| K7 | A deep collection on the candidate tree reproduces the recorded compiler and generated-output evidence exactly. A cheap live check validates its authenticated current baseline without compiling every owner. |
| K8 | The main verifier is a thin composition root below 1,500 lines. The complete aggregate mutation concept is one sourced child below 1,500 lines. Neither module splits a condition's invariant. |
| K9 | Bash syntax, ShellCheck, Node syntax, Prettier, strict proof lint, format/diff checks, private-doc gates, and the live verifier pass with real exit status. |
| K10 | Exactly one GPT-5.6 Sol/xhigh/fast item review runs after K1-K9 are green. A narrow review runs only if an accepted finding materially changes executable code. The proof, ledger transition, verifier implementation, and baseline form one exact item commit. No push or PR occurs. |

## Compiler And Generated-Code Boundary

The existing bind census uses a Rust parser plus source-derived ownership
records. It remains the exact authority-site inventory. It cannot, by itself,
prove name resolution or expansion. NNC9.1 adds a second, narrower proof:

1. Cargo emits MIR for every production library and binary target in all six
   owner packages. Collection uses all features, the recorded host target, the
   recorded compiler, and no test configuration.
2. The compiler contract counts resolved direct TCP, UDP, and Unix bind and
   standard-library adoption constructors in that MIR.
3. Each target sum must equal its package inventory. The package aggregate must
   equal the source inventory by constructor kind. `nimbus-network` has an
   independent zero-call requirement.
4. The parsed Rust scanner records qself adoption, network glob imports,
   authority-shaped macros, `include!` expansions, and conditional module
   paths. Unclassified qself, glob, or operation-shaped macro boundaries fail
   closed. A direct call in source that is absent from the selected compiler
   configuration also breaks source-to-MIR reconciliation.
5. Cargo generates `nimbus-firebase` outputs in a fresh isolated target
   directory. The contract requires the exact five-file relative roster,
   hashes each positive-size output, and scans it for forbidden network-
   authority vocabulary.

This is complementary evidence. The source inventory owns site identity and
classification. Rustc owns resolution and expansion evidence. This proof does
not present either source as a substitute for the other.

## Candidate Evidence

The versioned baseline is
`nnc9.1-compiler-authority-baseline.json`. Its current candidate records:

- Rust `1.96.1` on `aarch64-apple-darwin`.
- `1,657` compiler/source inputs plus the effective Cargo configuration,
  compiler launchers and toolchain binaries, target cfg, and relevant compiler
  environment.
- The six effect-owner packages expose seven production targets. These targets
  are the CLI, KV, network, proxy, sandbox, and server libraries plus the
  sandbox guest-user-switch binary.
- `16` resolved calls: seven TCP binds, three TCP `from_std` adoptions, one
  TCP raw-file-descriptor adoption, three Unix binds, and two Unix `from_std`
  adoptions.
- Zero UDP, owned-file-descriptor, raw-socket, or owned-socket calls.
- Zero resolved socket-authority calls in `nimbus-network`.
- One source `include!` boundary and 22 conditional module paths.
- The scanner records 44 cfg-bearing external modules, 17 exactly classified
  authority-shaped macros, and 37 existing classified risks.
- Zero qself bind or network glob import boundaries.
- Five generated Firebase Rust outputs with zero parsed authority, risk,
  composition, or forbidden-boundary findings. The baseline authenticates the
  exact four `firebase_grpc.rs` `OUT_DIR` include edges to the other generated
  files.

The baseline records `102,796,303` bytes of compiler MIR by target digest and
byte count. It does not commit those transient compiler artifacts.

## Verification Ledger

| Gate | Result |
| --- | --- |
| Main/child modularity | Pass: main verifier `1,273` lines; aggregate self-test module `1,242` lines; compiler contract `1,334` lines. |
| Parsed Rust scanner | Pass: `14/14`, including multiline qself, grouped/glob imports, nested conditional module paths, cfg-bearing external modules, authority-shaped macro names, and raw/instance generated binds. |
| Compiler contract self-test | Pass: `18/18`. Stale input, inventory, compiler/config/environment, and target matrix; missing owner; invalid/unknown call schema; aggregate, target, and package mismatches; a network-owned call; qself/glob/macro boundaries; missing/invalid generated output or scan result; and generated authority all fail closed. |
| Compiler baseline cheap check | Pass: six packages, 16 resolved calls, and five generated outputs. |
| Compiler deep reproduction | Pass: fresh MIR for all seven production targets plus a fresh isolated generated-output collection reproduce the authenticated baseline exactly. Existing vendored Brotli warnings do not change the result. |
| Aggregate mutation suite | Pass: `607/607` in the sole post-narrow-correction aggregate run. |
| Live verifier | Pass: `39/39`, including NNCV038. |
| Quality and docs | Pass: Bash syntax, sourced ShellCheck, Node syntax, Prettier, both Rustfmt scopes, diff checks, and strict proof lint pass. Docs pass `108` pages, and the site passes `17/17` conditions. |
| Candidate-frozen item review | The full review accepted R1-R5. The sole narrow review accepted NR1-NR6. Review cadence is exhausted. |
| Exact item commit | The commit containing this proof and the NNC9.1 `done` ledger row is the exact item checkpoint. |

## Item Review Disposition

The one full GPT-5.6 Sol/xhigh/fast review rated the candidate incorrect at
`0.97`. It reported five findings. We accept all five because they identify a
material K4-K7 false-pass or reproducibility risk:

| ID | Priority | Disposition and required correction |
| --- | --- | --- |
| R1 | P1 | Corrected. All seven production library/binary targets compile with the recorded all-feature, host-target, no-test posture. Source-to-MIR reconciliation and parsed risky-boundary rejection cover unselected source. |
| R2 | P2 | Corrected. Exact call keys, safe nonnegative values, target/package/aggregate sums, per-package inventory equality, and the independent network-zero invariant pass. |
| R3 | P2 | Corrected. The baseline authenticates Cargo and rustc launchers and toolchain binaries, target and target cfg, relevant environment, Cargo configuration, locked metadata, and the exact production-target matrix. Collection forces the recorded rustc and target. |
| R4 | P2 | Corrected. The Rust scanner supplies parsed qself, glob, macro, include, and conditional-path evidence; `13/13` tests and current exact boundary classification pass. |
| R5 | P2 | Corrected. Firebase generation uses a fresh target directory, and the exact five positive-size outputs reproduce their recorded digests with zero findings. |

These material executable corrections authorize exactly one narrow correction
review after all affected proofs pass. They do not authorize another full
review or a new audit.

The sole narrow GPT-5.6 Sol/xhigh/fast review reported six P2 findings and
rated the correction incorrect at `0.97`. We accept all six:

| ID | Disposition and required correction |
| --- | --- |
| NR1 | Corrected. The compiler identity authenticates effective Cargo build/compiler environment variables. |
| NR2 | Corrected. Cargo configuration discovery walks to the filesystem root, records search order, and deduplicates Cargo home by canonical path. |
| NR3 | Corrected. The scanner records every ordinary cfg-bearing external module, its predicate, and its resolved source candidates. |
| NR4 | Corrected. Operation-shaped macro names fail closed even when their token body is empty. |
| NR5 | Corrected. Generated Rust uses the parsed authority scanner, including raw and instance operations; the exact four generated include edges are authenticated separately. |
| NR6 | Corrected. The contract requires explicit scan counts and a findings array; the missing-field mutation passes. |

The corrected scanner, helper, baseline, deep reproduction, aggregate, and
live verifier all pass with the final counts above. The review cadence permits
no further structured review.

## Ownership Conclusion

NNC9.1 adds no runtime authority. `nimbus-network` remains transport-free and
provider-effect-free, with `nimbus-core` as its only workspace dependency.
Compute remains the workload saga coordinator. Services retains logical names
and readiness. Sandbox, server, KV, machine, proxy, and node retain concrete
effects. System retains observed projections. Cluster transport remains
outside `nimbus-network`.
