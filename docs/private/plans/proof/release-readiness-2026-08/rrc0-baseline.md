# RRC0 Release Baseline

Evidence anchor: `RRC0_BASELINE_COMPLETE`

Recorded on 2026-08-27 before release testing or repair work.

## Candidate repositories

- Nimbus candidate: `1403bc780da2fe4e39ccdecb6615f0bbaf4fbc14`
- Nimbus candidate branch: `codex/release-readiness-2026-08`
- Nimbus remote baseline: `origin/main` at
  `b57a2d680891de852d5576e65ccaea787b005431`
- Desktop candidate: `fc7b2ec8dc1f30928c061e8cd41e18b6742988ed`
- Desktop remote baseline: `origin/main` at
  `fc7b2ec8dc1f30928c061e8cd41e18b6742988ed`

The Nimbus candidate contains the accepted storage-repair campaign through
`1403bc780`. The desktop candidate equals its remote `main` baseline.

## macOS host

- Operating system: macOS 15.7.2, build 24G325
- Architecture: arm64
- Rust: rustc 1.96.1 and cargo 1.96.1
- Node.js: 26.7.0
- npm: 11.19.0
- Python: 3.14.7
- Docker: 29.7.2
- Podman: 6.1.0
- Installed Nimbus comparison binary: 0.1.45

Node.js 26 is outside the application matrix's supported Node.js 22 through 24
range. Application evidence must use a supported local runtime.

## Linux host

The local network host resolves as `minicloud.local` at `192.168.4.29` and is
reachable as `nimbus@minicloud.local`.

- Operating system: Debian 13
- Kernel: Linux 6.12.94+deb13-amd64
- Architecture: x86_64
- Rust: 1.97.1
- Node.js: 20.19.2
- Podman: 5.4.2
- Installed Nimbus comparison binary: 0.1.31

The Linux Node.js runtime is also outside the application matrix's supported
range. Candidate installation and Linux tests must not use the installed
Nimbus 0.1.31 binary as candidate evidence.

## Published and hosted state

The latest published Nimbus release is stable `v0.1.45`, published on
2026-07-06. It is neither a draft nor a prerelease.

The latest hosted runs for remote baseline `b57a2d680` are red:

- CI run 33015217206 failed. The direct failure is
  `libsql_replica_post_visibility_ack_loss_forces_crash_and_replay` in the
  external libSQL provider job. The run reports 58 passed and 1 failed in that
  job.
- Coverage run 33015217388 failed on the same test in the engine shard. The run
  reports 1,976 passed and 1 failed in that shard.

The failure says that a lost libSQL acknowledgement returned a document ID
instead of a terminal ambiguous result. The candidate includes later storage
repairs, so this campaign must reproduce the case against the candidate before
it assigns the final defect verdict.

## Initial release verifier

Command:

```text
python3 docs/private/plans/proof/release-readiness-2026-08/verify.py
```

Initial result:

```text
Summary: 0 passed, 46 unverified, 0 failed, 0 blocked, 0 structural errors
exit status: 1
```

This is the required fail-before state. No release condition was green through
a skip or missing proof.
