# Time and ordering

Nimbus treats time and order as separate architecture concerns. A value must
carry the weakest semantics that satisfy its consumer; no universal time
provider or globally fake clock is introduced.

## Clock domains

| Domain | Authority | Valid uses | Invalid uses |
| --- | --- | --- | --- |
| Serializable epoch facts | `WallClock` | External timestamps, absolute schedules, token and signature validation | Process-local elapsed TTL accounting |
| Process-local elapsed policy | `MonotonicClock` | Local rate windows, retry cadence, latency, in-memory retention | Persistence, logs as epoch facts, cross-process comparison |
| Distributed ownership | Provider time plus monotonically increasing epoch | Committer leases and future cluster ownership | Subtracting provider expiry from local wall time |
| Durable order | Logical sequence numbers | Commit, journal, publication, and replay order | Inferring order from timestamps |

`SystemWallClock` is the production adapter for Unix-epoch observations.
`SystemMonotonicClock` is observation-only: waiting remains owned by the
component whose state can wake or cancel the wait. Test harnesses carry manual
wall and monotonic clocks independently so wall corrections never imply elapsed
time and elapsed advancement never changes a serialized timestamp.

`Instant` values are never serialized or compared across processes. Epoch
timestamps remain data facts, not duration budgets.

## Durable deadlines

The scheduler persists the caller's exact absolute deadline. Its local worker
waits for that durable deadline with bounded wall-clock resampling and an
earlier-work notification. A forward wall correction therefore executes within
one resample bound; a backward correction cannot execute the job early; and
shutdown interrupts a far-future wait. The waiter does not busy-loop at a zero
configured interval.

Convex `runAt` adapters pass the requested absolute timestamp into the Engine.
They do not convert it to a relative delay outside the Engine, including inside
a mutation execution unit where the scheduled write must commit or roll back
with its parent.

## Local elapsed lifetimes

Tenant write-rate windows, transaction-session TTLs, and retained Firebase
listen/write streams use monotonic observations. Their public epoch metadata is
sampled separately where the protocol requires it. Expiry is checked before
tenant/principal invalidation for transaction sessions, preserving observable
error precedence.

## Committer lease renewal

The storage provider owns committer-lease expiry and epoch fencing. Engine
renewal uses process-local monotonic time only for cadence and a conservative
local safety budget:

- initial acquisition requests 30 seconds, renewal requests 60 seconds, and
  normal renewal cadence is 10 seconds;
- the local fail-closed deadline reserves a 15-second provider-expiry margin,
  yielding 15 seconds after acquisition and 45 seconds after renewal;
- successful renewals restore the normal cadence and reset the consecutive
  failure streak;
- transient provider failures use deterministic owner/tenant-keyed jittered
  retries with a 1-second base, 4-second cap, and at most 25 percent positive
  jitter, always clipped to the remaining local safety budget;
- consuming the local safety budget changes the lease to validity-unknown,
  stops renewal, and drains the committer rather than allowing another write;
- provider `Fenced` is terminal and follows the existing eviction path;
- shutdown wakes and joins the renewal worker promptly;
- diagnostics expose the provider expiry as provider time, plus monotonic age
  since the last successful acquire/renew and the current failure streak.

Platform suspension can consume the entire local safety budget. Nimbus never
interprets a recent local observation as proof that a distributed lease is
still valid; every protected provider operation remains epoch-fenced.

## Distributed activation gates

The Cloudflare Durable Object module is a compatibility-test substrate. Its
process-local lane mutex and local-wall activation record cannot exclude a
second process. The module is crate-private, and architecture verification
rejects any production construction or invocation outside the concept-owned
module. There is no served Durable Object data plane.

Horizontal-scaling HS5 owns promotion. It must provide per-object placement,
monotonically increasing activation epochs, and storage-atomic rejection of a
stale epoch on every protected write. Promotion evidence must cover expired and
stale epochs, acknowledgement loss, replay, cancellation, crash, takeover, and
two-process fencing.

The future cluster super-net allocator is also blocked. Its current
Raft-committed lease shape contains a wall expiry observed by a node's local
clock, but no safe skew model has been selected. Before cluster admission can be
enabled, HS5 must do one of the following:

1. State and enforce maximum leader/node clock skew and drift, maximum committed
   observation delay, and a reassignment grace that proves the former and new
   owners cannot overlap; or
2. Replace wall expiry with clock-free distributed authority.

Either design must atomically validate the lease epoch with protected writes.
Deterministic promotion tests must cover forward and backward node time, delayed
committed observations, partitions, restart, stale epoch, and concurrent
reassignment. Until that proof exists, `assert_cluster_admission` rejects the
future cluster allocator even though its source-level seam and unit tests exist.

## Ambient source control

`scripts/data/clock-sources.tsv` classifies every reachable production
`Timestamp::now`, `SystemTime::now`, and canonical free-helper source with an
owner, semantic rationale, and removal trigger. Tests, test support, benchmarks,
and exact `#[cfg(test)]` items are excluded structurally. Correctness-sensitive
source trees reject new ambient reads even when someone attempts to add an
allowlist entry.
