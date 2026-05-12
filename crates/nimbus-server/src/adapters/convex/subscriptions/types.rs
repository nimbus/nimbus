use std::sync::Arc;

use super::*;
use nimbus_runtime::InvocationServices;

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
        services: InvocationServices,
        read_set: Option<RuntimeReadSet>,
        last_value: Option<Arc<Value>>,
    },
    RuntimeNamedPaginatedQuery {
        name: String,
        args: Value,
        page_size: usize,
        cursor: Option<String>,
        auth: Option<InvocationAuth>,
        services: InvocationServices,
        read_set: Option<RuntimeReadSet>,
        last_value: Option<Arc<Value>>,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_runtime::InvocationServices;
    use serde_json::json;

    use super::ConvexSubscriptionTransform;

    #[test]
    fn runtime_transform_clone_reuses_last_value_arc() {
        let last_value = Arc::new(json!({
            "runtime": true,
            "value": ["large", "payload"],
        }));
        let transform = ConvexSubscriptionTransform::RuntimeNamedQuery {
            name: "messages:list".to_string(),
            args: json!({}),
            auth: None,
            services: InvocationServices::default(),
            read_set: None,
            last_value: Some(last_value.clone()),
        };

        let cloned = transform.clone();

        let ConvexSubscriptionTransform::RuntimeNamedQuery {
            last_value: Some(cloned_last_value),
            ..
        } = cloned
        else {
            panic!("cloned transform should stay a runtime named query");
        };

        assert!(
            Arc::ptr_eq(&last_value, &cloned_last_value),
            "cloning the transform should retain the same shared payload pointer",
        );
    }
}
