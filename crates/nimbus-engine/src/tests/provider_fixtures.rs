use std::env;

use super::*;

const REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str = "NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES";

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

pub(crate) async fn expect_external_provider_future_within<T, Fut>(
    description: &str,
    local: Duration,
    ci: Duration,
    future: Fut,
) -> T
where
    Fut: Future<Output = T>,
{
    let timeout_budget = ci_or_local_duration(local, ci);
    timeout(timeout_budget, future)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{description} within the bounded external-provider correctness timeout of {timeout_budget:?}"
            )
        })
}
