# NNC8.5 Bounded Retries, Backoff, And Cancellation

## Scope

NNC8.5 proves that a permanent network-control-plane failure cannot make a
request, retained worker, or authority loop spin without a bound or useful
diagnostic. It does not add a new retry framework, provider capability,
network effect, or policy owner.

Dependency: NNC8.4 is complete at
`ee0837369700fe7b677aebccc901fe484678c53f`.

## Frozen acceptance

| ID | Required evidence | Status |
| --- | --- | --- |
| K1 | The source census classifies every production loop that retries network-control-plane work. | `pass` |
| K2 | Provision, restart, and teardown drivers terminate after at most 64 decisions and return a typed progress-limit error. | `pass` |
| K3 | Startup recovery terminates after at most 64 durable pages. | `pass` |
| K4 | The restart watch limits each sweep to 64 pages and waits between sweeps. | `pass` |
| K5 | A permanent restart-store failure uses capped exponential backoff. | `pass` |
| K6 | Read-only wake hints cannot bypass restart-store failure backoff. | `pass` |
| K7 | Restart-watch cancellation interrupts the backoff wait and does not erase durable work. | `pass` |
| K8 | A successful restart-store sweep resets the failure backoff. | `pass` |
| K9 | Each restart-store failure emits a structured diagnostic with the error, consecutive failure count, and retry delay. | `pass` |
| K10 | System table and connectivity projection failures retain work, back off, report diagnostics, recover, and stop with their runtime/Engine owner. | `pass` |
| K11 | Network authority lock contention terminates with a typed timeout and performs no unlocked mutation. | `pass` |
| K12 | Segment placement and IPAM searches are finite under their durable limits; forwarding I/O uses explicit deadlines. | `pass` |
| K13 | Focused behavior, full affected gates, dependency/effect checks, docs gates, and one candidate-frozen Sol/xhigh/fast review pass. | `pass` |

## Bounded source census

| Owner | Retry or wait contract | Bound, backoff, cancellation, diagnostic | Disposition |
| --- | --- | --- | --- |
| Compute provision driver | Advances one durable provision record. | `MAX_DECISIONS_PER_RUN = 64`; typed `ProgressLimit`; provider results are confirmed before return. | Existing proof. |
| Compute restart driver | Advances one durable restart epoch. | `MAX_RESTART_DECISIONS_PER_RUN = 64`; typed `ProgressLimit`; definite failure returns durable truth. | Existing code; add focused progress-limit coverage only if the candidate matrix lacks it. |
| Compute teardown driver | Advances one durable teardown record. | `MAX_TEARDOWN_DECISIONS_PER_RUN = 64`; typed `ProgressLimit`; cleanup-pending returns retained truth. | Existing proof. |
| Compute startup recovery | Enumerates recoverable saga pages once. | `MAX_STARTUP_RECOVERY_PAGES = 64`; typed `PageLimit`; startup fails closed. | Existing proof. |
| Compute retained teardown/supervisor waits | Joins one exact retained key. | Watch channels wake on completion/cancellation; failures are retained and returned. No retry effects occur in the wait loop. | Wait-only, not a retry authority. |
| Compute durable restart watch | Scans at most 64 pages, then waits on an injected clock. | Normal deadlines and cancellation are tested. Store errors currently fall back to the periodic deadline, but `wake.notified()` can bypass that delay and the error is swallowed. | **Expected-red product gap.** |
| System table projection | Retains dirty scopes after failure. | Exponential backoff capped at five seconds; counters and structured error; owner/runtime replacement cancels stale work. | Existing permanent-failure proof. |
| System connectivity projection | Retains the latest typed observation. | Exponential backoff capped at two seconds; structured warning; weak Engine ownership stops the driver. | Existing permanent-failure/cancellation proof. |
| Network authority store | Retries process and file locks. | Configured timeout and retry interval; typed path/timeout diagnostic; no unlocked read or mutation. | Existing cross-process proof. |
| OCI segment placement | Rescans after a stale complete-set observation. | Each stale result requires another monotonic block append; the durable tenant maximum is 64 blocks, then growth fails closed. | Statically finite; no sleep-based retry owner. |
| OCI IPAM | Searches one finite address ring. | Stops on a free address or after returning to the starting address with typed subnet exhaustion. | Statically finite. |
| Machine forwarding | Reads/connects over a provider-owned stream. | Response-size limit plus explicit connect/read deadline and typed timeout. | Bounded I/O, not a retry loop. |
| Listener, proxy, and signal loops | Serve or observe until their owner shuts down. | These loops do not retry failed control-plane mutations. Listener groups abort and join owned tasks; proxy workers and signal streams follow their provider owner. | Excluded from retry authority census. |

Unrelated document OCC retries, protocol accept loops, test polling loops, and
provider-internal command execution are outside this item. Their inclusion
would not prove the network lifecycle acceptance criterion.

## Expected-red behavior

Inject a `WorkloadSagaStoreError::Unavailable` restart-candidate page forever,
start the real `DurableRestartWatch`, and send many read-only hints while its
clock remains before the retry deadline. Before the correction, hints wake the
watch and increase durable page calls without time advancing. After the
correction:

1. page calls remain unchanged before the injected clock reaches the deadline.
2. consecutive failures wait at base, two-times base, then four-times base.
3. cancellation returns `RestartWait::Cancelled` during that wait.
4. one successful sweep resets the next failure delay to the base.
5. each failure logs the store error, failure count, and retry delay.

## Ownership constraints

- NNC8.5 limits product edits to the compute-owned restart watch and its private
  tests.
- `nimbus-network -> nimbus-core` remains the only initial workspace edge.
- No socket, provider effect, policy, naming, projection authority, cluster
  transport, or new public API enters `nimbus-network`.
- Wake hints remain advisory and cannot weaken durable-store backpressure.

## Verification ledger

| Evidence | Result |
| --- | --- |
| Frozen census and expected-red case | Complete. The first focused run passed `14`, failed the hint-bypass case, and filtered `465`; 32 hints caused 33 page calls at clock time zero. |
| Corrected focused tests | Store-focused matrix `16/16`; restart watch `11/11`; complete saga module `283/283`. The watch proves base/two-times/four-times backoff, a 64-times cap, hint rejection, cancellation, recovery reset, zero saga load/CAS, and zero supervisor calls during store failure. |
| Existing retry/cancellation matrix | System permanent-failure, connectivity permanent-failure, projection lease retry, diagnostic reset, and cancellation retention pass `5/5`; network lock timeout passes `1/1`; OCI placement passes `8/8`. |
| Full affected gates | Compute passes `481` with one intentional ignore. All-target/all-feature check, strict Clippy, warning-denied Rustdoc, Rustfmt, and diff checks pass. |
| Architecture gates | Live verifier passes `38/38`; NNCV004, NNCV008, NNCV012, NNCV024, NNCV034, and every other current condition are green. |
| Documentation gates | Strict proof lint passes one file with zero diagnostics. Prettier passes. Docs pass `108` pages, and the site passes `17/17` conditions. |
| Structured item review | The full Sol/xhigh/fast review accepted one P3 test-synchronization defect. The correction replaces scheduler yields with a registered, bounded store-call observation. Restart watch `11/11`, full compute `481 + 1 ignored`, strict Clippy, Rustfmt, and diff checks pass after correction. The one narrow review is clean at `0.98`; cadence is exhausted. |

The only product change affects the compute restart watch. A store failure now
increments a local saturating counter and calculates capped exponential delay.
The watch uses the configured rescan period and waits only on the injected clock
and cancellation token. Advisory hints regain their normal wake behavior after
a successful sweep resets the counter. Each failed sweep logs the store error,
consecutive failure count, and retry delay without a stable identifier in a
metric label.

## Item review

The one full item review used GPT-5.6 Sol with xhigh reasoning and fast service
tier. It reviewed staged tree `780a4fceac51d6befb7d74cc404bc9e631be15c7`.
The staged binary patch SHA-256 was
`76f806ac69b6e2c9fa57dbbf611e6686fee56bf68698de0a7d780e27da45a379`.
TruffleHog was clean.

| Finding | Disposition |
| --- | --- |
| P3: repeated `yield_now()` calls did not prove that the spawned watch had an opportunity to consume each advisory hint. | Accepted. `WatchStore` now exposes a race-safe page-call notification. The test registers that observation before checking and uses a bounded timeout with the observed call count in its failure diagnostic. This correction changes executable test code, so it authorizes one narrow review. |

The full review accepted the production backoff, cap, cancellation, recovery
reset, diagnostic, and ownership changes. Its overall confidence was `0.91`.
The structured finding confidence was `0.93`.

The narrow correction review used staged tree
`a77b9cac37c1efa24003dfca0dc09880834452f1`. Its staged binary patch SHA-256
was `d6a2bc293e6b1f20e0f489f0bff3748a9baac821601b296a0bfc6701066f6675`.
GPT-5.6 Sol used xhigh reasoning and fast service tier. TruffleHog was clean.
The review reported zero findings and accepted the correction at confidence
`0.98`. The cadence permits no further review.
