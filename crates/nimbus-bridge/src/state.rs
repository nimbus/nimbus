use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Cursor, DocumentId, Filter, Query, TableId, TableName, TenantId};
use nimbus_runtime::NimbusRuntimeError;

use super::read_tracking::{RuntimeIndexRangeRead, RuntimeReadSet};

#[derive(Clone)]
pub struct RuntimeHostState {
    server_request_id: Option<String>,
    host_call_session_id: String,
    active_host_call_sessions: Arc<Mutex<HashMap<String, usize>>>,
    max_nested_runtime_invocations: usize,
    remaining_nested_runtime_invocations: Arc<AtomicUsize>,
    read_set: Arc<Mutex<RuntimeReadSet>>,
    document_read_table_ids: Arc<Mutex<HashMap<TableName, Option<TableId>>>>,
}

impl RuntimeHostState {
    pub fn new(
        host_call_session_id: impl Into<String>,
        server_request_id: Option<String>,
        max_nested_runtime_invocations: usize,
    ) -> Self {
        let host_call_session_id = host_call_session_id.into();
        let mut active_host_call_sessions = HashMap::new();
        active_host_call_sessions.insert(host_call_session_id.clone(), 1);
        Self {
            server_request_id,
            host_call_session_id,
            active_host_call_sessions: Arc::new(Mutex::new(active_host_call_sessions)),
            max_nested_runtime_invocations,
            remaining_nested_runtime_invocations: Arc::new(AtomicUsize::new(
                max_nested_runtime_invocations,
            )),
            read_set: Arc::new(Mutex::new(RuntimeReadSet::default())),
            document_read_table_ids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn server_request_id(&self) -> Option<&str> {
        self.server_request_id.as_deref()
    }

    pub fn host_call_session_id(&self) -> &str {
        &self.host_call_session_id
    }

    pub fn enter_host_call_session(
        &self,
        host_call_session_id: impl Into<String>,
    ) -> std::result::Result<RuntimeHostCallSessionGuard, NimbusRuntimeError> {
        let host_call_session_id = host_call_session_id.into();
        if host_call_session_id.is_empty() {
            return Err(NimbusRuntimeError::Contract(
                "runtime host-call token must not be empty for nested runtime invocation"
                    .to_string(),
            ));
        }
        {
            let mut active_sessions = self
                .active_host_call_sessions
                .lock()
                .expect("runtime host session lock should not be poisoned");
            *active_sessions
                .entry(host_call_session_id.clone())
                .or_insert(0) += 1;
        }
        Ok(RuntimeHostCallSessionGuard {
            active_host_call_sessions: Arc::clone(&self.active_host_call_sessions),
            host_call_session_id,
        })
    }

    pub fn snapshot_read_set(&self) -> RuntimeReadSet {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .clone()
    }

    pub fn validate_host_call_session(
        &self,
        tenant_id: &TenantId,
        host_call_session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        match host_call_session_id {
            Some("") => Err(NimbusRuntimeError::Contract(format!(
                "runtime host-call token must not be empty for tenant {tenant_id}"
            ))),
            Some(actual)
                if !self
                    .active_host_call_sessions
                    .lock()
                    .expect("runtime host session lock should not be poisoned")
                    .contains_key(actual) =>
            {
                Err(NimbusRuntimeError::Contract(format!(
                    "runtime host-call token does not match the active session for tenant {tenant_id}"
                )))
            }
            _ => Ok(()),
        }
    }

    pub fn consume_nested_runtime_invocation_budget(
        &self,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        let mut remaining = self
            .remaining_nested_runtime_invocations
            .load(Ordering::SeqCst);
        loop {
            if remaining == 0 {
                return Err(NimbusRuntimeError::NestedInvocationLimitExceeded(
                    self.max_nested_runtime_invocations,
                ));
            }
            match self.remaining_nested_runtime_invocations.compare_exchange(
                remaining,
                remaining - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Ok(()),
                Err(next_remaining) => remaining = next_remaining,
            }
        }
    }

    pub fn record_table_read(&self, table: &TableName, table_id: Option<&TableId>) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_table(table, table_id);
    }

    pub fn record_document_read(
        &self,
        table: &TableName,
        table_id: Option<&TableId>,
        document_id: &DocumentId,
    ) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_document(table, table_id, document_id);
    }

    pub(crate) fn document_read_table_id<F>(&self, table: &TableName, resolve: F) -> Option<TableId>
    where
        F: FnOnce(&TableName) -> Option<TableId>,
    {
        if let Some(table_id) = self
            .document_read_table_ids
            .lock()
            .expect("runtime host table-id cache lock should not be poisoned")
            .get(table)
            .cloned()
        {
            return table_id;
        }

        let table_id = resolve(table);
        self.document_read_table_ids
            .lock()
            .expect("runtime host table-id cache lock should not be poisoned")
            .insert(table.clone(), table_id.clone());
        table_id
    }

    pub fn record_paginated_window_read(
        &self,
        query: &Query,
        table_id: Option<&TableId>,
        page_size: usize,
        after: Option<&Cursor>,
        page: &nimbus_core::Page,
    ) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_paginated_window(query, table_id, page_size, after, page);
    }

    pub fn record_index_read(&self, read: RuntimeIndexRangeRead) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_index_range(read);
    }

    pub fn record_predicate_read(
        &self,
        table: &TableName,
        table_id: Option<&TableId>,
        filters: &[Filter],
    ) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_predicate(table, table_id, filters);
    }
}

pub struct RuntimeHostCallSessionGuard {
    active_host_call_sessions: Arc<Mutex<HashMap<String, usize>>>,
    host_call_session_id: String,
}

impl Drop for RuntimeHostCallSessionGuard {
    fn drop(&mut self) {
        let mut active_sessions = self
            .active_host_call_sessions
            .lock()
            .expect("runtime host session lock should not be poisoned");
        let Some(count) = active_sessions.get_mut(&self.host_call_session_id) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            active_sessions.remove(&self.host_call_session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn tenant_id() -> TenantId {
        TenantId::new("tenant-a").expect("tenant id should parse")
    }

    #[test]
    fn host_call_session_validation_binds_non_empty_tokens_to_active_session() {
        let state = RuntimeHostState::new("runtime-host", None, 1);
        let tenant_id = tenant_id();
        let active_session = state.host_call_session_id().to_string();

        state
            .validate_host_call_session(&tenant_id, None)
            .expect("missing token remains valid for host calls that do not carry one");
        state
            .validate_host_call_session(&tenant_id, Some(&active_session))
            .expect("active session token should validate");

        let empty_error = state
            .validate_host_call_session(&tenant_id, Some(""))
            .expect_err("empty host-call token should be rejected");
        assert!(
            empty_error.to_string().contains("must not be empty"),
            "empty-token error should name the contract: {empty_error}"
        );

        let mismatch_error = state
            .validate_host_call_session(&tenant_id, Some("stale-runtime-host-999"))
            .expect_err("stale host-call token should be rejected");
        assert!(
            mismatch_error.to_string().contains("does not match"),
            "mismatched-token error should name the active-session binding: {mismatch_error}"
        );
    }

    #[test]
    fn host_call_session_validation_accepts_nested_tokens_only_while_active() {
        let state = RuntimeHostState::new("query:outer", None, 1);
        let tenant_id = tenant_id();

        state
            .validate_host_call_session(&tenant_id, Some("mutation:inner"))
            .expect_err("nested token should not validate before nested invocation starts");

        let first_guard = state
            .enter_host_call_session("mutation:inner")
            .expect("nested token should register");
        state
            .validate_host_call_session(&tenant_id, Some("mutation:inner"))
            .expect("active nested token should validate");

        let second_guard = state
            .enter_host_call_session("mutation:inner")
            .expect("duplicate nested token should ref-count");
        drop(first_guard);
        state
            .validate_host_call_session(&tenant_id, Some("mutation:inner"))
            .expect("duplicate nested token should remain active until every guard drops");

        drop(second_guard);
        state
            .validate_host_call_session(&tenant_id, Some("mutation:inner"))
            .expect_err("nested token should stop validating after the nested call finishes");
        state
            .validate_host_call_session(&tenant_id, Some("query:outer"))
            .expect("outer token should remain active");
    }

    #[test]
    fn document_read_table_id_cache_memoizes_present_and_missing_tables() {
        let state = RuntimeHostState::new("runtime-host", None, 1);
        let table = TableName::new("messages").expect("table should parse");
        let table_id = TableId::new();
        let resolve_count = Cell::new(0usize);

        assert_eq!(
            state.document_read_table_id(&table, |_| {
                resolve_count.set(resolve_count.get() + 1);
                Some(table_id.clone())
            }),
            Some(table_id.clone())
        );
        assert_eq!(
            state.document_read_table_id(&table, |_| {
                resolve_count.set(resolve_count.get() + 1);
                None
            }),
            Some(table_id)
        );
        assert_eq!(
            resolve_count.get(),
            1,
            "present table id should be resolved once per runtime host state"
        );

        let missing_table = TableName::new("missing_messages").expect("table should parse");
        let missing_resolve_count = Cell::new(0usize);
        assert_eq!(
            state.document_read_table_id(&missing_table, |_| {
                missing_resolve_count.set(missing_resolve_count.get() + 1);
                None
            }),
            None
        );
        assert_eq!(
            state.document_read_table_id(&missing_table, |_| {
                missing_resolve_count.set(missing_resolve_count.get() + 1);
                Some(TableId::new())
            }),
            None
        );
        assert_eq!(
            missing_resolve_count.get(),
            1,
            "missing table lookup should also be cached for the runtime invocation"
        );
    }
}
