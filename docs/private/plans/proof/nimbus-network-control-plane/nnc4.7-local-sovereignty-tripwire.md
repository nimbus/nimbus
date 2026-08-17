# NNC4.7 — Local Sovereignty Tripwire

Status: `done — acceptance, review disposition, and closeout green`

Source checkpoint:

- commit: `df85ca12028c9433b083dcb1dab7a23c71d4c98f`
- tree: `27d1bced4d77b6633c1e624d8b5bac839f9808f8`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- original dirty checkout and clean `machine-os` companion: inspected only,
  unchanged

## Owner Boundary

NNC4.7 owns one provider-neutral, privileged test adapter that falsifies the
already-declared local sovereignty profile inside an outer Linux isolation
boundary. It does not add a production `NetworkProvider`, move nft/netns/KVM
effects into `nimbus-network`, absorb the egress PDP/PEP, invent a name
provider, or run the complete Nimbus workload lifecycle. NNC9.2 will reuse this
tripwire for the pre-staged offline lifecycle.

The closed capability values remain source declarations:

- container attachment:
  `nimbus-sandbox.container.host-managed-attachment`;
- krun attachment: `nimbus-sandbox.krun.host-managed-attachment`;
- local ingress: `nimbus-server.tcp-listener`;
- local-only control plane, no required external dependency, offline restart
  claimed by those exact source owners.

NNC4.7 supplies runtime evidence for isolation and hidden network attempts. It
does not upgrade a capability declaration into provider readiness, KVM
availability, a successful bind, current PEP readiness, or lifecycle proof.
The exact `nimbus-network -> nimbus-core` workspace edge remains unchanged.

## Expected-Red Evidence

The audit ran from the source checkpoint above:

```text
cargo test -p nimbus-sandbox --test krun_linux_egress -- --list
# exit 0
# 0 tests, 0 benchmarks

test -e scripts/nimbus-network-sovereignty-tripwire.sh
# absent

rg 'sovereignty.*tripwire|tripwire.*sovereignty' .github/workflows
# absent

ssh -o BatchMode=yes -o ConnectTimeout=5 minicloud true
# exit 255
# Could not resolve hostname minicloud
```

This is the expected failure for NNC4.7, not a passing provider lane. The
Linux-only krun target disappearing into a zero-test Darwin success is
specifically insufficient. Both existing egress workflows react to krun paths
but execute only `container_linux_egress`; no named KVM/minicloud tripwire job
exists.

The focused declaration/selection baseline passed 15 tests across
`nimbus-network`, `nimbus-sandbox`, `nimbus-server`, and `nimbus-machine`.
That is useful source-level evidence, but it does not prove:

- an outer namespace/private-veth boundary;
- direct loopback and enumerated private-peer success;
- unenumerated private or public IPv4/IPv6 denial and named counter deltas;
- UDP and TCP DNS recording;
- descendant network syscall tracing;
- positive-control reset to a zero payload baseline;
- exact teardown and same-identity re-entry;
- a durable successful evidence bundle.

## Canonical Harness Seam

The implementation stays in concept-owned proof tooling above
`nimbus-network`:

- one thin live command at
  `scripts/nimbus-network-sovereignty-tripwire.sh`;
- concept-owned orchestration/evidence modules under
  `scripts/nimbus_network_sovereignty_tripwire/`;
- deterministic fake-effect and evidence mutation tests under
  `scripts/nimbus-network-control-plane/`;
- one versioned machine-readable result plus hashed raw artifacts.

No production Rust source or new provider interface is required. A later
provider-specific KVM regression lane consumes this seam; it must not fork the
evidence or status authority.

## Frozen Result Contract

The three results are disjoint:

| Result | Exit | Meaning |
| --- | ---: | --- |
| `PASS` | 0 | Named Linux proof entered; every control, reset, profile, evidence, and cleanup assertion passed. |
| `SKIPPED` | 77 | Non-Linux host or unavailable privilege, KVM-required capability, or preinstalled tool. Minimal evidence records the exact reason; no pass assertion is legal. |
| `FAIL` | any other nonzero | Invalid input/runner identity, or a proof phase, payload, evidence, or cleanup failure after admission. |

CLI misuse and unsafe identifiers are `FAIL`, not `SKIPPED`. A required named
runner must treat exit 77 as missing evidence. Preflight writes evidence but
performs no namespace, nft, route, resolver, process, or socket effect.

## Frozen Runtime Assertions

The live profile must prove all of the following on one exact named privileged
Linux minicloud/KVM runner:

1. exact runner identity, kernel, architecture, UID/capabilities, KVM posture,
   preinstalled tool paths/versions, source commit/tree/dirty state, and
   harness digest are recorded;
2. one host-global lock admits only one profile;
3. fresh run-qualified subject and peer namespaces contain the only veth
   endpoints; collisions are refused rather than deleted;
4. the subject has loopback plus one RFC1918 and one ULA address, exact private
   peer routes, and no `CAP_NET_ADMIN` in its traced probe;
5. the private peer does not forward;
6. loopback and the exact IPv4/IPv6 private peers are reachable;
7. an unenumerated private destination is denied and classified;
8. documentation-only numeric public IPv4 and IPv6 attempts increment their
   exact named deny counters and do not reach a real external service;
9. named UDP and TCP DNS attempts are counted and recorded with unique control
   names;
10. `strace -ff -yy -e trace=%network` covers the complete launched probe tree
    and records the expected loopback/private/DNS/public attempts;
11. controls are validated before counters, captures, and traces rotate;
12. the post-reset baseline is exactly zero before a benign loopback/private
    profile runs;
13. the benign profile succeeds with zero unexpected DNS, denied IPv4, denied
    IPv6, or unclassified output;
14. capture/trace writers stop before hashes are finalized;
15. cleanup removes exact child processes, namespaces, veths, nft state,
    resolver overrides, and lock ownership; cleanup failure overrides a would-
    be pass;
16. an immediate second run with the same logical runner identity starts from
    an absent-resource baseline.

The harness performs no apt/dnf/brew install, image/registry pull, git fetch,
DNS-dependent lookup, cloud API, hosted certificate, relay, or external
control-plane call after the proof boundary is active.

## Frozen Self-Test Matrix

Deterministic tests must independently reject:

1. a non-Linux result reported as pass;
2. missing root/effective capability reported as pass;
3. required KVM unavailable but reported as pass;
4. a missing required tool reported as pass;
5. missing, unsafe, or mismatched runner identity;
6. mutation before preflight admission;
7. failed loopback or IPv4/IPv6 private-peer positive control;
8. absent unenumerated-private denial;
9. absent IPv4 deny counter delta;
10. absent IPv6 deny counter delta;
11. absent UDP DNS count/record;
12. absent TCP DNS count/record;
13. empty or incomplete network syscall trace;
14. a reset with residual counters, DNS capture, or trace;
15. unexpected DNS, denied output, or destination after reset;
16. a successful profile with failed positive controls;
17. timeout/signal without cleanup;
18. leaked namespace/interface/nft/resolver/capture/trace effect;
19. missing or tampered artifact/digest;
20. contradictory `PASS` plus a skip reason;
21. a hidden download/install/pull command;
22. cleanup failure beneath an otherwise passing profile;
23. missing required assertion identity;
24. `SKIPPED` translated to process success.

Self-tests use fake command/effect adapters and bounded semantic checkpoints;
they do not require privilege, KVM, wall-clock sleeps, or network access.

## Implemented Evidence And Fail-Closed Corrections

The implementation keeps the live effect adapter outside every production
crate. `environment.py` admits one exact runner and source identity,
`isolation.py` owns the privileged namespace/nft/process lifecycle,
`probe.py` runs as UID/GID 65534 with every ambient/bounding/effective/
inheritable/permitted capability cleared, and `evidence.py` independently
derives the versioned result from authenticated raw artifacts.

Three read-only audits found false-pass risks before candidate freeze. All were
accepted and corrected:

- the artifact manifest is now an exact census, excluding only
  `evidence.json`; topology, nft rules/counter JSON, UDP/TCP capture, both
  syscall traces, probe stdout, attempt JSON, and every command stream are
  authenticated and cross-checked;
- top-level and phase assertions are derived from raw probe booleans, exact
  counters, captures, and traces rather than trusted as summaries;
- every required effect command must exit zero; only exact veth absence probes
  and signal-driven peer/tcpdump termination may use their classified nonzero
  results;
- CAP_KILL and CAP_SETPCAP join the other five required runner capabilities,
  while the subject proves zero capability sets, UID/GID 65534, and
  `NoNewPrivs: 1`;
- shell-wrapped as well as direct install/download/pull commands fail closed;
- invalid named-runner/provider identity is `FAIL` even when the host would
  otherwise be unsupported;
- fresh-process re-entry binds two independently validated PASS documents only
  when named runner, Linux boot, source, inputs, and resource identities match;
  process PID/start-tick identity differs; predecessor cleanup is exact; and
  successor start follows predecessor finish from an absent baseline.

The deterministic suite passes `70/70` with zero skips. It includes positive
controls plus
mutations for omitted/unmanifested artifacts, contradictory assertions,
synthetic phase results, raw failed probes beneath passing summaries, required
effect failure, every required capability, shell-wrapped downloads, process
group timeout/signal cleanup, collision non-deletion, two-process lock
contention, no-follow parent traversal, exclusive file creation, stable resolved
tool identity, non-spawning pre-admission, root/non-root wrapper isolation,
`SKIPPED`/77, identity precedence, exact repeat count, complete time intervals,
and fresh-process pair ordering.

The 1,940-line test executable remains one explicit review-band exception. It
is a test-only contract owner: its canonical synthetic evidence fixture,
raw-artifact mutation helpers, environment classifier matrix, detector matrix,
and process/cleanup harness execute under one `unittest` entry point against the
same schema. The subprocess-heavy wrapper fixture moved to the concept-owned
98-line `sovereignty_tripwire_wrapper_harness.py` rather than allowing the main
owner to cross 2,000 lines. The production-facing `evidence.py` and
`isolation.py` owners remain below the 1,500-line threshold at 1,477 and 1,479
lines; integrity, synchronization, and privileged workspace ownership are
separate small modules. No further logic may accrete into the test owner
without revisiting this exception.

## Structured Review Disposition

The one complete item review used GPT-5.6 Sol, xhigh reasoning, fast mode over
the 242,432-byte candidate bundle. It reported eight findings and correctly
classified the candidate as not ready at 0.99 confidence. All eight were
accepted:

| ID | Finding | Disposition and proof |
| --- | --- | --- |
| F1 | Inherited Python import paths at privileged entry | Corrected with isolated `-I -S`, discarded `PYTHONHOME`/`PYTHONPATH`, fixed source injection, and a malicious current-directory/Python-path test. |
| F2 | PATH-selected tools before admission and bare effect tools | Corrected by deciding identity/substrate first, discovering root-owned absolute tools only after admission, substituting exact paths for every effect, and rejecting unauthenticated recorded paths. |
| F3 | Reused/symlink evidence paths under root | Corrected with a new exclusive output directory, component-by-component `O_NOFOLLOW` traversal, root ownership/mode checks, and exclusive no-follow artifact creation. |
| F4 | Effect ownership registered after fallible evidence writes | Corrected with deferred termination and a success callback that records namespace/veth ownership before stream persistence; injected persistence failure proves cleanup ownership survives. |
| F5 | Claimed Git identity was only shape-checked | Corrected by independently recomputing and comparing commit, tree, and dirty state with fixed `/usr/bin/git`; three claim mutations fail. |
| F6 | Wrapper test skipped on Linux | Corrected with one deterministic root/non-root subprocess harness; the final 70-test suite has zero skips. |
| F7 | Incomplete run/pair time intervals | Corrected by requiring timezone-aware start/finish timestamps, validating both intervals, then ordering predecessor finish before successor start. |
| F8 | CLI accepted repeat counts that validation rejected | Corrected by admitting exactly two attempts and rejecting every other count before effects. |

Those executable corrections required the one permitted narrow Sol/xhigh/fast
correction review. It reviewed one 268,699-byte bundle and found five additional
privilege-boundary defects plus one stale-ledger defect, again correctly
rejecting that intermediate candidate at 0.99 confidence. All six were
accepted and corrected:

| ID | Finding | Disposition and proof |
| --- | --- | --- |
| C1 | `/usr/bin/env bash` and bare Make `bash` preceded isolation | Corrected with direct `#!/bin/bash -p`, fixed `/bin/bash -p` Make execution, discarded shell startup variables, and malicious `BASH_ENV`/`ENV` proof. |
| C2 | `platform.platform()` could spawn before admission | Corrected with a non-spawning kernel fact string; an invalid-runner test proves the helper and every tool/version adapter remain uncalled. |
| C3 | Tool discovery executed the unresolved candidate | Corrected by authenticating lexical and resolved ownership, rejecting wrong-binary symlinks, and executing/recording only the stable resolved path. |
| C4 | Intermediate evidence-parent symlinks were followed | Corrected by walking every absolute parent component from a root descriptor with `O_DIRECTORY|O_NOFOLLOW`; an intermediate-symlink mutation fails. |
| C5 | Wrapper skip was not deterministic under UID 0 | Corrected by copying the exact wrapper/package to a controlled fixture and dropping a root test child to UID/GID 65534 before preflight. |
| C6 | Ledger counts remained pre-correction | Corrected here and in the plan index/recovery ledger with 70 tests, 1,940/98 test owners, and final proof hashes. |

No third structured review ran: the owner cadence permits exactly one full item
review and one narrow correction review. After C1-C6, owner/manual inspection,
Ruff, compile, ShellCheck, 70 deterministic tests, 17 static checks, 62
adversarial verifier cases, and two fresh privileged source-matched runs provide
the correction proof.

## Named LinuxKit Proof

The exact pre-staged image was
`local/nnc47-minicloud@sha256:faa980f3b551b9b30b00a543d567b80e8e838fd1341a31f0d5ce664d50523417`.
No pull, package install, source fetch, DNS-dependent lookup, or external
control-plane operation occurred inside either proof run.

Both independent container invocations used `--pull=never --network none
--pid=host --uts=host --privileged`, mounted the owner source and Git metadata
read-only, and ran the same `nnc47-linuxkit` identity on hostname
`docker-desktop`. Observed facts were:

- LinuxKit `6.12.76-linuxkit`, `aarch64`, PID 1 `/initd`, boot
  `1000f2ed-b6bc-4e4d-8e5d-ca6b269d69a6`, UID 0, and effective capability mask
  `000001ffffffffff`;
- exact tools: Git 2.43.0, iproute2 6.1.0, nftables 1.0.9, Python 3.12.3,
  util-linux/setpriv 2.39.3, strace 6.8, procps/sysctl 4.0.4, tcpdump 4.99.4,
  GNU hostname 3.23, and GNU uname 9.4; effect execution used the stable
  resolved `/usr/bin/ip` and `/usr/bin/python3.12` paths;
- source commit `df85ca12028c9433b083dcb1dab7a23c71d4c98f`, tree
  `27d1bced4d77b6633c1e624d8b5bac839f9808f8`, and executable harness digest
  `ae1c4124b3a8708325aea8ccd7259661f14ca12382741f4902c89cabbf9e17a5`;
- each process completed two attempts, 96 recorded commands, 210 authenticated
  artifacts, and all 20 stable assertions;
- each attempt observed control counters
  `denied_ipv4=1, denied_ipv6=1, denied_private=1, dns_tcp=1, dns_udp=1,
  unexpected=0`; every reset and post-reset profile counter was zero.

| Run | UTC interval | Process identity | Evidence SHA-256 | Manifest SHA-256 | Archive SHA-256 |
| --- | --- | --- | --- | --- | --- |
| predecessor | `05:15:24.546852`–`05:15:35.302725` | `82387@1398829` | `0d9e198833e8f33d49be104688d7a6096d66d331d35fe2b67922a774ff3e569c` | `d31a89a5b36b443793736c645880e221a28fbac24bb07eafad0947d12439b14b` | `9b6943f1679bb7da3bf1d255ad923521f8bbd03feb3a7bede437eb9b11aef9c2` |
| successor | `05:16:03.801974`–`05:16:14.077456` | `84795@1402754` | `c633937360b3cae0eba662054ed5fd3040ab1117db58e41bc68baa32d3321c3d` | `d7fc6f2d70ec3a0f2aea857ef8bfa1b48bf9c6b01ccb20c6b334db1d451a4859` | `7918aa5f0dffe91638812d94aa81e6d6afbf9a1b35c09ee5c3bc6b24fab7919f` |

Both assertion-ID sets have SHA-256
`a38b6b83534db978f8aa090b52d665e3d19c51977897923b7790c136b3fdd1c8`.
The independent verifier accepted each document and then printed
`sovereignty tripwire fresh-process re-entry: PASS` for the ordered pair. The
exact `nnc47-minicloud` container was removed after the proof; the pre-staged
local image remains as the offline proof input, and no proof container remains.

One earlier direct-entry correction run honestly returned `SKIPPED`/77 because
the first stable-path implementation treated the meaningless writable bits on
root-owned symlinks as target mutability and therefore rejected the legitimate
`ip` and `python3` links. It created no effect commands. The resolver now
authenticates the root-owned symlink and its immutable parent, executes the
resolved regular target, and rejects links whose target binary name does not
match. The 70-test suite and the two final PASS processes above prove that
correction; the skipped bundle is not cited as passing evidence.

## Acceptance Ledger

| Gate | Status | Evidence |
| --- | --- | --- |
| Read-only ownership/call-graph audit | `done` | Three independent read-only audits plus owner source inspection; no paths changed by audit agents. |
| Expected-red proof | `done` | Zero-test Darwin target, absent harness/workflow, unresolved historical proof host recorded above. |
| Status/evidence contract | `frozen` | This proof defines `PASS`/0, `SKIPPED`/77, and fail-closed nonzero semantics. |
| Deterministic self-tests | `done` | `70/70`, zero skips: fake-effect, evidence-mutation, root-safety, cleanup, classifier, stable-tool, interval, CLI, and isolated-wrapper tests pass in 5.268 seconds or less across correction runs. |
| Local unsupported-host proof | `done` | The wrapper subprocess drops a root test to UID/GID 65534 when necessary, writes `SKIPPED` evidence, preserves exit 77, records zero assertions/commands, and ignores malicious shell/Python startup paths; invalid identity still fails before skip. |
| Named privileged live profile | `done` | Two source-matched LinuxKit processes each pass all sixteen runtime clauses, 20 stable assertions, 96 commands, and 210 authenticated artifacts. |
| Re-entry/cleanup proof | `done` | Ordered predecessor/successor processes have distinct PID/start ticks on the same boot and source; pair validation proves predecessor cleanup and successor absent baseline. |
| Capability/dependency regression | `done` | Capability positive/named-negative matrix `15/15`; registry/selection registrations `10/10`; exact workspace edge is `["nimbus-core"]`; static verifier `17/17`; no production Rust changed. |
| Item review | `done` | One full GPT-5.6 Sol/xhigh/fast review found eight accepted defects; the one permitted narrow correction review found five additional executable defects plus stale counts. All fourteen findings are corrected and dispositioned above; no third review ran. |
| Closeout gates | `done` | Ruff/compile/ShellCheck, tripwire static verifier, aggregate verifier `17/17` plus adversarial self-test `62/62`, exact dependency edge, format/diff, website 109, docs 108, site 17/17, and two final source-matched live proofs pass. |

## Known Routing Constraint

`docs/private/plans/README.md` currently names
`nimbus-sandbox-egress-regression-and-seams-plan.md` as the future owner of
provider-specific KVM lanes, but that draft is absent from this worktree and
from `HEAD`; an ignored copy exists only in the original user-owned checkout.
NNC4.7 therefore owns only the generic tripwire and its current named proof.
It will link a durable provider-lane owner if and when that separate draft is
promoted, rather than copying or modifying the user-owned file.
