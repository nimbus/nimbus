use std::env;
use std::time::Duration;

const REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str = "NEOVEX_REQUIRE_EXTERNAL_PROVIDER_FIXTURES";

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

pub(crate) fn external_provider_test_timeout(local: Duration, ci: Duration) -> Duration {
    if env::var_os("CI").is_some() {
        ci
    } else {
        local
    }
}
