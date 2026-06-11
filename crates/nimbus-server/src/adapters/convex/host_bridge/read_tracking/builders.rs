use super::*;

fn lock_query_builders(
    builders: &std::sync::Mutex<ConvexRuntimeQueryBuilders>,
) -> std::sync::MutexGuard<'_, ConvexRuntimeQueryBuilders> {
    builders
        .lock()
        .expect("convex runtime query builder lock should not be poisoned")
}

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn new_builder_id(&self) -> String {
        let mut builders = lock_query_builders(self.query_builders().as_ref());
        builders.next_builder_id += 1;
        format!(
            "{}-builder-{}",
            self.host_call_session_id(),
            builders.next_builder_id
        )
    }

    pub(in crate::adapters::convex) fn insert_builder(
        &self,
        builder_id: String,
        state: ConvexRuntimeQueryBuilderState,
    ) {
        lock_query_builders(self.query_builders().as_ref())
            .builders
            .insert(builder_id, state);
    }

    pub(in crate::adapters::convex) fn with_builder_mut<R>(
        &self,
        builder_id: &str,
        update: impl FnOnce(&mut ConvexRuntimeQueryBuilderState) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut builders = lock_query_builders(self.query_builders().as_ref());
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
        lock_query_builders(self.query_builders().as_ref())
            .builders
            .remove(builder_id)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "convex runtime query builder not found: {builder_id}"
                ))
            })
    }
}
