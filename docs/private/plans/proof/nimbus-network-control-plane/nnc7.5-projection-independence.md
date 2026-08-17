# NNC7.5 Projection Independence

Status: `complete`

## Outcome

Failures in `_nimbus` connectivity tables cannot change an authority.
Loss, lag, deletion, rebuild, or a stale write cannot stop a listener or
repeat a machine effect. These failures also cannot change a workload saga,
service definition, or network authority.

## Frozen scope

| Field | Contract |
| --- | --- |
| Item | `NNC7.5` |
| Baseline | `7057fece3897f116a51cde06e972c88258cd913a` |
| Product seam | A `nimbus-system` projection-only retry coordinator over typed NNC7.4 observations. |
| Source owners | Server listener leases, compute-authenticated workload observations, services state, and machine manager snapshots. |
| Forbidden | No `nimbus-network` edit, socket or provider effect, desired-state mutation, lease transition, workload-saga transition, logical-name authority, public provider interface, TLS/telemetry work, or NNC8 recovery work. |
| Review | The full GPT-5.6 Sol, xhigh, fast review and the one allowed narrow correction review are complete. All five accepted P2 findings are corrected and proven. Review cadence is exhausted. |

## Source-derived failure census

| ID | Current behavior | Required behavior |
| --- | --- | --- |
| F1 | `serve_leased` awaits the main listener projection before router construction. A write failure unwinds the socket and settles its active lease. | Validate immutable observation input before activation. Queue projection work after authority exists. Projection failure cannot stop serving or settle the lease. |
| F2 | A sibling projection failure drops the socket, settles its lease, and fails the complete listener group. | Keep the prepared listener and lease. Queue projection work independently. A retry writes the same stable listener and port identities. |
| F3 | Machine create, start, stop, restart, update, and delete return an error after the manager effect if `_nimbus` snapshot or event recording fails. | Return the authoritative manager result. Queue snapshot or deletion projection independently. Report projection failure through bounded structured warnings. |
| F4 | Typed service connectivity records have tests and a drift fixture but no production writer from the already-authenticated workload observation. | Project the exact authenticated service observation after the services-owned sink succeeds. System failure cannot change the sink result or saga state. |
| F5 | Listener and service writers overwrite by document ID without a generation or lease-epoch stale-write guard. A stale service write deletes current children before replacement. | Reject a lower fence before any child delete or overwrite. An equal replay repairs missing rows. |
| F6 | Projection rows can be deleted after a successful write, but there is no source-retaining rebuild trigger for the current process. | Retain only immutable observed snapshots in the projection coordinator. An automatic production refresh and an explicit rebuild mark them dirty without calling an authority or effect owner. A rebuild advances its request revision so an older in-flight completion cannot acknowledge it. |

## Acceptance

| ID | Falsifiable success criterion |
| --- | --- |
| A1 | `listener_projection_failure_keeps_every_listener_active_and_retries` injects the former kth projection failure. Both listeners serve, both leases remain `Active`, and the retry creates the exact listener and port rows. |
| A2 | A projection retry never calls bind, adopt, close, settle, or any provider command. A source scan proves the coordinator imports no network manager, lease authority, socket, sandbox, proxy, or machine effect API. |
| A3 | `projection_rebuild_restores_deleted_rows_without_touching_authority` deletes projected rows, snapshots the network authority bytes, triggers rebuild, and gets the same rows with byte-identical authority. |
| A4 | `stale_listener_projection_cannot_replace_a_newer_fence` writes generation or epoch N+1, then N. The N+1 listener and port documents remain exact. |
| A5 | `stale_service_projection_cannot_delete_newer_children` writes a newer service and children, then submits an older source, attachment, or listener fence. Every newer row remains exact. |
| A6 | An equal listener or service replay repairs a missing projection row and does not mutate source authority. |
| A7 | `machine_lifecycle_succeeds_while_projection_retries` injects a system write failure after one manager effect. The API succeeds, the manager call count stays one, and retry records the snapshot. |
| A8 | Machine deletion projection failure does not repeat or reverse manager deletion. Its retry removes only `_nimbus` machine/connectivity rows. |
| A9 | The services sink remains authoritative for logical observation. A system projection failure does not change its return value, service catalog, workload record, or provider call count. |
| A10 | The production service projection authenticates exact tenant, source generation, attachment ID/generation/provider, endpoint ID, listener ID, port lease ID, lease epoch, application protocol, guest port, and actual address from the accepted workload observation. |
| A11 | Retry work is coalesced by stable projection identity, cancelled with Engine shutdown, and uses bounded exponential backoff. Permanent failure cannot busy-loop or grow one entry per retry. |
| A12 | `nimbus-network -> nimbus-core` remains its only workspace edge. All affected behavior, strict quality, static verifier, docs, and proof gates pass. |

## Fail-before evidence

| Failure | Baseline observation |
| --- | --- |
| Listener projection | The injected projection write error unwound the prepared listener group. A client got a connection reset, and lease cleanup started. |
| Machine projection | The start route returned HTTP 500 after the manager had completed one start effect. |
| Stale connectivity | Older listener and service observations replaced newer rows. The service writer also deleted newer child rows before replacement. |

## Implementation evidence

- `nimbus-system` owns one retained projection runtime. It coalesces work by
  stable identity and retries only typed `_nimbus` writes.
- Listener, service, and machine effect owners submit immutable observations
  after their authoritative operation succeeds.
- Listener and service row sets use one atomic write batch. Generation,
  attachment generation, and listener lease epoch reject stale work before a
  delete or replacement.
- Equal replay repairs deleted rows. A 30-second production refresh repairs
  retained projections. Explicit rebuild advances each request revision and
  wakes the driver. Neither path inspects or calls an authority.
- The retry delay starts at 25 ms and has a 2-second upper bound. Engine shutdown
  cancels the driver. A permanent failure retains one coalesced entry.
- `nimbus-network` has no product change. Provider, socket, lease, policy,
  service-name, and workload-saga authority stay in their existing owners.

## Acceptance evidence

| Criterion | Result | Evidence |
| --- | --- | --- |
| A1 | `pass` | The listener failure test serves through both listeners, retains both active leases, and observes the exact retried listener and port rows. |
| A2 | `pass` | The projection runtime imports Engine plus typed record values only. Its source contains no socket, network-manager, lease-authority, sandbox, proxy, or machine-manager effect call. |
| A3 | `pass` | The explicit and automatic rebuild tests delete projection rows and restore them. Durable port-lease authority is byte-identical before and after. A state-machine regression proves an older in-flight success cannot clear a newer rebuild revision. |
| A4 | `pass` | The stale listener test proves lower generation and lease epoch are no-ops. |
| A5 | `pass` | The stale service test proves lower source, attachment, or listener fences cannot change the newer parent or children. |
| A6 | `pass` | Equal listener and service replays restore missing child rows without changing source authority. |
| A7 | `pass` | The machine route succeeds after one manager call while the failed snapshot write retries to convergence. |
| A8 | `pass` | The deletion projection fails once, retries to empty machine/connectivity rows, removes its retained tombstone, and never repeats manager deletion. |
| A9 | `pass` | A dropped system Engine makes projection unavailable. Services still records the same observation and definition with unchanged provider-call counts. |
| A10 | `pass` | The workload test authenticates the complete tenant, source, attachment, endpoint, listener, lease, protocol, guest-port, and actual-address tuple. |
| A11 | `pass` | The coalescing test proves bounded backoff and one retained identity. `Notify` wakes a sleeping driver for new work or explicit rebuild, and the driver-drop boundary proves cancellation after Engine shutdown. |
| A12 | `pass` | Focused and full affected behavior, strict affected quality, verifier, format, diff, docs, and proof gates pass. |

## Command ledger

All ordinary local test commands used
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`. The server commands
also used `--test-threads=1`.

| Gate | Result |
| --- | --- |
| Focused behavior | `35/35`: system connectivity `12/12`; rebuild stale-completion state machine `1/1`; system machine deletion `1/1`; compute workload projection `16/16`; server listener failure `1/1`; server machine lifecycle `4/4`. |
| Full `nimbus-system` | Final correction candidate: `84/84`. The earlier full candidate passed `82/82`, then the two review regressions increased the suite. External-provider execution remains explicitly disabled and is not claimed. |
| Full `nimbus-compute` | `473` passed, `1` subprocess helper ignored, `0` failed. |
| Full `nimbus-server` | `753` passed, `35` declared ignores, `0` failed across unit and integration targets. |
| Strict Clippy | Pass for the three affected crates, all targets, and all features with warnings denied. `CARGO_TARGET_DIR=target/ptrcomp` isolates the repository's pointer-compression V8 variant. The gate found and corrected one boxed driver-step value and one clone of a `Copy` condition array. The final rebuild-revision correction reran the complete `nimbus-system` all-target/all-feature gate. |
| Warning-denied Rustdoc | Pass for the three affected crates with all features and the isolated pointer-compression target. The final correction reran `nimbus-system`. |
| Format and diff | `cargo fmt --all --check` and `git diff --check` pass. |
| Static architecture | `36/36`. The source-derived bind census was updated from line 519 to line 517 after the server edit. `nimbus-network -> nimbus-core` remains the exact workspace edge. |
| Documentation | `check-docs` passes `108` pages. The docs-site gate passes `17/17` conditions. |
| Structured review | One full and one narrow GPT-5.6 Sol, xhigh, fast review. Both used one bundle, TruffleHog was clean, and the actual reviewer identity matched the owner requirement. All five P2 findings are corrected. No further review is authorized or needed. |

The first strict all-feature Clippy attempt used the shared target and stopped
at the repository's pointer-compression guard. It did not report a Nimbus
lint. The isolated target is the successful result above. We did not clean the
shared target.

One attempted exact invocation used only the bare rebuild test name and selected
zero tests. It is not counted. The subsequent `connectivity` filter selected
and passed the new state-machine regression in the `14/14` filtered set.

## Review dispositions

| Review | Finding | Disposition and proof |
| --- | --- | --- |
| Full, thread `019ffc96-e1c2-7172-a01f-3021d3db373e` | P2: a stale service listener fence could regress after every child row was lost. | Accepted. The durable parent stores listener generation and lease epoch. The writer rejects a lower fence from the parent before child repair or deletion. The stale-after-child-loss test passes. |
| Full | P2: retained entries had no production rebuild trigger. | Accepted. Clean retained entries receive a 30-second refresh deadline. A 50 ms test interval proves automatic repair. `Notify` wakes explicit rebuild and newly submitted work. |
| Full | P2: a 250 ms quiet window did not prove Engine cancellation against a 2-second retry bound. | Accepted. The test now observes the semantic driver-running guard and waits for its drop after Engine shutdown. |
| Full | P2: the machine fault could be consumed by an event write instead of the snapshot write. | Accepted. The injector now rejects only `StorageCommitBeforeVisibility` for the `machines` table and asserts exactly one snapshot failure before convergence. |
| Narrow, thread `019ffcad-645d-7550-bc29-b807f60b3b8c` | P2: an older in-flight success could clear a concurrent explicit rebuild request. | Accepted. Rebuild advances every retained entry revision. `rebuild_revision_rejects_an_older_in_flight_success` proves the stale acknowledgement cannot clear or remove the pending request. |

The full review reported the item incorrect at confidence `0.95`. The narrow
review confirmed the first four corrections and reported the rebuild race at
confidence `0.92`. The final correction passes focused tests, full-system
tests, strict quality checks, and static checks. The item has no remaining
review allowance.

## Candidate identity

- Full-review input: staged tree
  `2f6ab18bd50e3cad956724c79949cc9cb92d55cd`. Binary patch SHA-256:
  `4f996b818a4841ca6d2abf288019954009c4c0119b6c5d7071638473c374c02e`.
- Narrow-review input: staged tree
  `2062092f4b53b7a1f55ebe05f8b5c4dea06f2e0a`. Binary patch SHA-256:
  `ee11eedc8bbf41e5d25d4e6ffb2e4a9d4ff609494438fda7d0bb32df32df1826`.
- Final executable and static-proof candidate before ledger closeout: staged
  tree `6a9c0a57abe0c8e3810edfea4bcf879f1bea525e`. Binary patch SHA-256:
  `23a6c439158e6436d302fbfbaaac23fd7f78adddf99d920cb743fef424f94e72`.
  `19` paths, including `16` Rust paths.
