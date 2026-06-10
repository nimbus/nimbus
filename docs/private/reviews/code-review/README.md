# Code Review

Dated, agent-assisted reviews of the Nimbus codebase. Each review is a self-contained, codex-actionable document: every finding has a stable ID, exact `file:line` anchors, the reasoning behind it, and a concrete fix direction.

## Reviews

- [2026-06-09 — Full-codebase review](2026-06-09-full-codebase-review.md) — all 27 crates (~391k LoC). Architecture validated clean/acyclic; **193 findings** (2 critical, 12 high, 35 medium, 116 low, 28 info).

## Executing the review
- [Codex execution prompt](codex-execution-prompt.md) — a ready-to-paste prompt that tells a codex agent to **re-confirm every finding** (all 193, including low/info/nice-to-haves) and then **properly fix each confirmed one** to the repo's enterprise bar, with a per-change verification gate and a running remediation ledger.

## How to consume this (codex mode)
1. **Read Part I** of the dated report for the architecture verdict, themed analysis, and the **recommended remediation order** (security → correctness → modularity → quality).
2. **Work the ledger in Part II by severity.** Each finding is independently executable. Start with Critical and High — those were double-verified by two independent agents.
3. **Before editing any cited location, re-open the file.** Critical/high anchors are reliable; low/info anchors come from a structural pass and line numbers may have drifted.
4. **One finding ≈ one focused change.** IDs are stable — reference them in commits/PRs (e.g. `fix(mongodb): enforce auth gate [E1-1]`).
5. Per repo convention this is a docs artifact; code fixes land as their own changes.

## Top-priority findings (double-verified)

| ID | Sev | Subsystem | Title | Location |
|---|---|---|---|---|
| `E1-1` | critical | Adapters | dispatch() never checks conn.authenticated; SCRAM auth is purely advisory | `crates/nimbus-mongodb/src/commands/mod.rs:29-66` |
| `E1-2` | critical | Adapters | Every command authorizes to the engine as PrincipalContext::system(), bypassing engine authz | `crates/nimbus-mongodb/src/commands/crud/filter.rs:149` |
| `E1-3` | high | Adapters | matches_simple_filters silently ignores Gt/Gte/Lt/Lte, returning wrong results on the _id fast path | `crates/nimbus-mongodb/src/commands/crud/filter.rs:128-132` |
| `E1-5` | high | Adapters | Default listener ships hard-coded admin/admin credentials | `crates/nimbus-mongodb/src/lib.rs:58-62` |
| `A2-1` | high | Storage | Open-ended single-field range scan returns documents of other JSON types | `crates/nimbus-storage/src/index/scan/range.rs:60-73` |
| `A2-2` | high | Storage | No mixed-type or negative-number range-scan tests; existing range test passes only by accident | `crates/nimbus-storage/src/index/tests.rs:451-489` |
| `B1-1` | high | Engine | Direct mutation path bumps applied_head before invalidating the document cache (stale read-after-write) | `crates/nimbus-engine/src/engine/mutations/direct/store.rs:21-24` |
| `B1-2` | high | Engine | Execution-unit OCC conflict check runs outside the sequence lock, leaving a serialization gap for predicate/range/insert dependencies | `crates/nimbus-engine/src/engine/execution_units/commit.rs:42-55` |
| `B2-1` | high | Engine | Pagination cursor signature is plan-dependent, causing spurious rejection when the query plan flips | `crates/nimbus-engine/src/evaluator/cursor.rs:87-94` |
| `B3-1` | high | Engine | Lost wakeup in begin_delete_blocking: condvar notified without holding the guarding mutex | `crates/nimbus-engine/src/tenant/lifecycle.rs:43-62` |
| `B4-1` | high | Engine | Trigger invocations in Running state are never re-enqueued after a crash | `crates/nimbus-engine/src/engine/mutations/commit_processing.rs:90-98` |
| `C3-1` | high | Runtime | Bun/JSC linked FFI path drops the watchdog and concurrency permit — no timeout enforcement | `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs:112-158` |
| `D4-1` | high | Server | MongoDB data plane has no authentication enforcement — SCRAM handshake is decorative | `crates/nimbus-mongodb/src/commands/mod.rs:43` |
| `I1-1` | high | CLI | HTTP-sourced machine image is persisted as the bootable disk with no integrity verification | `crates/nimbus-bin/src/machine/manager/image.rs:123-273` |

## Subsystem scorecard

| Subsystem | Health | Crit | High | Med | Low | Info |
|---|---|---:|---:|---:|---:|---:|
| Adapters | RED | 2 | 2 | 7 | 16 | 3 |
| Storage | RED | 0 | 2 | 3 | 18 | 5 |
| Engine | RED | 0 | 5 | 8 | 13 | 0 |
| Runtime | RED | 0 | 1 | 5 | 11 | 5 |
| Server | RED | 0 | 1 | 3 | 14 | 6 |
| Security | GREEN | 0 | 0 | 0 | 12 | 3 |
| Trust | GREEN | 0 | 0 | 0 | 4 | 2 |
| Sandbox | YELLOW | 0 | 0 | 4 | 9 | 1 |
| CLI | RED | 0 | 1 | 5 | 11 | 2 |
| Misc | GREEN | 0 | 0 | 0 | 8 | 1 |

Health: **RED** = has critical/high · **YELLOW** = has medium · **GREEN** = low/info only.

## Method
Two-phase multi-agent review. Phase 1 mapped every crate and validated 8 architecture invariants (7 hold; the one failure is `ARCHITECTURE.md` doc-drift). Phase 2 ran a deep finder per subsystem across ten review dimensions, an adversarial verifier that re-read and tried to refute every medium+ claim, and an independent second-skeptic pass on every critical/high. False-positives were removed; 0 of 14 double-verified findings were disputed. See the dated report's appendix for full methodology.
