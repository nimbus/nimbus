# NDB6 Proof — node-dbus-integration CI lane

New `node-dbus-integration` job in `.github/workflows/ci.yml` on
`ubuntu-24.04`, wired into `rust-gate-summary.needs:` (so it gates the Rust
merge summary). `nimbus-node` has zero UI/Node prerequisites, so the lane is
self-contained (checkout + `setup-rust-cached`, no UI dist / convex codegen).

## Bootstrap (the hard part)

GitHub runners have no interactive login session, so the per-user systemd
manager is not running by default. The lane:

1. `sudo apt-get install -y dbus-user-session systemd-container`
2. `sudo loginctl enable-linger "$USER"` + `sudo systemctl start user@<uid>`
3. waits for `/run/user/<uid>/bus`, then exports `XDG_RUNTIME_DIR` +
   `DBUS_SESSION_BUS_ADDRESS` via `$GITHUB_ENV`.

A separate verify step hard-fails the job if `systemctl --user show
--property=Version` cannot reach the Manager (a `degraded`/`starting`
`is-system-running` is an accepted reachability signal, not a skip).

Then: `cargo test -p nimbus-node --features
systemd-dbus,systemd-dbus-integration-tests --test zbus_systemd_live
--no-fail-fast`, and a step that writes a per-test markdown table to
`$GITHUB_STEP_SUMMARY`.

## Verifier fix

Condition 9's gate-membership check used an awk `/start/,/end/` range whose
end pattern (`^  [a-z…]:`) also matched the `rust-gate-summary:` start line,
collapsing the range to one line so it never saw the `needs:` list. Rewrote it
to extract the block *body* with a flag. Condition 9 now PASSes (verifier:
`9 passed, 1 failed`).

## Green run (the real evidence)

CI runs on `push:main` / `pull_request` / `workflow_dispatch` — not on
feature-branch pushes. The lane is exercised on the `node-dbus-binding` branch
via `gh workflow run ci.yml --ref node-dbus-binding` (and finally on the PR),
iterating the systemd bootstrap until the lane is green.

First green `node-dbus-integration` run: `<run URL — filled after green>`
Per-test results: `<filled after green>`
