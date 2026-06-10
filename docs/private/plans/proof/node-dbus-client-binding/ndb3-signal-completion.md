# NDB3 Proof — signal-correlated completion + property encoding

Implements the trust-critical flow: a real client speaks the systemd Manager1
protocol with `Manager.Subscribe` + `JobRemoved` correlation (no polling),
plus a centralized `OwnedValue` property encoder, and a `GetUnit`-based
`inspect_unit`.

## Signal-correlated job completion (`signals.rs`)

The naive flow (call `StartTransientUnit`, return the job path as success)
masks async unit failures — the Manager returns the job path long before the
unit actually starts. The correct flow, with the race closed by establishing
the stream *before* the method call:

```
1. manager.subscribe().await                 // enable signal emission
2. let mut s = manager.receive_job_removed()  // stream live BEFORE the call
3. let job = manager.start_transient_unit(..) // (or stop_unit) → job path
4. while let Some(sig) = s.next().await {
       let a = sig.args()?;
       if a.job() == &job { return classify(a.result()); }
   }
```

`classify_result`: `done` → success; `skipped` → already in target state;
`failed`/`canceled`/`timeout`/`dependency` **and any unrecognized result**
(`once`/`merged`/`assert`/`unsupported`/`collected`, …) → `Failed` — an
unknown result is never silently treated as success.

The subscribe → stream → method-call ordering lives in one file
(`signals.rs`) so the race-closing order is auditable (and the verifier's
condition 6 lexical-order check passes).

## Property encoding (`properties.rs`)

`encode_start_properties` maps each `SystemdDbusProperty` to a
`(String, OwnedValue)` with the systemd-correct name and zvariant signature:

| typed property | D-Bus name | signature | note |
|---|---|---|---|
| `Description`/`Slice` | same | `s` | |
| `Restart` | `Restart` | `s` | `no`/`on-failure`/`always` |
| `RestartSec(secs)` | `RestartUSec` | `t` | seconds → **microseconds** |
| `MemoryMax`/`CpuWeight`/`TasksMax` | `MemoryMax`/`CPUWeight`/`TasksMax` | `t` | |
| `ExecStart` | `ExecStart` | `a(sasb)` | argv[0] = exec path prepended |

`ExecStart` is encoded via a `#[derive(Type, Value, OwnedValue)]` `ExecCommand`
struct so it gets a *static* `(sasb)` signature — this is what lets
`Vec<ExecCommand>` become an array `Value` cleanly.

## inspect_unit (`mod.rs`)

`GetUnit(name)` → object path → `UnitProxy` (`active_state`, `sub_state`) +
`ServiceProxy` (`main_pid`) at that path. A `NoSuchUnit` reply (unit never
started, or GC'd after stop) is reported as `inactive`/`dead`, not an error.
`main_pid == 0` is mapped to "no main pid" (`None`).

## Error mapping

NDB3 routes every D-Bus call through a single temporary `map_zbus`. NDB4
replaces it with the full taxonomy in `error.rs` (adding the `Transport` and
`NotFound` core variants); because every call site already funnels through
`map_zbus`, that swap is localized.

## Verification

- `cargo build -p nimbus-node --features systemd-dbus,systemd-dbus-test-bus`
  — clean (signals/properties/inspect compile against zbus 5.15 +
  zbus_systemd 0.26000.0).
- `cargo test -p nimbus-node --features systemd-dbus,systemd-dbus-test-bus`
  — `6 passed; 0 failed` (4 classifier + 2 property-encoder tests:
  `scalar_properties_encode_to_expected_names_and_signatures`,
  `exec_start_encodes_as_array_of_sasb_with_argv0_prepended`).
- `cargo clippy … --all-targets` — clean (workspace `clippy::all = deny`).

The live signal round-trip (real `JobRemoved` ordering, `ExecStart`-not-found
→ `failed`) is exercised by NDB5 against `systemctl --user`.

## Verifier

Condition 6 (signal-correlated completion + `OwnedValue` encoder) flips to
PASS. Verifier state after NDB3: `6 passed, 4 failed`.
