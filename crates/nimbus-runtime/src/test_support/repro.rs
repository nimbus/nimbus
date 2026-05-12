#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeReproCase {
    id: &'static str,
    profile: &'static str,
    description: &'static str,
}

impl RuntimeReproCase {
    pub(crate) const fn new(
        id: &'static str,
        profile: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            id,
            profile,
            description,
        }
    }

    pub(crate) const fn id(self) -> &'static str {
        self.id
    }

    pub(crate) fn describe(self) -> String {
        format!("case {} [{}]: {}", self.id, self.profile, self.description)
    }

    pub(crate) fn failure_context(self, invariant: &str) -> String {
        format!("{invariant}; {}", self.describe())
    }

    pub(crate) fn failure_context_with_repro(self, invariant: &str, repro_command: &str) -> String {
        format!(
            "{} Repro: {}",
            self.failure_context(invariant),
            repro_command
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IsolatedRuntimeTestCase {
    metadata: RuntimeReproCase,
    subprocess_test_name: &'static str,
}

impl IsolatedRuntimeTestCase {
    pub(crate) const fn new(
        id: &'static str,
        profile: &'static str,
        description: &'static str,
        subprocess_test_name: &'static str,
    ) -> Self {
        Self {
            metadata: RuntimeReproCase::new(id, profile, description),
            subprocess_test_name,
        }
    }

    pub(crate) fn subprocess_test_name(self) -> &'static str {
        self.subprocess_test_name
    }

    pub(crate) const fn metadata(self) -> RuntimeReproCase {
        self.metadata
    }

    pub(crate) fn repro_command(self) -> String {
        format!(
            "cargo test -p nimbus-runtime {} -- --ignored --exact --nocapture",
            self.subprocess_test_name
        )
    }

    pub(crate) fn failure_context(self, invariant: &str) -> String {
        self.metadata
            .failure_context_with_repro(invariant, &self.repro_command())
    }
}
