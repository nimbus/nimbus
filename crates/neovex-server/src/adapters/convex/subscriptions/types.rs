use super::*;

#[derive(Debug, Clone)]
pub(in crate::adapters::convex) enum ConvexSubscriptionTransform {
    Identity,
    Get {
        document_id: DocumentId,
    },
    First,
    Unique,
    RuntimeNamedQuery {
        name: String,
        args: Value,
        auth: Option<InvocationAuth>,
        read_set: Option<RuntimeReadSet>,
    },
    RuntimeNamedPaginatedQuery {
        name: String,
        args: Value,
        page_size: usize,
        cursor: Option<String>,
        auth: Option<InvocationAuth>,
        read_set: Option<RuntimeReadSet>,
    },
}

#[derive(Debug)]
pub(in crate::adapters::convex) struct ConvexRuntimeSubscriptionSetup {
    pub(in crate::adapters::convex) initial_value: Value,
    pub(in crate::adapters::convex) base_queries: Vec<Query>,
    pub(in crate::adapters::convex) transform: ConvexSubscriptionTransform,
}

#[derive(Debug, Default)]
pub(in crate::adapters::convex) struct ConvexSubscriptionTransforms {
    pub(in crate::adapters::convex) by_id: HashMap<u64, ConvexSubscriptionTransform>,
    pub(in crate::adapters::convex) by_request: HashMap<String, ConvexSubscriptionTransform>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::adapters::convex) enum ConvexClientMessage {
    Authenticate {
        token: String,
    },
    ClearAuth,
    Subscribe {
        request_id: String,
        query: Query,
    },
    SubscribeNamed {
        request_id: String,
        name: String,
        #[serde(default = "empty_args")]
        args: Value,
        #[serde(default)]
        page_size: Option<usize>,
        #[serde(default)]
        cursor: Option<String>,
    },
    Unsubscribe {
        subscription_id: u64,
    },
}
