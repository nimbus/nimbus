use super::*;

#[derive(Debug, Clone)]
pub(super) enum ConvexSubscriptionTransform {
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
        read_set: Option<ConvexRuntimeReadSet>,
    },
    RuntimeNamedPaginatedQuery {
        name: String,
        args: Value,
        page_size: usize,
        cursor: Option<String>,
        auth: Option<InvocationAuth>,
        read_set: Option<ConvexRuntimeReadSet>,
    },
}

#[derive(Debug)]
pub(super) struct ConvexRuntimeSubscriptionSetup {
    pub(super) initial_value: Value,
    pub(super) base_queries: Vec<Query>,
    pub(super) transform: ConvexSubscriptionTransform,
}

#[derive(Debug)]
pub(super) struct ConvexRuntimeSubscriptionHandle {
    pub(super) convex_subscription_id: u64,
    pub(super) underlying_subscription_ids: Vec<u64>,
}

#[derive(Debug, Default)]
pub(super) struct ConvexSubscriptionTransforms {
    pub(super) by_id: HashMap<u64, ConvexSubscriptionTransform>,
    pub(super) by_request: HashMap<String, ConvexSubscriptionTransform>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ConvexClientMessage {
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
