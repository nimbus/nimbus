use super::*;

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
