//! Immutable exact registry for small workload teardown capabilities.

#[cfg(test)]
mod tests {
    use super::super::teardown_test_support::production_source;

    const SOURCE: &str = include_str!("teardown_registry.rs");

    #[test]
    fn registry_rejects_duplicate_role_provider_registration() {
        let source = production_source(SOURCE);
        assert!(source.contains("struct WorkloadTeardownCapabilityRegistry"));
        assert!(source.contains("DuplicateIngressProvider"));
        assert!(source.contains("DuplicateExecutionProvider"));
        assert!(source.contains("DuplicateAttachmentProvider"));
        assert!(source.contains("NetworkRoleConflict"));
    }
}
