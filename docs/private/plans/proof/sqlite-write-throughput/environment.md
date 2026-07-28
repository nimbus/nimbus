# SQLite Write-Throughput Environment

Captured: 2026-07-27

| Item | Value |
| --- | --- |
| Base commit | `e47b64eacc3d54dc5bfe7d51727306a81cfacb28` |
| Branch | `codex/sqlite-write-throughput-plan` |
| Worktree | `/Users/jack/src/github.com/nimbus/nimbus-sqlite-write-throughput-plan` |
| CPU | Apple M2 Max |
| Memory | 34,359,738,368 bytes (32 GiB) |
| Architecture | arm64 |
| OS | macOS 15.7.2, build 24G325 |
| Rust | `rustc 1.96.1 (31fca3adb 2026-06-26)` |
| Cargo | `cargo 1.96.1 (356927216 2026-06-26)` |
| rusqlite | 0.40.1 |
| libsqlite3-sys | 0.38.1 |
| SQLite runtime | 3.51.3 |
| SQLCipher runtime | 4.14.0 community |
| SQLite source id | `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1` |
| SQLite link features | `bundled-sqlcipher-vendored-openssl`, `unlock_notify`, `session`, `modern_sqlite`, `limits`, `backup`, `fallible_uint` |
| Durability | WAL, `synchronous=FULL`, foreign keys on |
| Page/checkpoint | 4 KiB page, `wal_autocheckpoint=1000` |

The benchmarks used the repository's shared release target:
`/Users/jack/src/github.com/nimbus/nimbus/target`.

The complete Engine baseline binary before adding the layered harness had
SHA-256:

`5e5cf2477a0f5fecfa23a0f96f129cd21c10d5bc690ab76c83ad5590ecef19d5`

The layered binary must be rebuilt and re-hashed from each candidate commit.
Do not reuse the planning run's binary hash as evidence for an implementation
candidate.

Final reviewed planning-worktree layered binary SHA-256:

`1ac46fb1dbf2d2d2b56eeedfe65770d5766be3dadc40f7bfd075efe406a2aa39`

The clean report used as the planning reference has SHA-256
`6b4f69edf5b040822195deb699826bdbfcdf9e0a15e59f2033a16aba45b28574`,
but its overwritten executable was not hashed. It is therefore explicitly
not an acceptance baseline. Complete reruns from the immediately preceding
review-pass binary all breached the CV gate under unrelated host load and are
committed verbatim, without merged or discarded samples, through
`layered-review-reruns.md`. The final binary also makes every per-second
column use one estimator, fixes the fixture's table id across processes, and
audits journal payloads, every version row, catalog/metadata rows, and live
state after every batch. SWT0 must capture a quiet, same-session
binary/report pair before implementation acceptance begins.

The final binary's complete 12 × 60 report is committed as
`layered-final-binary-rejected.md` with SHA-256
`77cb7fec6178c5d579462c3d9dcf4ee654188f2b5a62e5b7d89f199f866ea559`.
It is rejected because the production-storage lane had 10.3% CV.

## Capture commands

```bash
git rev-parse HEAD
git status --short
rustc --version --verbose
cargo --version
sw_vers
uname -m
sysctl -n machdep.cpu.brand_string
sysctl -n hw.memsize
shasum -a 256 <benchmark-binary>
shasum -a 256 <benchmark-report>
```

## Noise policy

- Warm caches/build artifacts before measurement.
- Do not run overlapping Cargo builds or performance suites.
- Use at least the prescribed warmup and measured rounds.
- Reject a lane when CV exceeds 10%; quiet the host and rerun.
- Base/candidate A/B must use the same commit-independent fixture, hardware,
  build profile, durability, and batch configuration.
- Record raw samples; a summary-only result is not acceptable proof.
