//! Test-only selection policy for external provider fixtures.
//!
//! Docker lifecycle belongs to `scripts/external-provider-fixture.sh`. Rust
//! provider tests either consume the explicit URLs exported by that interface,
//! deliberately omit provider execution in an ordinary workspace lane, or fail
//! with an actionable command. They never provision an implicit container.

use std::env;

pub const REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str =
    "NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES";
pub const DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str =
    "NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalProviderFixtureMode {
    UseExplicit,
    Omit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureInputDecision {
    UseExplicit,
    Omit,
    Reject,
}

fn classify_fixture_inputs(
    required_env_present: &[bool],
    fixtures_required: bool,
    fixtures_disabled: bool,
) -> FixtureInputDecision {
    if required_env_present.iter().all(|present| *present) {
        return FixtureInputDecision::UseExplicit;
    }

    let any_present = required_env_present.iter().any(|present| *present);
    if any_present || fixtures_required || !fixtures_disabled {
        return FixtureInputDecision::Reject;
    }

    FixtureInputDecision::Omit
}

fn nonempty_env(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty())
}

/// Select the only legal fixture mode for provider-backed tests.
///
/// `UseExplicit` means every required URL is present. `Omit` is reserved for
/// ordinary workspace lanes that explicitly disable external providers. Every
/// other configuration fails; in particular, a direct provider test can no
/// longer start a drifting Testcontainers image or silently skip itself.
pub fn external_provider_fixture_mode(
    provider: &str,
    provider_label: &str,
    required_env_names: &[&str],
) -> ExternalProviderFixtureMode {
    let required_env_present: Vec<bool> = required_env_names
        .iter()
        .map(|name| nonempty_env(name))
        .collect();
    let fixtures_required = env::var_os(REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV).is_some();
    let fixtures_disabled = env::var_os(DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV).is_some();

    match classify_fixture_inputs(&required_env_present, fixtures_required, fixtures_disabled) {
        FixtureInputDecision::UseExplicit => ExternalProviderFixtureMode::UseExplicit,
        FixtureInputDecision::Omit => {
            eprintln!(
                "omitting {provider_label} execution because {DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV}=1; this workspace result is not external-provider evidence"
            );
            ExternalProviderFixtureMode::Omit
        }
        FixtureInputDecision::Reject => {
            let missing: Vec<&str> = required_env_names
                .iter()
                .copied()
                .zip(required_env_present)
                .filter_map(|(name, present)| (!present).then_some(name))
                .collect();
            panic!(
                "{provider_label} tests require the pinned shared fixture; missing non-empty environment variable(s): {}. Run `make test-external-provider PROVIDER={provider}`. Ordinary workspace lanes that intentionally omit provider execution must set {DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV}=1. Automatic per-test containers are not supported.",
                missing.join(", ")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_input_decision_table_is_exhaustive() {
        for (present, required, disabled, expected) in [
            (vec![true], false, false, FixtureInputDecision::UseExplicit),
            (vec![true], true, true, FixtureInputDecision::UseExplicit),
            (vec![false], false, true, FixtureInputDecision::Omit),
            (vec![false], false, false, FixtureInputDecision::Reject),
            (vec![false], true, false, FixtureInputDecision::Reject),
            (vec![false], true, true, FixtureInputDecision::Reject),
            (vec![true, false], false, true, FixtureInputDecision::Reject),
            (vec![false, true], false, true, FixtureInputDecision::Reject),
        ] {
            assert_eq!(
                classify_fixture_inputs(&present, required, disabled),
                expected,
                "unexpected decision for present={present:?}, required={required}, disabled={disabled}"
            );
        }
    }
}
