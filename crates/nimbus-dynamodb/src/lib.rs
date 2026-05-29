//! `nimbus-dynamodb` — Amazon DynamoDB wire-protocol compatibility adapter.
//!
//! This crate owns DynamoDB wire semantics, AttributeValue conversion,
//! expression bridging, operation dispatch, SigV4 verification, and stream
//! shaping over explicit Nimbus capabilities (e.g. `Arc<Service>`). It is
//! transport-agnostic: it exposes a dispatch entrypoint rather than an HTTP
//! router, and must not depend on `nimbus-server` or `axum`. `nimbus-server`
//! mounts the dispatch on its own `POST /` route (see
//! `docs/plans/dynamodb-adapter-plan.md`).
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
pub mod key;
pub mod tenant;
pub mod wire;

pub use error::map_core_error;
pub use tenant::{AccessKeyRegistry, AuthMode, KeyBinding, ensure_tenant, tenant_context};

pub use attribute_value::{
    attribute_value_to_stored, fields_to_item, item_to_fields, item_to_stored,
    stored_to_attribute_value, stored_to_item, validate_item,
};
pub use commands::ttl::sweep_all_tenants;
pub use config::DynamoDbConfig;
pub use dispatch::{DispatchContext, KNOWN_OPERATIONS, dispatch, is_known_operation};
pub use key::{
    decode_key, encode_key, is_reserved_attribute_name, sortable_key, validate_attribute_names,
};
pub use wire::{WireResponse, render_error, render_success};
