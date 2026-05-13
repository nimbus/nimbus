use std::env;

const REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str = "NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES";
const DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES_ENV: &str =
    "NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES";

pub(crate) fn require_explicit_external_provider_fixture_envs(
    provider_label: &str,
    env_names: &[&str],
) {
    if env::var_os(REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV).is_none() {
        return;
    }

    let missing: Vec<&str> = env_names
        .iter()
        .copied()
        .filter(|name| env::var_os(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "{REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV} is set, so {provider_label} tests require explicit fixture env vars: {}",
        missing.join(", ")
    );
}

pub(crate) fn implicit_external_provider_fixtures_disabled(provider_label: &str) -> bool {
    if env::var_os(DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES_ENV).is_none() {
        return false;
    }

    eprintln!(
        "skipping {provider_label} test because {DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES_ENV} is set and no explicit provider fixture URL was provided"
    );
    true
}
