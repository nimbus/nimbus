use super::*;

impl TenantPersistence {
    delegate_store_method!(
        #[cfg(test)]
        fn upsert_resource_path_binding(
            &self,
            binding: &ResourcePathBinding
        ) -> Result<()>
    );
    delegate_store_method!(
        fn resource_path_binding(
            &self,
            locator: &DocumentLocator
        ) -> Result<Option<ResourcePathBinding>>
    );
}
