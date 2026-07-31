//! `nimbus-dynamodb` — Amazon DynamoDB wire-protocol compatibility adapter.
//!
//! This crate owns DynamoDB wire semantics, AttributeValue conversion,
//! expression bridging, operation dispatch, SigV4 verification, and stream
//! shaping over explicit Nimbus capabilities (e.g. `Arc<Engine>`). It is
//! transport-agnostic: it exposes a dispatch entrypoint rather than an HTTP
//! router, and must not depend on `nimbus-server` or `axum`. `nimbus-server`
//! mounts the dispatch on its own `POST /` route (see
//! `docs/private/plans/archive/dynamodb-adapter-plan.md`).
//!
//! Scaffolded in D0.0. The protocol surfaces — wire envelope, AttributeValue
//! codec, composite-key encoding, error mapping, control plane, item ops,
//! Query/Scan, batch/transactions, secondary indexes, Streams, TTL, tagging —
//! land from D0.1 onward.

pub mod attribute_value;
pub mod auth;
pub mod commands;
pub mod config;
pub mod dispatch;
pub mod error;
pub mod expression;
pub mod item_size;
pub mod key;
pub mod key_management;
pub mod tenant;
pub mod wire;

pub use error::map_core_error;
pub use tenant::{
    ACCESS_KEY_CLAIM, AccessKeyRegistry, AuthMode, KeyBinding, access_key_principal,
    adapter_principal, caller_principal, ensure_tenant, ensure_tenant_async, maintenance_context,
    request_context,
};

pub use attribute_value::{fields_to_item, item_to_fields, validate_item};
pub use commands::ttl::{sweep_all_tenants, sweep_all_tenants_async};
pub use config::DynamoDbConfig;
pub use dispatch::{
    DispatchContext, KNOWN_OPERATIONS, dispatch, dispatch_async, is_known_operation,
};
pub use item_size::{
    MAX_ITEM_SIZE_BYTES, attribute_value_size, item_size_bytes, validate_item_size,
    validate_updated_item_size,
};
pub use key::{
    decode_key, encode_key, is_reserved_attribute_name, sortable_key, validate_attribute_names,
};
pub use key_management::{
    RedactedAccessKey, StoredAccessKey, delete_access_key, list_access_keys, put_access_key,
    put_access_key_async, rotate_secret,
};
pub use wire::{WireResponse, render_error, render_success};
