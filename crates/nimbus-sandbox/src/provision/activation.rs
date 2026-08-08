//! Pure classification for provider-local workload activation inspection.

/// Closed runtime state supplied by an effect-owning provider adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProvisionActivationRuntimeState {
    /// The workload process is currently running.
    Running,
    /// Provider creation is still converging without a retry decision.
    Starting,
    /// Provider substrate exists, but the running effect is absent and retryable.
    Startable,
    /// The workload process exited after activation may have produced effects.
    Exited,
    /// The provider reported a state without a safe retry classification.
    Unknown,
}

/// Provider-neutral result consumed by one provision phase inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProvisionActivationObservationKind {
    Succeeded,
    Absent,
    InProgress,
    Ambiguous,
}

/// Classify one normalized provider runtime state without I/O or mutation.
pub(crate) const fn classify_provision_activation(
    state: ProvisionActivationRuntimeState,
) -> ProvisionActivationObservationKind {
    match state {
        ProvisionActivationRuntimeState::Running => ProvisionActivationObservationKind::Succeeded,
        ProvisionActivationRuntimeState::Starting => ProvisionActivationObservationKind::InProgress,
        ProvisionActivationRuntimeState::Startable => ProvisionActivationObservationKind::Absent,
        ProvisionActivationRuntimeState::Exited => ProvisionActivationObservationKind::Ambiguous,
        ProvisionActivationRuntimeState::Unknown => ProvisionActivationObservationKind::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_classifier_has_one_closed_outcome_for_each_runtime_state() {
        assert_eq!(
            classify_provision_activation(ProvisionActivationRuntimeState::Running),
            ProvisionActivationObservationKind::Succeeded
        );
        assert_eq!(
            classify_provision_activation(ProvisionActivationRuntimeState::Starting),
            ProvisionActivationObservationKind::InProgress
        );
        assert_eq!(
            classify_provision_activation(ProvisionActivationRuntimeState::Startable),
            ProvisionActivationObservationKind::Absent
        );
        assert_eq!(
            classify_provision_activation(ProvisionActivationRuntimeState::Exited),
            ProvisionActivationObservationKind::Ambiguous
        );
        assert_eq!(
            classify_provision_activation(ProvisionActivationRuntimeState::Unknown),
            ProvisionActivationObservationKind::Ambiguous
        );
    }
}
