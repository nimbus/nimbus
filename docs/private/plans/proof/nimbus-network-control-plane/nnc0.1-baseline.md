# NNC0.1 Dependency, Ownership, and Bind Baseline

Status: `passed`

Source branch: `codex/nimbus-network-architecture-audit`

Source HEAD: `e990c018a20b063a0ac093ad0e78b8e71117ec70`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Captured: `2026-07-23`, on `aarch64-apple-darwin`

## Result

The current workspace graph is acyclic in each explicitly captured profile.
The source census classifies 24 production bind, allocation, provider-bind,
pre-bound, inherited-socket, or local-IPC sites; six portable network-owner
sites; and three trust inputs that must not become network authorities. No
production site is left without a disposition.

The durable machine-readable artifacts are:

- `nnc0.1-dependency-graph.json` — declared workspace edges plus resolved
  normal, dev/test/build, all-feature, and target-conditioned profiles.
- `nnc0.1-bind-owner-inventory.json` — exact source locations, current truth,
  disposition, target owner, and implementation item for every classified
  production site.

## Dependency graph proof

The capture script is
`scripts/capture-nimbus-network-dependency-baseline.mjs`. It records the exact
commands in the JSON artifact and preserves, per declared workspace edge:

- dependency kind;
- target condition;
- optionality;
- default-feature behavior; and
- explicitly enabled features.

The resolved cycle profiles were:

| Profile | Target | Resolved edges | Cycles |
| --- | --- | ---: | ---: |
| normal/default | `aarch64-apple-darwin` | 215 | 0 |
| normal + dev/test/build/default | `aarch64-apple-darwin` | 238 | 0 |
| all features/all kinds | `aarch64-apple-darwin` | 244 | 0 |
| all features/all kinds | `x86_64-unknown-linux-gnu` | 244 | 0 |
| all features/all kinds | `aarch64-unknown-linux-gnu` | 244 | 0 |
| all features/all kinds | `x86_64-pc-windows-msvc` | 244 | 0 |

The target-conditioned rows evaluate Cargo's dependency resolution for the
named target. They do not claim that those targets were compiled on this host.
Later band verification must still compile and test each required supported
target in its owning lane.

## Bind and owner census

The source census used the exact `rg` commands recorded in
`nnc0.1-bind-owner-inventory.json`, then inspected each result in its module and
target context. Its classifications are:

| Class | Count | Result |
| --- | ---: | --- |
| Production bind/allocation/effect sites | 24 | Every site has one disposition and implementation owner item. |
| Portable network-owner sites | 6 | Current and target ownership are recorded. |
| Trust inputs, not network authorities | 3 | Zero network authority is assigned. |
| Unclassified production sites | 0 | Pass. |
| Production UDP bind sites | 0 | Absence recorded; UDP still remains in the future conflict model. |

The important current authorities and effect seams are:

- sandbox manifest and PEP port selection are caller-local scans;
- CLI dev/start and managed-machine selection use probe/drop windows;
- server, sibling wire adapters, standalone KV, PEP, and
  `MachinePortProxy` own real TCP binds;
- systemd/pre-bound listener paths already expose adoption seams;
- Netavark, gvproxy `-ssh-port`, and the machine-forwarder `/expose` request
  are provider bind effects, not portable allocation authorities;
- Unix-domain machine API/readiness/ignition sockets are classified local IPC
  and are not absorbed into a TCP/UDP `PortLease`; and
- loopback ephemeral listeners confined to tests, benchmarks, and test-support
  modules are recorded as non-production exemptions.

The inventory is the deletion/migration checklist for NNC3.7b. A production
site cannot disappear from that checklist merely because a new authority
exists; it must be migrated, adopted, or retained as a narrow mechanically
verified exemption.

## Reconciled trust boundaries from PRs #238 and #239

The baseline was regenerated after rebasing onto the two merged trust fixes:

- `CloudFunctionsHttpTenantBinding` remains owned by the Cloud Functions
  deployment registry. Network receives only already trusted and admitted
  tenant attribution.
- `ConvexSiloAuthRegistry` remains owned by Convex/auth/compute/server.
  Network never chooses an authentication verifier or inspects a bearer token.
- `DeploymentArtifactLease` remains compute-owned generation lifetime state.
  It is neither renamed to nor unified with a host-global `PortLease`.

Missing or invalid tenant binding, silo authentication, or admission must cause
zero network calls. These changes therefore refine network preconditions but do
not change the target dependency direction.

## Commands and results

All commands below exited `0`:

```text
node --check scripts/capture-nimbus-network-dependency-baseline.mjs
node scripts/capture-nimbus-network-dependency-baseline.mjs \
  > docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-dependency-graph.json
jq empty docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-dependency-graph.json
jq empty docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
```

Artifact assertions also exited `0` and reported:

```text
declared workspace edges: 244
resolved profiles: 6
cycles in every profile: 0
production sites: 24
duplicate production IDs: 0
unclassified production sites: 0
```

No provider lane or compilation lane was invoked or skipped by this inventory
item. Its proof is metadata and source classification, not runtime provider
behavior.

## Recovery note

Both machine-readable artifacts name source HEAD
`e990c018a20b063a0ac093ad0e78b8e71117ec70`. The uncommitted proof/script edits
do not alter Rust manifests or the source sites being inventoried. If a later
rebase changes a manifest or inventoried production source before its
corresponding migration, NNC0.1 must be regenerated and the source-HEAD field
updated rather than treated as timeless evidence.
