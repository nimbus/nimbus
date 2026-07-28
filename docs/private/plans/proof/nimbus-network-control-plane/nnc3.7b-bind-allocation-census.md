# NNC3.7b Bind And Allocation Census Closure Proof

Date: 2026-07-27

Status: `complete; five review runs fully dispositioned; final gates green`

Starting checkpoint:
`df1ecc106e845c82a2b02102948712401d4fd3fb`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Written Acceptance

NNC3.7b requires:

> Every baseline production site is migrated or explicitly adopted; test-only
> and command-local ephemeral exemptions are narrow, named, and mechanically
> classified.

The implementation satisfies the criterion through three independently
verifiable clauses:

| Clause | Evidence | Candidate result |
| --- | --- | --- |
| Every logical baseline site has one truthful disposition. | Schema-v2 inventory contains 26 unique sites: 24 active canonical/migrated/adopted/provider/local-IPC seams and 2 explicitly retired probe/drop authorities. The two previously implicit sites are the canonical `nimbus-network` reserve operation and the machine-API Unix descriptor consumer. Every active row has a live source-occurrence or exact declaration proof; retired rows cannot retain an occurrence. | Pass |
| Every current-baseline direct production authority occurrence is exact and complete. | The generated census finds 67 authority/ownership occurrences keyed by path, authority kind, enclosing symbol, and ordinal, plus 36 exact ambiguous or macro-shaped occurrences proven not to be independent network authorities. This includes 35 by-value listener slots and 8 listener-return handoffs across composition and adapter children. Every current authority has one active site classification; missing, duplicate, stale, wrong-line, wrong-path-set, wrong-kind, wrong-symbol, same-path site swaps, and retired-site classifications fail closed. Independent direct scans cover the scanner's bounded-source-shape limits. | Pass |
| Exemptions are narrow and mechanically proven. | A `syn` AST visitor skips exact `#[cfg(test)]` nodes without exempting their production file. Test/bench paths and `nimbus-testing` are exact conventions, and direct production module/include edges into a convention-exempt or explicitly exempted Rust source are rejected. The three path-owned container test children (`lifecycle.rs`, `planning.rs`, and `support.rs`) are accepted only while `runtime/tests.rs` declares each exact escaped module name and `runtime.rs` keeps the parent `tests` module under `#[cfg(test)]`. The sole current computed production include is a named Firebase/Tonic code-generation boundary; all current generated outputs are separately scanned and contain no socket/bind/port authority. | Pass |

Written NNC3.7b acceptance: **3/3 candidate pass**.

There is no command-local ephemeral production exemption in the current
source. CLI dev provider-assigned ports are real classified binds that remain
held through server adoption. Every ephemeral bind omitted from the production
census is mechanically test-only.

## Result

The old NNCV006 check classified a whole Rust file as soon as any inventory
row named that path. A second bind, inherited descriptor, or allocation
authority added elsewhere in the same file therefore escaped the gate.
Moreover, its `examples` list exempted entire production files merely because
they contained an inline test module.

NNC3.7b replaces that file-level check with a two-part, concept-owned census:

- `scripts/nimbus-network-bind-census-ast/` is a standalone, locked Rust tool
  that parses production source with `syn` and returns structural authorities,
  risks, and function declarations as JSON.
- `scripts/verify-nimbus-network-bind-census.mjs` proves the narrow path-owned
  test exemptions, invokes the structural scanner, and compares the observed
  identities with the classified inventory.

The shell verifier remains the composition root and reports the combined
result as NNCV006. The scanner has its own `[workspace]`, so it introduces no
product/workspace dependency edge and does not alter `nimbus-network`.
NNCV006 is an exact current-source census and a bounded regression guard. It is
not a Rust compiler: `syn` does not provide name resolution, macro expansion,
conditional compilation evaluation, generated-code expansion, or type
inference. NNC3.9 and NNC9.1 therefore require compiler-resolved/generated-code
evidence before the final one-authority claim.

An occurrence identity is:

```text
repository path | authority kind | enclosing Rust symbol | ordinal
```

The diagnostic line is also recorded and must match the source snapshot, but
an IP address or numeric port is never identity. The structural scanner:

1. walks production Rust under `crates/`;
2. applies only the inventory-declared path/test-support conventions and exact
   mechanically proven path-owned test children, after enumerating every Rust
   file and rejecting direct production `mod`, `#[path]`, or literal
   `include!` references into either exemption class;
3. skips exact inline `#[cfg(test)]` AST items/expressions while continuing to
   visit the remainder of each production-named file;
4. parses functions, fields, tuple fields, nested owned types, return types,
   calls, method calls, request literals, string literals, and macros as Rust
   syntax rather than reconstructing Rust grammar with regular expressions;
5. enumerates TCP, UDP, generic socket, inherited/adopted descriptor, Unix
   local-IPC, by-value listener fields/parameters, listener-return handoffs,
   legacy probe/allocator/`PortManager`, and provider bind-request occurrences;
6. attributes each occurrence to its enclosing function and stable ordinal;
7. discovers known bind/adoption function paths even when used as values, and
   separately enumerates every instance `.bind`, bare/imported or ambiguous
   associated bind/adoption, and authority-shaped macro token stream,
   requiring an exact line-fenced non-authority reason for every current false
   positive;
8. rejects socket-authority import aliases plus top-level and associated Rust
   type aliases other than the explicitly scanned
   `UnixListener as StdUnixListener` spelling;
9. rejects an unclassified observed occurrence and a classified occurrence
   no longer present; and
10. emits structural function declarations so every declaration-only site is
    checked by exact path/name/line without a second lexical Rust parser, while
    active occurrence sites retain allowed kind/symbol contracts and retired
    sites must remain occurrence-free.

The scanner also catches inventory drift: duplicate keys, missing site IDs,
non-active site references, site/occurrence path divergence, stale line
numbers, and stale derived counts are named failures.

No Rust runtime, socket, provider, policy, auth, service, proxy, system,
workload, or cluster behavior changed. `nimbus-network` remains transport-free
and retains exactly one workspace dependency on `nimbus-core`.

## Fail-Before Evidence

The fail-before fixture declared one classified bind and a second bind in the
same synthetic production file:

```text
fn first_authority()  { TcpListener::bind("127.0.0.1:0"); }
fn second_authority() { TcpListener::bind("127.0.0.1:0"); }
```

Command:

```text
bash scripts/verify-nimbus-network-control-plane.sh --self-test
```

Before the classifier replacement, the command exited `1`. All 16 pre-existing
verifier self-tests passed, and the only new result was:

```text
SELFTEST FAIL second bind in a classified file escaped NNCV006
self-test: 1 failed
```

This was the intended defect, not a missing input or unrelated verifier
failure.

During the green conversion, the first real inventory run exposed a second
root cause: an inline `#[cfg(test)]` field in
`PreparedMachineSshPortLease` caused the inherited lexical stripper to blank
the following production `impl`. The initial lexical correction distinguished
comma-terminated fields/variants from braced or semicolon-terminated items.
A dedicated regression now proves such a field cannot hide a later production
bind, while the existing test-only-module case remains excluded.

The first structured review then identified six real fail-open classes in the
candidate verifier. Each is now represented by executable expected-red proof:

- a `#[cfg(test)]` function parameter without a trailing comma cannot consume
  its containing production function;
- an aliased socket type and an instance-method `.bind(...)` cannot escape the
  authority/risk census;
- generalized port-allocation function names and current provider bind
  requests (`gvproxy`, systemd, Netavark, and the machine forwarder) are
  executable scan inputs rather than prose-only `rg` commands;
- path-owned module names are escaped and linked through the exact declared
  child path and exact cfg-owned parent module;
- swapping two same-path site IDs fails on the site's allowed
  authority-kind/symbol contract; and
- symbol-presence rows resolve one exact function declaration and source line,
  not an arbitrary identifier elsewhere in the file.

The required corrected-candidate review found six additional valid Rust/source
forms that the first correction still missed. Before any second-cycle edit,
the scope governor was applied: all six reproduce as an NNC3.7b fail-open,
change only the census/exemption owner, require no new public/storage/provider
contract, and are therefore in-scope blockers rather than follow-ups or
NNC3.8/NNC3.9 work. Running the previously staged helper against each fixture
produced exit `0`:

```text
cfg_macro old_candidate_rc=0
prebound_consumer old_candidate_rc=0
reserve_port old_candidate_rc=0
provider_request old_candidate_rc=0
postfix_bind old_candidate_rc=0
type_alias old_candidate_rc=0
```

The second-cycle lexical helper then:

- terminates cfg-owned `macro_rules!`, declarative macro, and attributed block
  targets at their exact brace boundary;
- discovers by-value listener fields/parameters and direct listener-return
  handoffs from production source;
- enforces `reserve` as an allocation-action token, classifying both the
  canonical network reserve operation and the sandbox composition call;
- detects fully qualified known provider-request literals and any `*Request`
  literal that carries `host_port`, `publish*_port`, or `port_mappings`;
- classifies every postfix `.bind(...)`, including index and `?` receivers; and
- rejects direct Rust `type Alias = TcpListener`-style authority aliases.

The intended final review of that second-cycle candidate exposed five more
valid Rust forms:

- a cfg-owned brace macro invocation followed by production code;
- `UnixDatagram` bind/adoption/ownership;
- shorthand `ProviderRequest { host_port }` fields;
- tuple-struct listener ownership; and
- multiline listener-return types.

All five were direct NNC3.7b fail-open cases. More importantly, they were the
third independent set of valid syntax forms that required changes to a growing
hand-written Rust grammar. The owner therefore stopped the regex patch loop
and replaced the authority/declaration parser with the standalone `syn` AST
tool. The JavaScript wrapper retains only its narrow module-linkage proof and
inventory comparison responsibilities.

The structural replacement has four direct Rust tests covering the five
third-review forms plus mixed referenced/owned listener types. The shell
verifier has one expected-red case for each form. Structural function
declarations replace the former lexical `functionRanges` parser, and
authority-shaped macro token streams now require explicit classification
because the tool deliberately does not perform macro expansion.

Focused helper evidence after the fix:

```text
unclassified production bind/allocation authority:
__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs
|tcp-bind|second_authority|1:line=2

unclassified production bind/allocation authority:
__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs
|tcp-bind|production_authority|1:line=6

stale authority occurrence classification:
__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs
|tcp-bind|deleted_authority|1
```

A fixture containing only a `#[cfg(test)]` listener module exits `0`. The
complete fail-closed matrix passes 44/44.

## Reconciled Logical Inventory

The schema-v2 inventory source checkpoint is
`df1ecc106e845c82a2b02102948712401d4fd3fb`. No production Rust source differs
from that checkpoint.

| Inventory ID | State | Reconciled authority |
| --- | --- | --- |
| `network-port-lease-authority` | active/canonical | `nimbus-network` owns the one crash-safe host-global reserve operation while remaining free of socket/provider effects. |
| `sandbox-manifest-port-allocation` | active/migrated | Sandbox builds the launch batch; `LocalPortLeaseAuthority` atomically reserves it. No manifest scan or first-free choice remains. |
| `sandbox-pep-port-allocation` | active/migrated | The PEP is one host-internal request in the same atomic batch. |
| `sandbox-pep-assignment` | active/migrated | Sandbox projects an already-reserved request and composes the proxy effect. |
| `cli-dev-prebound-bind` | active/provider bind | CLI claims before the real bind and retains the Active socket through server adoption. |
| `cli-dev-prebound-consumer` | active/adopted | Startup transfers the same-incarnation socket bundle without a numeric handoff. |
| `cli-start-adapter-probe` | retired | Adapter resolution is pure desired state; the shared lease plus real bind decides availability. |
| `machine-ssh-port-allocation` | active/migrated | Machine preparation range-reserves and claims in the host-global authority before gvproxy. |
| `machine-ssh-port-probe` | retired | Production probe/drop allocation is deleted. |
| `cli-main-direct-listener` | active/migrated | CLI claims before every resolved main-listener bind and adopts the exact result. |
| `cli-main-systemd-listener` | active/adopted | Inherited TCP fd 3 is validated and recorded as externally owned. |
| `node-systemd-socket-unit` | active/adopted | Systemd retains the declared `ListenStream` bind effect. |
| `server-sibling-wire-listeners` | active/migrated | Server claims before each sibling bind, activates before observation/serve, and retains protocol ownership. |
| `server-main-listener-consumer` | active/adopted | The server adapter converts the continuously held descriptor without releasing its Active fence. |
| `kv-direct-listener` | active/migrated | KV claims before its RESP bind and records exact success/failure evidence. |
| `kv-prebound-listener` | active/adopted | KV records external provenance and never releases the outside owner. |
| `pep-listener-bind` | active/migrated | Proxy binds an inert listener under sandbox-owned claim; accept starts only after exact activation. |
| `machine-port-proxy-bind` | active/migrated | Sandbox binds inert wildcard guest listeners, adopts the batch, then starts accept loops. |
| `netavark-port-mapping-effect` | active/provider effect | Sandbox lowers lease-authenticated desired mappings; Netavark retains kernel effects. |
| `machine-forwarder-port-effect` | active/provider effect | Sandbox sends exact expose/unexpose requests; the machine forwarder retains the effect. |
| `machine-gvproxy-ssh-bind` | active/provider effect | Gvproxy receives only the durably selected/claimed port; readiness supplies adoption evidence. |
| `machine-api-unix-consumer` | active/local IPC | The machine API resolves and transfers the exact direct/systemd Unix descriptor into its serve loop. |
| `machine-api-unix-direct` | active/local IPC | Machine API filesystem socket remains outside TCP/UDP lease authority. |
| `machine-api-unix-systemd` | active/local IPC | Inherited Unix descriptor remains systemd/machine-owned local IPC. |
| `machine-ready-unix-listener` | active/local IPC | Provider readiness rendezvous remains filesystem IPC. |
| `machine-ignition-unix-listener` | active/local IPC | One-shot ignition transport remains filesystem IPC. |

Totals are derived and checked:

| Measure | Count |
| --- | ---: |
| Logical production sites | 26 |
| Active sites | 24 |
| Retired probe/drop sites | 2 |
| Exact source authority/ownership occurrences | 67 |
| Exact classified non-authority risk occurrences | 36 |
| By-value listener ownership slots | 35 |
| Listener-return handoffs | 8 |
| Direct TCP bind occurrences | 6 |
| TCP `from_std` occurrences | 3 |
| TCP inherited raw-fd occurrences | 1 |
| UDP bind occurrences | 0 |
| Unix local-IPC occurrences | 7 |
| Provider bind-request occurrences | 4 |
| Allocation-definition occurrences | 2 |
| Legacy `PortManager` definition occurrences | 1 |
| Unclassified occurrences | 0 |

The remaining `PortManager` occurrence is classified truthfully as the
sandbox composition adapter over the network authority. NNCV005 continues to
reject its old type name until the explicitly sequenced NNC3.9 deletion gate;
NNC3.7b does not silently perform that later item.

## Exemption Proof

| Mechanism | Admitted scope | Mechanical condition |
| --- | --- | --- |
| Path convention | `crates/**/tests/**`, `crates/**/tests.rs` | Scanner enumerates these exact paths, then skips them only while no scanned direct production `mod`, `#[path]`, or literal `include!` reaches them. |
| Inline cfg | One exact `#[cfg(test)]` item/module/expression | AST visitor skips only the attributed node and scans the remaining file. |
| Path-owned test modules | Exact `lifecycle.rs`, `planning.rs`, and `support.rs` children under `container/runtime/` | `runtime/tests.rs` must declare each escaped path/module pair, that file must be the exact `runtime/tests.rs` child, and `runtime.rs` must declare the parent `tests` module under `#[cfg(test)]`. |
| Benchmark convention | `crates/**/benches/**` | Scanner skips only that exact directory shape and rejects a production inclusion edge into it. |
| Test-support crate | `crates/nimbus-testing/` | Exact prefix only; it remains outside product allocation authority. |

The former broad `examples` list is deleted. Production files such as
`nimbus-runtime/src/egress.rs`, CLI auth/token/UI/deploy/start files, and
machine local-server code are no longer wholly exempt; only their exact
test-owned AST nodes disappear from the production view.

The production tree has one computed Rust include:
`crates/nimbus-firebase/src/grpc.rs` includes the Tonic output selected by
`OUT_DIR`. This is not treated as a test exemption or silently claimed as
parsed source. The vendored Firestore proto/build-script boundary is named,
and all three current build-output directories contain the same five generated
Rust files with the same hashes. A direct scan across all fifteen current
outputs finds no `TcpListener`, `UdpSocket`, Unix listener/datagram, `socket2`,
`std::net`, `tokio::net`, `::bind`, `host_port`, or `port_mappings`
occurrence. NNC9.1 owns making generated/compiler-resolved evidence a permanent
verifier condition.

The source-shape risk census finds no unclassified socket alias. It records 36
current occurrences across 21 paths with exact line-fenced reasons:
13 domain/configuration/credential/HostBridge instance methods named `bind`,
2 read-only `SocketDigest` descriptor observations, 1 nimbus-kv call into its
already-classified listener adapter, and 20 macro invocations that format
admitted endpoint data or diagnostics. An authority-shaped macro token stream
must be explained rather than silently ignored, but the source scanner does
not claim macro expansion. Any change to the currently classified authority or
risk set, line drift, or missing reason fails NNCV006. The generated authority
and risk scans agree exactly with their recorded classifications.

## Structured Review Disposition

The first review command was:

```text
AUTOREVIEW_ALLOW_NESTED_CODEX=1 \
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local \
  --engine codex \
  --model gpt-5.6-sol \
  --thinking xhigh \
  --codex-speed fast \
  --prompt 'Review only the staged NNC3.7b source-derived bind/allocation census closure against its written acceptance criteria and proof. Do not broaden into NNC3.8 restart convergence or NNC3.9 PortManager deletion.'
```

The helper confirmed GPT-5.6 Sol, xhigh reasoning, priority/fast service, one
101,846-byte review bundle, and seven findings.

| Finding | Disposition |
| --- | --- |
| Reported undefined `redacted` identifiers in the lexical parser. | Rejected. The review bundle's secret sanitizer replaced the real cursor expression in its display. `node --check`, direct execution, and the full fail-closed harness executed that repository path. The local iterator binding was subsequently renamed to `syntaxCharacter` solely to avoid another sanitizer-ambiguous rendering. |
| Cfg-attributed parameter without a trailing comma could consume its production parent. | Accepted and fixed with delimiter-aware item termination plus an expected-red regression. |
| Instance `.bind`, socket aliases, and UDP descriptor adoption were not all covered. | Accepted and fixed with explicit UDP adoption patterns, ambiguous member/chained-bind classification, alias rejection, and focused regressions. |
| The broader allocation/provider scan commands were prose rather than enforced census inputs. | Accepted and fixed for the current high-signal allocation and provider-request shapes, including exact current classifications and synthetic expected-red proof. |
| Path-owned module evidence used a regex-sensitive name and weak parent relationship. | Accepted and fixed with escaped names plus exact cfg-owner, declared-child, and resolved-path checks; a regex-shaped module fixture now fails closed. |
| Same-path logical site IDs could be swapped. | Accepted and fixed with per-site allowed authority kind/symbol contracts and a swap regression. |
| Symbol-presence accepted an identifier anywhere in the file. | Accepted and fixed with one exact declaration name and line per declaration-only site plus a stale-declaration regression. |

The required corrected-candidate review used the same engine settings against
one immutable 139,088-byte bundle (thread
`019fa5ae-cade-7cb2-a011-e3429b939358`). It produced six findings, all accepted
after direct reproduction:

| Finding | Disposition |
| --- | --- |
| A cfg-owned `macro_rules!` item could blank following production code. | Accepted; old candidate exited `0`. Brace-terminated macro/block handling and expected-red proof added. |
| Pre-bound listener parameters, fields, and returns were not source-derived. | Accepted; old candidate exited `0`. Exact by-value slot and return-handoff occurrences are now classified across all current composition/adapter children. |
| Recorded `reserve_port` scan was not executable. | Accepted; old candidate exited `0`. `reserve` is now an allocation action with canonical network/sandbox classifications. |
| Provider requests depended on current initializer spelling. | Accepted; old candidate exited `0`. Fully qualified known requests and port-bearing generic request literals are now structural candidates. |
| Index/`?` and other postfix `.bind` receivers escaped. | Accepted; old candidate exited `0`. One complete instance-bind scan preserves member/chained diagnostics and adds postfix classification. |
| Rust `type` aliases escaped import-alias rejection. | Accepted; old candidate exited `0`. Direct authority type aliases now fail closed. |

The next whole-item review used the same engine settings against one immutable
166,295-byte bundle (thread
`019fa5c4-d17c-7b52-b081-6cf7304258e7`). Its first submission stopped at the
privacy preflight and produced no model verdict; after removing the
code-shaped prose trigger, the actual Sol review produced five findings:

| Finding | Disposition |
| --- | --- |
| A cfg-owned brace macro invocation could hide following production code. | Accepted. The structural visitor skips the exact attributed macro item, while an expected-red fixture proves the following bind remains visible. |
| `UnixDatagram` bind/adoption/ownership was absent. | Accepted. Direct bind, `from_std`, `from_raw_fd`, aliases, by-value ownership, and return handoffs are structural authority shapes with focused proof. |
| Shorthand `ProviderRequest { host_port }` escaped. | Accepted. `ExprStruct` field members are inspected independently of explicit/shorthand initializer spelling. |
| Tuple-struct listener ownership escaped. | Accepted. Every AST `Field`, including unnamed fields and enum variants, is visited. |
| Multiline listener return types escaped. | Accepted. Return ownership is derived from the parsed `ReturnType`, independent of source layout. |

The repeated syntax failures were treated as one architecture finding: the
lexical parser was not a defensible exhaustive seam. The accepted correction
is the standalone structural scanner, not five more regex fragments.

The first review of that structural candidate used one immutable 204,091-byte
bundle (thread `019fa5e2-504f-7a10-9192-d8a349aa6993`) and again confirmed the
actual engine as GPT-5.6 Sol, xhigh, fast. It produced four findings, all
verified as direct NNC3.7b acceptance blockers:

| Finding | Disposition |
| --- | --- |
| Conventional `tests`/`benches` paths and named whole-file exemptions could be included by production code. | Accepted. The scanner now enumerates every Rust file and rejects any unguarded production `mod`, `#[path]`, or `include!` edge into a conventional or explicit exemption. Exact cfg-owned inclusion remains allowed. |
| Bind/adoption functions used as values or through bare imports/calls escaped. | Accepted. Known associated function paths are classified from every `ExprPath`; bind/adoption imports and bare calls become explicit fail-closed risks. |
| Trait/impl associated socket aliases escaped the top-level alias guard. | Accepted. Both trait defaults and impl associated types are rejected when they contain a socket authority type. |
| Cfg filtering missed associated items, locals/statements, field values, and match arms. | Accepted. Attribute-aware visitors now cover top-level, impl, trait, foreign, statement, expression, arm, field/value, parameter, and variant boundaries, with negative and positive regressions. |

The scope governor confirmed all four fixes remained inside the same
standalone scanner/shell-proof owner, changed no product source or
public/storage/provider contract, and directly proved the written
current-baseline and narrow-exemption criteria. Because those accepted fixes
changed executable behavior, one corrected-candidate rerun was appropriate.

That fifth review used one immutable 226,745-byte bundle (thread
`019fa5f6-9922-7200-8915-6b77e3664cfa`) and again confirmed GPT-5.6 Sol,
xhigh, fast. Its six findings exposed a scope error in the proof: the document
was presenting a source AST census as if it were a compiler-semantic proof.
The findings were reclassified against the literal NNC3.7b baseline
acceptance before any further scanner edit:

| Finding | NNC3.7b disposition |
| --- | --- |
| `cfg_attr(..., path = ...)` can select a module path that is not represented by a direct `#[path]` attribute. | Accepted as NNC9.1 verifier hardening, not a current-baseline miss. A direct production scan finds no current conditional path attribute. |
| Computed or non-`.rs` `include!` targets are outside the scanner's file walk. | Accepted as NNC9.1 generated/compiler-resolved hardening. The one current computed production include is the named Firebase/Tonic boundary; all fifteen current generated Rust outputs were directly inspected and contain no socket/bind/port authority. |
| `bind_addr` and `From`-based standard-library socket adoption are not in the bounded source-shape table. | Accepted as NNC9.1 compiler-resolved hardening. A direct production scan finds no current occurrence. |
| Qself function values and glob-imported bare `bind` can require name resolution. | Accepted as NNC9.1 compiler-resolved hardening. A direct production scan finds no current qself or relevant glob-import occurrence. |
| An associated alias can name `Self` inside a socket-type impl. | Accepted as NNC9.1 compiler-resolved hardening. A direct production scan finds no current socket `Self` alias. |
| A cfg-test field inside a provider-request literal is inspected before the generic field-value visitor filters it. | Accepted as NNC9.1 source/compiler hardening. Current cfg-test fields exist, but none is a port-bearing provider request; the exact current census remains unchanged and complete. |

No finding identifies an unclassified current production bind, adoption,
allocation, inherited socket, or provider request. The correction after this
review is documentation and sequencing only: NNCV006 is an exact baseline
census plus bounded source regression guard, while NNC3.9/NNC9.1 own final
compiler-resolved/generated-code closure. There is no sixth review: no
executable candidate changed after the fifth review, and another synthetic
syntax hunt would repeat the invalid semantic-completeness premise rather than
test NNC3.7b acceptance.

## Verification Ledger

| Gate | Command | Result |
| --- | --- | --- |
| Inventory JSON and derived counts | `jq empty ...nnc0.1-bind-owner-inventory.json` plus exact count assertions | Pass; 26/24/2 sites, 67 authorities/ownership handoffs, and 36 non-authority risks agree with the summary. |
| Focused census | `node scripts/verify-nimbus-network-bind-census.mjs --inventory ...` | Pass; no output, exit `0`. |
| Candidate regeneration | same helper with `--print-candidates` and `--print-risks` | Pass; 67 authorities across 25 paths and 36 classified risks across 21 paths; exact inventory diffs are empty. |
| Structural scanner behavior | `cargo test --manifest-path scripts/nimbus-network-bind-census-ast/Cargo.toml --locked --offline` | Final replay passes 8/8 direct AST cases. |
| Structural scanner quality | standalone `cargo fmt --check`, `cargo check --all-targets`, strict `cargo clippy --all-targets -- -D warnings`, and warning-denied `cargo doc --no-deps`, all `--locked --offline` | Pass. |
| Same-file unclassified bind | synthetic first/second authority fixture | Pass; second authority is the exact NNCV006 failure. |
| Cfg-field visibility | synthetic cfg field followed by production bind | Pass; production authority is the exact NNCV006 failure. |
| First-review fail-closed cases | cfg parameter, import alias, UDP raw-socket adoption, instance bind, generalized allocator, provider request, site swap, declaration drift, and regex-shaped exemption fixtures | Pass; all nine produce the exact expected NNCV006 failure. |
| Second-review fail-closed cases | cfg macro, listener consumer, reserve allocator, generic/qualified provider request, postfix bind, and Rust type alias | Pass; old candidate exited `0` for all six; corrected cases produce exact NNCV006 failures. |
| Third-review fail-closed cases | cfg brace macro, Unix datagram, shorthand provider request, tuple ownership, and multiline return | Pass; all five produce exact NNCV006 failures under the structural scanner. |
| Fourth-review fail-closed cases | production inclusion of exempt source, bind function value, bare bind import/call, associated alias, and cfg-associated/statement filtering | Pass; four negative cases fail precisely as NNCV006 and the cfg-only control remains accepted. |
| Fifth-review baseline audit | direct `rg` scans for conditional module paths, alternate standard socket constructors, qself/relevant glob binds, socket `Self` aliases, and cfg-test provider-request fields | Pass for the current baseline: no authority occurrence; current cfg-test fields are not provider requests. |
| Generated-code boundary | enumerate the three `target/debug/build/nimbus-firebase-*/out` trees, hash all five Rust outputs per tree, and scan all fifteen outputs for socket/bind/port authority patterns | Pass; each corresponding file hash is identical across the three trees and the authority scan exits `1` with no match. |
| Genuine cfg module | synthetic test-only module | Pass; exit `0`. |
| Stale classification | synthetic deleted authority classification | Pass; exact stale-key diagnostic. |
| Syntax | `node --check ...bind-census.mjs`; `bash -n ...control-plane.sh` | Pass. |
| Formatting | `npx prettier --check scripts/verify-nimbus-network-bind-census.mjs` | Pass. |
| Aggregate verifier self-tests | `bash scripts/verify-nimbus-network-control-plane.sh --self-test` | Final replay passes 44/44, including all twenty-five bounded scanner cases. |
| Live verifier | `bash scripts/verify-nimbus-network-control-plane.sh` | Final replay exits `1` with 14 pass and exactly one fail; NNCV005 is the NNC3.9-owned `PortManager` deletion, while NNCV006 passes. |
| Dependency invariant | Cargo metadata plus live NNCV004/NNCV012 | Pass; `nimbus-network -> nimbus-core` remains the only workspace edge and no transport/provider effect enters the crate. |
| Network behavioral regression | `cargo nextest run -p nimbus-network --all-targets --no-fail-fast` | Final replay passes 115/115, 0 skipped. |
| Rust formatting | `cargo fmt --all --check` | Pass. |
| Focused crate check | `cargo check -p nimbus-network --all-targets --locked` | Pass. |
| Strict focused Clippy | `cargo clippy -p nimbus-network --all-targets --locked -- -D warnings` | Pass. |
| Warning-denied rustdoc | `RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-network --no-deps --locked` | Pass. |
| Documentation gates | `bash scripts/check-docs.sh`; `bash scripts/verify-nimbus-docs-site.sh` | Final replay passes; 108 pages link-clean and 17/17 site conditions. |
| Structured review | actual GPT-5.6 Sol, xhigh reasoning, fast mode | First run: 6 accepted/fixed, 1 rejected with executable evidence. Second run: 6 accepted/reproduced. Third run: 5 accepted syntax failures resolved by replacing the lexical parser. Fourth run: 4 accepted structural-boundary findings fixed with direct proof. Fifth run: 6 findings accepted as NNC9.1 semantic/generated-code hardening after current-baseline scans proved none is an NNC3.7b authority miss; its overbroad semantic premise is corrected without executable changes. No further review is required. |

## Changed Paths

- `docs/private/plans/nimbus-network-control-plane-plan.md`
- `docs/private/plans/README.md`
- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`
- `docs/private/plans/proof/nimbus-network-control-plane/nnc3.7b-bind-allocation-census.md`
- `scripts/verify-nimbus-network-bind-census.mjs`
- `scripts/verify-nimbus-network-control-plane.sh`
- `scripts/nimbus-network-bind-census-ast/Cargo.toml`
- `scripts/nimbus-network-bind-census-ast/Cargo.lock`
- `scripts/nimbus-network-bind-census-ast/src/main.rs`

No push or PR is authorized.
