use nimbus_core::{CollectionName, DocumentLocator, DocumentPath, ResourcePathBinding, Result};

use super::{MemoryTenantSnapshot, MemoryTenantStore};

fn sorted_bindings(
    bindings: impl IntoIterator<Item = ResourcePathBinding>,
) -> Vec<ResourcePathBinding> {
    let mut bindings = bindings.into_iter().collect::<Vec<_>>();
    bindings.sort_by(|left, right| {
        left.document_path
            .to_string()
            .cmp(&right.document_path.to_string())
            .then_with(|| left.locator.table.cmp(&right.locator.table))
            .then_with(|| left.locator.id.cmp(&right.locator.id))
    });
    bindings
}

impl MemoryTenantSnapshot {
    pub fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        Ok(sorted_bindings(
            self.state.resource_bindings.values().cloned(),
        ))
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        Ok(self.state.resource_bindings.get(locator).cloned())
    }

    pub fn locator_for_document_path(
        &self,
        path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        Ok(self.state.document_paths.get(path).cloned())
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        Ok(sorted_bindings(
            self.state
                .resource_bindings
                .values()
                .filter(|binding| binding.collection_group() == collection_group)
                .cloned(),
        ))
    }
}

impl MemoryTenantStore {
    pub fn upsert_resource_path_binding(&self, binding: &ResourcePathBinding) -> Result<()> {
        self.transact(|state| state.upsert_resource_path_binding(binding))
    }

    pub fn remove_resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.transact(|state| Ok(state.remove_resource_path_binding(locator)))
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        self.read_snapshot()?.resource_path_binding(locator)
    }

    pub fn locator_for_document_path(
        &self,
        path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        self.read_snapshot()?.locator_for_document_path(path)
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        self.read_snapshot()?
            .scan_collection_group_bindings(collection_group)
    }
}
