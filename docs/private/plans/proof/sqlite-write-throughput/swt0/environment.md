# SWT0.2 `B_ref` Environment And Artifact Identity

Captured: 2026-07-28

| Item | Value |
| --- | --- |
| `B_ref` source commit | `2a1853dab85b5677d7b4443c44c7809bd0edf550` (clean `origin/main`, post SWT0.1 merge) |
| Branch / worktree | `codex/sqlite-write-throughput-p0-baseline` / `nimbus-sqlite-write-throughput-p0` |
| Worktree status at build and every run | clean, 0 dirty files |
| CPU / memory | Apple M2 Max, 34,359,738,368 bytes (32 GiB), arm64 |
| OS | macOS 15.7.2 (24G325) |
| Rust / Cargo | 1.96.1 (2026-06-26) |
| Build | `cargo bench -p nimbus-engine --bench sqlite-write-overhead --bench concurrent-write-throughput --no-run`, worktree-local target |
| Layered binary SHA-256 | `1e7320c0b5d0832e04ab49e54afeee84447437431897bf5801eccefaa69cf501` |
| Engine binary SHA-256 | `9e37894214d355f200dd937da9b9d47f568b69a6e6379cfeeacdb0d3cb6d6f72` |
| Durability | WAL, `synchronous=FULL`; layered fixture bytes/frames identical to CTRL0 (1,396,712 / 339 production lane) |

`B_ref` is a source commit plus protocol, not a permanent numeric denominator:
every candidate gate rebuilds and reruns it in the same session (D8/D12/D13).
The reference results below are the SWT0.2 diagnostic reference run.

## Complete attempt ledger

Acceptance is per complete protocol run; no lane or round was merged across
attempts, no outlier was discarded. Rejected runs are retained byte-for-byte
under `rejected/`.

| Protocol | Attempt | Report SHA-256 | Verdict |
| --- | --- | --- | --- |
| layered | 1 | `d8139dce…630fcc4` | rejected: raw 23.4%, resident 26.4%, guarded 28.4%, lower bound 15.8% CV (pre-pause foreign load) |
| layered | 2 | `2098328b…797bff` | rejected: raw 10.8%, lower bound 13.5%, storage 12.3% CV (Spotlight churn) |
| layered | 3 | `d8224b2f…5b5610f` | rejected: lower bound 11.7% CV; other four lanes 1.2–3.9% |
| layered | 4 | `065ffc11…20ee890` | **accepted**: all lanes 1.1–5.4% CV |
| crud | 1 | `55925232…bac2b6` | rejected: N=1 24.8%, N=256 24.6% CV |
| crud | 2 | `e3bf3f98…182085` | **accepted**: 1.7 / 1.5 / 4.3% CV |
| hotkey | 1 | `66d59ea6…25c1233` | rejected: N=1 14.9%, N=256 14.0% CV |
| hotkey | 2 | `712fa51c…033384` | rejected: N=256 10.4% CV |
| hotkey | 3 | `7544a346…402fb7` | **accepted**: 3.3 / 1.3 / 1.1% CV |
| WAL observation (diagnostic) | 1 | `1e1e0387…3d34da0` | diagnostic only; never acceptance evidence |

Attempt-count note: the orchestrating session initially carried a
three-attempt cap inherited from the independent audit brief. The plan's own
acceptance policy for this task is "CV must be at most 10%; otherwise quiet
the host and rerun" with every rejection retained, and layered attempt 4 was
taken under that policy in the same verified-quiet window that produced the
accepted hot-key run. All four layered attempts are recorded above.

## Noise policy observations

Foreign workloads (node test suites, Go tests, a desktop Codex process,
Spotlight indexing of the fresh build tree) rejected attempts 1–2; the owner
paused other work, after which 1-minute load fell to ~3.2 and attempts
succeeded. Load was checked before and after every accepted run.
