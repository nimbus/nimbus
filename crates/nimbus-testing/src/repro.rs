#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicTestCase {
    id: &'static str,
    profile: &'static str,
    description: &'static str,
}

impl DeterministicTestCase {
    pub const fn new(id: &'static str, profile: &'static str, description: &'static str) -> Self {
        Self {
            id,
            profile,
            description,
        }
    }

    pub fn id(self) -> &'static str {
        self.id
    }

    pub fn profile(self) -> &'static str {
        self.profile
    }

    pub fn description(self) -> &'static str {
        self.description
    }

    pub fn describe(self) -> String {
        format!("case {} [{}]: {}", self.id, self.profile, self.description)
    }

    pub fn failure_context(self, invariant: &str) -> String {
        format!("{invariant}; {}", self.describe())
    }

    pub fn failure_context_with_repro(self, invariant: &str, repro_command: &str) -> String {
        format!(
            "{} Repro: {}",
            self.failure_context(invariant),
            repro_command
        )
    }
}

#[cfg(test)]
mod tests {
    use super::DeterministicTestCase;

    #[test]
    fn deterministic_test_case_formats_context_and_repro() {
        let case = DeterministicTestCase::new(
            "websocket-auth-change-resubscribe",
            "run-to-completion-snapshot",
            "auth changes force explicit resubscribe for runtime-backed subscriptions",
        );

        assert_eq!(
            case.describe(),
            "case websocket-auth-change-resubscribe [run-to-completion-snapshot]: auth changes force explicit resubscribe for runtime-backed subscriptions"
        );
        assert_eq!(
            case.failure_context("subscription bootstrap should recover"),
            "subscription bootstrap should recover; case websocket-auth-change-resubscribe [run-to-completion-snapshot]: auth changes force explicit resubscribe for runtime-backed subscriptions"
        );
        assert_eq!(
            case.failure_context_with_repro(
                "subscription bootstrap should recover",
                "cargo test -p nimbus-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed -- --nocapture",
            ),
            "subscription bootstrap should recover; case websocket-auth-change-resubscribe [run-to-completion-snapshot]: auth changes force explicit resubscribe for runtime-backed subscriptions Repro: cargo test -p nimbus-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed -- --nocapture"
        );
    }
}
