use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn new_builder_id(&self) -> String {
        let mut builders = self
            .query_builders()
            .lock()
            .expect("convex runtime query builder lock should not be poisoned");
        builders.next_builder_id += 1;
        format!("{}-builder-{}", self.session_id(), builders.next_builder_id)
    }

    pub(in crate::adapters::convex) fn insert_builder(
        &self,
        builder_id: String,
        state: ConvexRuntimeQueryBuilderState,
    ) {
        self.query_builders()
            .lock()
            .expect("convex runtime query builder lock should not be poisoned")
            .builders
            .insert(builder_id, state);
    }

    pub(in crate::adapters::convex) fn with_builder_mut<R>(
        &self,
        builder_id: &str,
        update: impl FnOnce(&mut ConvexRuntimeQueryBuilderState) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut builders = self
            .query_builders()
            .lock()
            .expect("convex runtime query builder lock should not be poisoned");
        let state = builders.builders.get_mut(builder_id).ok_or_else(|| {
            Error::InvalidInput(format!(
                "convex runtime query builder not found: {builder_id}"
            ))
        })?;
        update(state)
    }

    pub(in crate::adapters::convex) fn take_builder(
        &self,
        builder_id: &str,
    ) -> Result<ConvexRuntimeQueryBuilderState, Error> {
        self.query_builders()
            .lock()
            .expect("convex runtime query builder lock should not be poisoned")
            .builders
            .remove(builder_id)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "convex runtime query builder not found: {builder_id}"
                ))
            })
    }
}

impl ConvexRuntimeQueryBuilderState {
    pub(in crate::adapters::convex) fn into_query(self, limit: Option<usize>) -> Query {
        Query {
            table: self.table,
            filters: self.filters,
            order: self.order,
            limit,
        }
    }
}
