//! Bounded durable driver for one workload teardown generation.

#[cfg(test)]
mod tests {
    use super::super::teardown_test_support::{assert_tokens_in_order, production_source};

    const SOURCE: &str = include_str!("teardown_driver.rs");

    #[test]
    fn teardown_driver_records_exact_five_step_order() {
        let source = production_source(SOURCE);
        assert_tokens_in_order(
            source,
            &[
                "decide_teardown",
                "confirm_transition",
                "dispatch_confirmed",
                "confirm_teardown_result",
                "decide_teardown",
            ],
        );
    }
}
