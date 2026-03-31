use std::sync::atomic::{AtomicU64, Ordering};

use crate::runtime::InvocationRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInvocationContext {
    pub invocation_id: u64,
    pub function_name: String,
    pub kind: &'static str,
    pub is_top_level: bool,
}

impl RuntimeInvocationContext {
    pub fn top_level(request: &InvocationRequest) -> Self {
        static NEXT_INVOCATION_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            invocation_id: NEXT_INVOCATION_ID.fetch_add(1, Ordering::Relaxed),
            function_name: request.function_name.clone(),
            kind: request.kind.as_str(),
            is_top_level: true,
        }
    }
}
