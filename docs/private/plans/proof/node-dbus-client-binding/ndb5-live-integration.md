# NDB5 Proof — Linux-gated live integration tests

`crates/nimbus-node/tests/zbus_systemd_live.rs`, gated
`#![cfg(all(target_os = "linux", feature = "systemd-dbus-integration-tests"))]`
— it compiles to nothing off-Linux or without the explicit feature, and only
*runs* in NDB6's CI lane against a real `systemctl --user`.

## Tests

1. **`start_inspect_stop_roundtrip_against_session_systemd`** (core liveness
   proof): start `/usr/bin/sleep 30` as a transient unit → assert the returned
   job path is `/org/freedesktop/systemd1/job/…` (JobRemoved=done) → inspect →
   assert `active`/running with a main PID → stop (JobRemoved-correlated) →
   assert `inactive` → inspect again → `inactive` (the NoSuchUnit-after-GC
   path). Exercises the full chain: connection + capability probe + property
   encoding + `Manager.Subscribe`/`JobRemoved` completion + `GetUnit` +
   `UnitProxy`/`ServiceProxy` reads.
2. **`failed_unit_is_observable_via_inspect`**: start `/usr/bin/false` → poll
   inspect until the unit settles → assert it reaches `failed`. Proves the
   failure-observation path surfaces a non-`done` terminal state.

## No silent skips

`session_client()` does `ZbusSystemdClient::new(BusKind::Session).await.expect(..)`
and asserts `dbus_available() && transient_units()`. An unreachable session
bus or a systemd-user instance without transient-unit support is a **hard test
failure**, never a skip — a broken bootstrap surfaces as red, not as a vacuous
green.

## Test seam

`SystemdStopUnitRequest`/`SystemdInspectUnitRequest` already expose public
`for_workload(workload_id)`. For start, NDB5 adds
`SystemdStartTransientUnitRequest::for_integration_test(workload_id,
executable, args)` (gated `#[cfg(feature = "systemd-dbus-integration-tests")]`)
so the test drives `StartTransientUnit` without constructing a full
`HostLifecyclePlan` + tenant decision. Uses `StartTransientMode::Fail` so a
stale unit surfaces rather than being silently replaced. Unique
UUID-suffixed (`pid` + nanos) workload ids prevent unit-name collisions, so
no `ResetFailedUnit` teardown is required for correctness (best-effort `stop`
in teardown).

## Compile verification (macOS dev host)

The `target_os = "linux"` gate means the body is not compiled on macOS. To
catch compile errors before CI, the gate was temporarily flipped to
`target_os = "macos"` and built with
`cargo test -p nimbus-node --features systemd-dbus,systemd-dbus-integration-tests --test zbus_systemd_live --no-run`
(the zbus/zbus_systemd API is cross-platform; only the *runtime* needs Linux
systemd), then reverted to `target_os = "linux"`.

Result: compiles clean (`cargo test … --no-run`, `COMPILE_EXIT=0`) under the
macOS-flipped gate; gate restored to `target_os = "linux"`.

The tests are *run* (not just compiled) in NDB6 on `ubuntu-24.04`; that green
run is the real evidence and is captured in `ndb6-ci-lane.md`.

## Verifier

Condition 8 (integration test file gated by `target_os = "linux"` +
`systemd-dbus-integration-tests`) flips to PASS. Verifier after NDB5:
`8 passed, 2 failed`.
