use nimbus_core::{
    CommitEntry, DependencySet, Document, PaginatedWindowDependency, Query, TableId,
    commit_intersects_dependency_set,
};

use super::registry::SubscriptionRegistry;

pub(crate) fn subscription_dependencies(
    query: &Query,
    table_id: Option<TableId>,
    documents: &[Document],
) -> DependencySet {
    let Some(page_size) = query.limit else {
        return DependencySet::from_engine_query(query, table_id);
    };

    let Some(table_id) = table_id else {
        let mut dependencies = DependencySet::default();
        if query.filters.is_empty() {
            dependencies.record_missing_table(&query.table);
        } else {
            dependencies.record_missing_predicate(&query.table, query.filters.clone());
        }
        return dependencies;
    };

    let (end_sort_values, end_doc_id) = documents
        .last()
        .map(|document| {
            (
                query.order.as_ref().map_or_else(Vec::new, |order| {
                    vec![document.get_field(&order.field).cloned()]
                }),
                Some(document.id.clone()),
            )
        })
        .unwrap_or_else(|| (Vec::new(), None));

    let mut dependencies = DependencySet::default();
    dependencies.record_paginated_window(PaginatedWindowDependency {
        table: query.table.clone(),
        table_id,
        filters: query.filters.clone(),
        order: query.order.clone(),
        start_sort_values: Vec::new(),
        start_doc_id: None,
        end_sort_values,
        end_doc_id,
        result_count: documents.len(),
        page_size,
    });
    dependencies
}

pub(crate) struct SubscriptionBatchCandidate<'a> {
    pub commit: &'a CommitEntry,
    pub candidate_documents: &'a [Document],
}

pub(crate) struct BatchAffectedSubscriptions {
    pub subscription_ids: Vec<u64>,
    pub merged_wakeup_count: u64,
}

impl SubscriptionRegistry {
    /// Returns the ids of subscriptions affected by the provided commit.
    pub fn affected_subscription_ids(
        &self,
        commit: &CommitEntry,
        candidate_documents: &[Document],
    ) -> Vec<u64> {
        self.state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .values()
            .filter(|subscription| subscription.active)
            .filter(|subscription| {
                commit_intersects_dependency_set(
                    commit,
                    &subscription.dependencies,
                    candidate_documents,
                    |_, _| Ok(None),
                )
            })
            .map(|subscription| subscription.id)
            .collect()
    }

    pub(crate) fn affected_subscription_ids_for_batch(
        &self,
        batch: &[SubscriptionBatchCandidate<'_>],
    ) -> BatchAffectedSubscriptions {
        let mut subscription_ids = Vec::new();
        let mut merged_wakeup_count = 0_u64;
        for subscription in self
            .state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .values()
            .filter(|subscription| subscription.active)
        {
            let mut match_count = 0_u64;
            for candidate in batch {
                if commit_intersects_dependency_set(
                    candidate.commit,
                    &subscription.dependencies,
                    candidate.candidate_documents,
                    |_, _| Ok(None),
                ) {
                    match_count += 1;
                }
            }
            if match_count != 0 {
                subscription_ids.push(subscription.id);
                merged_wakeup_count += match_count.saturating_sub(1);
            }
        }
        BatchAffectedSubscriptions {
            subscription_ids,
            merged_wakeup_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::{PrincipalContext, Query, TableName};
    use tokio::sync::mpsc;

    use super::SubscriptionRegistry;
    use crate::subscriptions::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY;

    #[test]
    fn unfiltered_pending_registrations_store_missing_table_dependency_until_bootstrap() {
        let registry = SubscriptionRegistry::new();
        let (tx, _rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let query = Query {
            table: TableName::new("tasks").expect("table name should be valid"),
            filters: Vec::new(),
            order: None,
            limit: None,
        };

        let registration = registry.register(
            query.clone(),
            PrincipalContext::anonymous(),
            "policy-v1".to_string(),
            tx,
            true,
        );
        let stored = registry
            .state
            .subscriptions
            .read()
            .expect("subscription lock should not be poisoned")
            .get(&registration.id())
            .expect("subscription should be stored")
            .dependencies
            .clone();

        assert!(stored.missing_tables.contains(&query.table));
        assert!(stored.tables.is_empty());
        assert!(stored.predicates.is_empty());
    }
}
