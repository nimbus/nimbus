//! Maps zbus / D-Bus errors to `nimbus_core::Error`.
//!
//! D-Bus method-error *replies* arrive as `zbus::Error::MethodError(name, ..)`
//! keyed by the error-name string (e.g. `org.freedesktop.systemd1.NoSuchUnit`);
//! only zbus-*internal* failures use `zbus::Error::FDO`. Both shapes are mapped
//! here, and every D-Bus call in the client funnels through [`map_zbus`].

use nimbus_core::Error;

/// Map any `zbus::Error` to the closest `nimbus_core::Error`.
pub(super) fn map_zbus(err: zbus::Error) -> Error {
    let message = err.to_string();
    match &err {
        // Transport / connection layer — the bus or peer is unreachable.
        zbus::Error::InputOutput(_)
        | zbus::Error::Address(_)
        | zbus::Error::Handshake(_)
        | zbus::Error::InvalidReply
        | zbus::Error::MissingField => Error::Transport(message),
        // zbus-internal fdo errors.
        zbus::Error::FDO(fdo) => map_fdo(fdo, message),
        // Named D-Bus error replies (standard + systemd-specific) → by name.
        zbus::Error::MethodError(name, _, _) => map_error_name(name.as_str(), message),
        zbus::Error::InterfaceNotFound => Error::NotFound(message),
        _ => Error::Internal(message),
    }
}

fn map_fdo(fdo: &zbus::fdo::Error, message: String) -> Error {
    use zbus::fdo::Error as Fdo;
    match fdo {
        Fdo::Disconnected(_)
        | Fdo::NoServer(_)
        | Fdo::NoNetwork(_)
        | Fdo::NoReply(_)
        | Fdo::Timeout(_)
        | Fdo::TimedOut(_) => Error::Transport(message),
        Fdo::AccessDenied(_) | Fdo::AuthFailed(_) | Fdo::InteractiveAuthorizationRequired(_) => {
            Error::PermissionDenied(message)
        }
        Fdo::UnknownObject(_)
        | Fdo::UnknownInterface(_)
        | Fdo::UnknownMethod(_)
        | Fdo::UnknownProperty(_)
        | Fdo::ServiceUnknown(_)
        | Fdo::NameHasNoOwner(_)
        | Fdo::FileNotFound(_) => Error::NotFound(message),
        Fdo::InvalidArgs(_) | Fdo::InvalidSignature(_) | Fdo::NotSupported(_) | Fdo::Failed(_) => {
            Error::InvalidInput(message)
        }
        Fdo::NoMemory(_) | Fdo::LimitsExceeded(_) => Error::ResourceExhausted(message),
        _ => Error::Internal(message),
    }
}

/// Classify a D-Bus error name (the suffix after the last `.`), covering both
/// `org.freedesktop.DBus.Error.*` and `org.freedesktop.systemd1.*` names.
fn map_error_name(name: &str, message: String) -> Error {
    if ends_with_member(name, "AccessDenied")
        || ends_with_member(name, "AuthFailed")
        || ends_with_member(name, "InteractiveAuthorizationRequired")
    {
        Error::PermissionDenied(message)
    } else if ends_with_member(name, "NoSuchUnit")
        || ends_with_member(name, "NoSuchUnitProcess")
        || ends_with_member(name, "NoSuchProcess")
        || ends_with_member(name, "UnknownObject")
        || ends_with_member(name, "UnknownInterface")
        || ends_with_member(name, "UnknownMethod")
        || ends_with_member(name, "ServiceUnknown")
        || ends_with_member(name, "FileNotFound")
    {
        Error::NotFound(message)
    } else if ends_with_member(name, "Disconnected")
        || ends_with_member(name, "NoServer")
        || ends_with_member(name, "NoNetwork")
        || ends_with_member(name, "NoReply")
        || ends_with_member(name, "Timeout")
        || ends_with_member(name, "TimedOut")
    {
        Error::Transport(message)
    } else if ends_with_member(name, "InvalidArgs")
        || ends_with_member(name, "InvalidSignature")
        || ends_with_member(name, "NotSupported")
        || ends_with_member(name, "Failed")
    {
        Error::InvalidInput(message)
    } else if ends_with_member(name, "NoMemory") || ends_with_member(name, "LimitsExceeded") {
        Error::ResourceExhausted(message)
    } else {
        Error::Internal(message)
    }
}

fn ends_with_member(name: &str, member: &str) -> bool {
    name == member || name.ends_with(&format!(".{member}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(error: &Error) -> &'static str {
        match error {
            Error::Transport(_) => "Transport",
            Error::PermissionDenied(_) => "PermissionDenied",
            Error::NotFound(_) => "NotFound",
            Error::InvalidInput(_) => "InvalidInput",
            Error::ResourceExhausted(_) => "ResourceExhausted",
            Error::Internal(_) => "Internal",
            _ => "other",
        }
    }

    #[test]
    fn systemd_and_standard_error_names_map_to_expected_variants() {
        let cases = [
            ("org.freedesktop.systemd1.NoSuchUnit", "NotFound"),
            ("org.freedesktop.DBus.Error.UnknownObject", "NotFound"),
            ("org.freedesktop.DBus.Error.UnknownMethod", "NotFound"),
            (
                "org.freedesktop.DBus.Error.AccessDenied",
                "PermissionDenied",
            ),
            ("org.freedesktop.DBus.Error.AuthFailed", "PermissionDenied"),
            ("org.freedesktop.DBus.Error.InvalidArgs", "InvalidInput"),
            (
                "org.freedesktop.systemd1.TransactionIsDestructive",
                "Internal",
            ),
            ("org.freedesktop.DBus.Error.Failed", "InvalidInput"),
            ("org.freedesktop.DBus.Error.Disconnected", "Transport"),
            ("org.freedesktop.DBus.Error.NoReply", "Transport"),
            (
                "org.freedesktop.DBus.Error.LimitsExceeded",
                "ResourceExhausted",
            ),
            ("org.freedesktop.DBus.Error.NoMemory", "ResourceExhausted"),
        ];
        for (name, expected) in cases {
            let mapped = map_error_name(name, name.to_string());
            assert_eq!(variant(&mapped), expected, "name {name}");
        }
    }

    #[test]
    fn transport_and_internal_zbus_errors_map_directly() {
        let io = zbus::Error::InputOutput(std::sync::Arc::new(std::io::Error::other("x")));
        assert_eq!(variant(&map_zbus(io)), "Transport");
        assert_eq!(
            variant(&map_zbus(zbus::Error::InterfaceNotFound)),
            "NotFound"
        );
        assert_eq!(variant(&map_zbus(zbus::Error::InvalidReply)), "Transport");
    }

    #[test]
    fn internal_fdo_errors_map_through_map_fdo() {
        let denied = zbus::fdo::Error::AccessDenied("no".into());
        assert_eq!(variant(&map_fdo(&denied, "msg".into())), "PermissionDenied");
        let limits = zbus::fdo::Error::LimitsExceeded("too many".into());
        assert_eq!(
            variant(&map_fdo(&limits, "msg".into())),
            "ResourceExhausted"
        );
    }
}
