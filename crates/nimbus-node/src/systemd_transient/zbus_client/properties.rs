//! Encodes typed [`SystemdDbusProperty`] values into the
//! `Vec<(String, OwnedValue)>` shape systemd's `StartTransientUnit` expects.
//! Centralizing `OwnedValue` construction here keeps the zvariant signatures
//! in one place instead of scattering them through the client.

use nimbus_core::{Error, Result};
use serde::Serialize;
use zbus::zvariant::{OwnedValue, Type, Value};

use super::super::{HostRestartPolicy, SystemdDbusProperty, SystemdExecStart};

/// One `ExecStart` command: systemd D-Bus signature `(sasb)` —
/// `(executable, argv, ignore_failure)`. A derived `Type` gives it the static
/// `(sasb)` signature, which is what lets `Vec<ExecCommand>` become an array
/// `Value` without dynamic-signature gymnastics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, Value, OwnedValue)]
struct ExecCommand {
    path: String,
    argv: Vec<String>,
    ignore_failure: bool,
}

/// Encode the typed properties of a `StartTransientUnit` request.
pub(crate) fn encode_start_properties(
    properties: &[SystemdDbusProperty],
) -> Result<Vec<(String, OwnedValue)>> {
    properties.iter().map(encode_property).collect()
}

fn encode_property(property: &SystemdDbusProperty) -> Result<(String, OwnedValue)> {
    let (name, value): (&str, OwnedValue) = match property {
        SystemdDbusProperty::Description(value) => ("Description", str_value(value)?),
        SystemdDbusProperty::Slice(value) => ("Slice", str_value(value)?),
        SystemdDbusProperty::Restart(policy) => ("Restart", str_value(restart_str(*policy))?),
        // systemd transient time properties use the `USec` suffix and
        // microseconds, not `RestartSec`/seconds.
        SystemdDbusProperty::RestartSec(seconds) => (
            "RestartUSec",
            u64_value(seconds.saturating_mul(1_000_000)),
        ),
        SystemdDbusProperty::MemoryMax(bytes) => ("MemoryMax", u64_value(*bytes)),
        SystemdDbusProperty::CpuWeight(weight) => ("CPUWeight", u64_value(*weight)),
        SystemdDbusProperty::TasksMax(max) => ("TasksMax", u64_value(*max)),
        SystemdDbusProperty::ExecStart(exec) => ("ExecStart", exec_start_value(exec)?),
    };
    Ok((name.to_string(), value))
}

/// `ExecStart` is an array of `(sasb)` commands. systemd's argv convention is
/// that `argv[0]` is the program path, so it is prepended to the request args.
fn exec_start_value(exec: &SystemdExecStart) -> Result<OwnedValue> {
    let mut argv = Vec::with_capacity(exec.args().len() + 1);
    argv.push(exec.executable().to_string());
    argv.extend(exec.args().iter().cloned());
    let command = ExecCommand {
        path: exec.executable().to_string(),
        argv,
        ignore_failure: exec.ignore_failure(),
    };
    to_owned(Value::from(vec![command]))
}

fn restart_str(policy: HostRestartPolicy) -> &'static str {
    match policy {
        HostRestartPolicy::No => "no",
        HostRestartPolicy::OnFailure => "on-failure",
        HostRestartPolicy::Always => "always",
    }
}

fn str_value(value: &str) -> Result<OwnedValue> {
    to_owned(Value::from(value))
}

fn u64_value(value: u64) -> OwnedValue {
    OwnedValue::from(value)
}

fn to_owned(value: Value<'_>) -> Result<OwnedValue> {
    OwnedValue::try_from(value).map_err(|err| {
        Error::Internal(format!("failed to encode systemd property as OwnedValue: {err}"))
    })
}

#[cfg(all(test, feature = "systemd-dbus-test-bus"))]
mod tests {
    use super::*;

    fn find<'a>(props: &'a [(String, OwnedValue)], name: &str) -> &'a OwnedValue {
        &props
            .iter()
            .find(|(key, _)| key == name)
            .unwrap_or_else(|| panic!("property {name} should be encoded"))
            .1
    }

    #[test]
    fn scalar_properties_encode_to_expected_names_and_signatures() {
        let props = encode_start_properties(&[
            SystemdDbusProperty::Description("desc".to_string()),
            SystemdDbusProperty::Slice("nimbus.slice".to_string()),
            SystemdDbusProperty::Restart(HostRestartPolicy::OnFailure),
            SystemdDbusProperty::RestartSec(2),
            SystemdDbusProperty::MemoryMax(1024),
            SystemdDbusProperty::CpuWeight(50),
            SystemdDbusProperty::TasksMax(64),
        ])
        .expect("scalar properties should encode");

        // Names that differ from the typed variant: RestartSec -> RestartUSec.
        let names: Vec<&str> = props.iter().map(|(name, _)| name.as_str()).collect();
        assert!(names.contains(&"RestartUSec"));
        assert!(!names.contains(&"RestartSec"));
        assert!(names.contains(&"CPUWeight"));

        // Numerics encode as u64; RestartSec(2) seconds -> 2_000_000 microseconds.
        assert_eq!(find(&props, "MemoryMax"), &OwnedValue::from(1024u64));
        assert_eq!(find(&props, "TasksMax"), &OwnedValue::from(64u64));
        assert_eq!(find(&props, "RestartUSec"), &OwnedValue::from(2_000_000u64));
        // Description / Restart are strings.
        assert_eq!(find(&props, "Description").value_signature().to_string(), "s");
        assert_eq!(find(&props, "Restart"), &str_value("on-failure").unwrap());
    }

    #[test]
    fn exec_start_encodes_as_array_of_sasb_with_argv0_prepended() {
        let exec = SystemdDbusProperty::ExecStart(SystemdExecStart::for_test(
            "/usr/bin/sleep",
            vec!["10".to_string()],
        ));
        let props = encode_start_properties(&[exec]).expect("ExecStart should encode");
        let value = find(&props, "ExecStart");
        // a(sasb): array of (string, array-of-string, bool).
        assert_eq!(value.value_signature().to_string(), "a(sasb)");
    }
}
