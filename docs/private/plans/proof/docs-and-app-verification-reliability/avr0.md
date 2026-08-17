# AVR0 Baseline and Red Verifier

Date: 2026-08-17

## Result

AVR0 is complete. Commit `e7ea6d220` adds the six missing private routing and
runbook documents and two source-derived verifier entry points. It changes no
product behavior. The live source baseline is red on all 24 conditions. The
condition-specific mutation self-test is green at 24/24.

| Checkpoint | Revision |
| --- | --- |
| Current-main baseline | `82bdcf2db5f7e021bdf701cab13f60e6e138c2cf` |
| AVR0 transition | `37d52204d` |
| AVR0 work commit | `e7ea6d220` |

`git cat-file -e HEAD:docs/private/plans/docs-and-app-verification-reliability-plan.md`
and `git cat-file -e HEAD:scripts/verify-docs-app-verification.sh` both pass.

## AVR0.1 Durable baseline

The branch started clean at `37d52204d`, two commits ahead of
`82bdcf2db`. The plan durability commit `7abfe4409` existed in `HEAD` before
the fetch that confirmed `origin/main` was still the recorded baseline.

The six bootstrap targets were absent from baseline revision `82bdcf2db`:

- `docs/private/README.md`
- `docs/private/operating/README.md`
- `docs/private/operating/local-dev.md`
- `docs/private/operating/verification.md`
- `docs/private/adapters/README.md`
- `docs/private/adapters/convex/ai-guidelines.md`

Each `git cat-file -e 82bdcf2db:<path>` failed. The repository ignores
`docs/private/` by default, so the work commit force-added only these exact
paths. No unrelated ignored file entered the index.

## AVR0.2 Private routing repair

The new documents establish these owners:

| Concern | Owner |
| --- | --- |
| Internal routing and private publication fence | `docs/private/README.md` |
| Operating-runbook routing | `docs/private/operating/README.md` |
| Fresh-checkout and focused local-development commands | `docs/private/operating/local-dev.md` |
| Gate selection, single-flight, exit status, evidence, and application-lane limitations | `docs/private/operating/verification.md` |
| Adapter seam routing and shared trust invariants | `docs/private/adapters/README.md` |
| Convex package, codegen, runtime, mutation, and trusted-silo guidance | `docs/private/adapters/convex/ai-guidelines.md` |

The Convex guide preserves the PR #238 and PR #239 trust boundaries. A URL
silo selects a provisioned verifier before bearer inspection. Caller data does
not become tenant authority. The independent Cloud Functions trusted-tenant
binding remains separate.

## AVR0.3 Effect inventory

### Source and temporary-state effects

| Current effect | Evidence | Risk and owner |
| --- | --- | --- |
| Application codegen runs in tracked workspaces. | `scripts/examples-verify.sh:356`; five app manifests and the root lockfile changed in the earlier live audit. | Source-byte mutation. AVR4. |
| Firebase protobuf generation runs in the repository. | `scripts/examples-verify.sh:185-196`. | Hidden fresh-checkout prerequisite and generated-source effect. AVR3-AVR4. |
| Two dev cases rename tracked `compose.yaml`. | `scripts/examples-verify.sh:110-157`. | Interrupted runs can alter the checkout. AVR5. |
| The runner creates one temporary root. | `scripts/examples-verify.sh:48`. | The EXIT trap stops only the current child and restores Compose. It never removes the root. AVR7. |
| Success is console-only. | The runner ends with `==> all examples verified`; it emits no JSON or JUnit. | Incomplete durable evidence. AVR8. |
| Cases run one loop at a time. | `scripts/examples-verify.sh:410-423`. | Nine serial boot cycles. AVR9. |

### Listener and lease effects

| Listener | Current desired input | Current risk |
| --- | --- | --- |
| Main HTTP/WebSocket listener | The shell binds `127.0.0.1:0`, closes it, and passes the selected port through `--port` (`scripts/examples-verify.sh:39-47,263`). | Scan-close race. AVR7 must consume product lease and listener-adoption authority. |
| MongoDB listener | Enabled by default on 27017 (`crates/nimbus-cli/src/start/mod.rs:103-113`; `start/adapters/mongodb.rs`). | Every case competes for the same host port. |
| DynamoDB listener | Enabled by default on 8000 (`crates/nimbus-cli/src/start/mod.rs:128-138`; `start/adapters/dynamodb.rs`). | Every case competes for the same host port. |
| S3 listener | Enabled by default on 9000 (`crates/nimbus-cli/src/start/mod.rs:152-162`; `start/adapters/s3.rs`). | Every case competes for the same host port. |

Product code already owns durable port leases and retained listener adoption.
The later runner work must consume those seams. It must not add a shell-side
allocator or move concrete socket effects into `nimbus-network`.

### Operator and application state

`crates/nimbus-operator/src/paths.rs:151-246` resolves one platform-global
`LocalServerPaths` value with authentication token, discovery record, and audit
log paths. `nimbus start`, `nimbus dev`, `nimbus run`, auth, UI, deploy, and
machine commands consume that same platform resolution. Concurrent cases can
therefore replace discovery state or share credentials and audit output.

The runner gives each case a data directory under one temporary root. It does
not provide case-local authentication, discovery, audit, application, control,
or log roots. It also does not set `NIMBUS_NETWORK_STATE_DIR`.
AVR7 must keep one run-global network-state root while it isolates the other
roots per case.

### Command effects and prerequisites

| Command path | Current behavior | Gap |
| --- | --- | --- |
| `make examples-verify` | Single-flight wrapper calls the script. | The target has no UI or embedded-package prerequisites. |
| Direct `bash scripts/examples-verify.sh` | Missing binary triggers raw `cargo build -p nimbus-bin --bin nimbus`. | Raw Cargo does not own the fresh-checkout UI and embedded-package graph. |
| Supplied binary | An executable `NIMBUS_EXAMPLES_VERIFY_BIN` skips the Rust build. | This useful fast path must remain. |
| Node | Host currently reports `v26.0.0`. | The runner does not reject versions outside the supported `>=22 <25` range before work. |
| App codegen and smoke | Uses npm workspace commands in the source checkout. | Preparation is not disposable or byte-checked. |

## AVR0.4 Red verifier

`scripts/verify-docs-app-verification.sh` owns AVRC01-AVRC24, their task and
phase mapping, baseline aggregation, and the mutation aggregate.
`scripts/examples-verify-contract-test.sh` owns the application conditions
AVRC11-AVRC24. Both fail closed on unknown selectors.

The baseline result is:

```text
Summary: 0 passed, 24 failed
```

The result is intentionally red. It records the source state before AVR1.
Normal task and phase modes return nonzero for selected red conditions. Only
`--baseline` records red conditions and returns success.

The self-test creates a minimal candidate-green fixture for each condition.
It then changes or removes that condition's load-bearing source input and
requires the same evaluator to fail. It reports:

```text
Mutation summary: 24/24
```

The verifier does not read active plan prose as contract data. AVRC01 avoids a
self-reference by constructing the legacy plan path from separate string
parts. Application predicates inspect implementation or independent fixture
paths, not assertion text in the contract script.

## AVR0.5 Verification evidence

| Command | Result |
| --- | --- |
| `bash -n scripts/verify-docs-app-verification.sh` | Pass. |
| `shellcheck scripts/verify-docs-app-verification.sh` | Pass, no diagnostics. |
| `bash -n scripts/examples-verify-contract-test.sh` | Pass. |
| `shellcheck scripts/examples-verify-contract-test.sh` | Pass, no diagnostics. |
| `bash scripts/verify-docs-app-verification.sh --baseline` | Pass; 0 green, 24 red. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass; 24/24 mutations detected. |
| Invalid `--task AVR99` selector | Exit 2 with `no conditions selected`. |
| Technical-writing lint over six new Markdown files | Pass; 6 files, 0 diagnostics. |
| Relative-link check over six new Markdown files | Pass; no missing target. |
| `bash scripts/check-docs.sh` | Pass; 108 pages, private fence intact. |
| `bash scripts/verify-nimbus-docs-site.sh` | Pass; 17/17. |
| `npm --prefix website run build` | Pass; 109 HTML pages and all three `llms` outputs. |
| `git diff --cached --check` before the work commit | Pass. |

The eight work-commit file digests are:

```text
69148ae9c8d728dc433a1dd8e8992d2d70931ec61a5435aaf25737f3ef6d6af5  scripts/verify-docs-app-verification.sh
c8b7c73636f028dd7643105f8ca5dc196966b547640df7c71438aa85b32026d1  scripts/examples-verify-contract-test.sh
92db58a9e5e13b1feedd160eb4d4d427244186a5acc26a2519792a94eb44914a  docs/private/README.md
9877923e1b0b854155e92219b3fc472c4d0cac3181178fe83cb5d3981f68b868  docs/private/operating/README.md
2218d68de9018c49225ceaf6ec6eca5d5bd7b491f4e9922ea9dfdd0bf4a5a0a8  docs/private/operating/local-dev.md
964533f25dc6b06fab30e128f5bb531e00c6a4f8c303f429ee7545dee737ebf8  docs/private/operating/verification.md
6365ac30c242edbf36a67f34a9278a8030a1d5e885bab4836d6dca212a619f24  docs/private/adapters/README.md
e1b2af2b90730f2bbd030de956472bdebef8eeefeb09688be3f8462519ceee5c  docs/private/adapters/convex/ai-guidelines.md
```

The commit range `37d52204d..e7ea6d220` contains exactly those eight paths.
No `crates/**`, `packages/**`, existing example, Makefile, workflow, public
documentation, or product configuration file changed.

## AVR0.6 Routing forward

AVR1 owns the 13 executable readers of the completed network plan:

```text
scripts/nimbus-network-control-plane/workload-executable-carrier-contract.sh
scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh
scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh
scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh
scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh
scripts/nimbus-network-control-plane/workload-restart-contract-fixture.mjs
scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs
scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh
scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh
scripts/nimbus-network-control-plane/workload-teardown-contract-fixture.mjs
scripts/nimbus-network-control-plane/workload-teardown-source-contract.mjs
scripts/verify-multi-tenant-network.sh
scripts/verify-nimbus-network-control-plane.sh
```

AVR1 must extract stable verifier fixtures before it archives the plan. AVR2
then owns public network documentation. AVR3-AVR10 own the application-lane
gaps recorded above. The AVR0 audit found no new finding that needs an
out-of-scope implementation owner.
