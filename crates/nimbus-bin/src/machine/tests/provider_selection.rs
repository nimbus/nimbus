use super::*;

// These tests exercise `resolve_machine_provider_from`, the pure precedence
// core behind `resolve_machine_provider`. The static default is krunkit: with
// no explicit selection and no environment override, resolution MUST land on
// krunkit. There is no auto-detection or capability sniffing — vfkit and any
// future backend are strictly opt-in via an explicit selection or the
// `NIMBUS_MACHINE_PROVIDER` environment variable.

#[test]
fn no_selection_resolves_to_krunkit() {
    let resolved = resolve_machine_provider_from(None, None)
        .expect("absent selection should resolve to the static default");
    assert_eq!(resolved, MachineProvider::Krunkit);
}

#[test]
fn empty_or_whitespace_environment_falls_back_to_krunkit() {
    for blank in ["", "   ", "\t", "\n"] {
        let resolved = resolve_machine_provider_from(None, Some(blank))
            .expect("a blank environment override should fall back to the default");
        assert_eq!(
            resolved,
            MachineProvider::Krunkit,
            "blank override {blank:?} should resolve to krunkit"
        );
    }
}

#[test]
fn explicit_selection_outranks_environment() {
    // An explicit selection (CLI flag or persisted config) wins even when the
    // environment names a different provider.
    let resolved = resolve_machine_provider_from(Some(MachineProvider::Vfkit), Some("krunkit"))
        .expect("explicit selection should resolve");
    assert_eq!(resolved, MachineProvider::Vfkit);

    let resolved = resolve_machine_provider_from(Some(MachineProvider::Krunkit), Some("vfkit"))
        .expect("explicit selection should resolve");
    assert_eq!(resolved, MachineProvider::Krunkit);
}

#[test]
fn environment_override_selects_named_provider() {
    let resolved = resolve_machine_provider_from(None, Some("vfkit"))
        .expect("a known environment override should resolve");
    assert_eq!(resolved, MachineProvider::Vfkit);

    let resolved = resolve_machine_provider_from(None, Some("krunkit"))
        .expect("a known environment override should resolve");
    assert_eq!(resolved, MachineProvider::Krunkit);
}

#[test]
fn environment_override_is_case_and_whitespace_insensitive() {
    for token in ["VFKIT", "Vfkit", "  vfkit  ", "\tVFKIT\n"] {
        let resolved = resolve_machine_provider_from(None, Some(token))
            .expect("case/whitespace variants should resolve");
        assert_eq!(
            resolved,
            MachineProvider::Vfkit,
            "override {token:?} should resolve to vfkit"
        );
    }
}

#[test]
fn unknown_environment_override_is_rejected() {
    let error = resolve_machine_provider_from(None, Some("qemu"))
        .expect_err("an unknown provider token must not silently fall back to a default");
    let message = error.to_string();
    assert!(
        message.contains("qemu"),
        "error should echo the offending token: {message}"
    );
    assert!(
        message.contains("krunkit") && message.contains("vfkit"),
        "error should enumerate the known providers: {message}"
    );
}
