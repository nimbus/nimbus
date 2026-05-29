//! Server-side composition shim for the DynamoDB adapter.
//!
//! Owns the listener bind/spawn/shutdown and the `POST /` route; all DynamoDB
//! protocol logic (X-Amz-Target dispatch, AttributeValue codec, expression
//! bridging, SigV4) lives in `nimbus-dynamodb`. `DynamoDbConfig` is re-exported
//! from the adapter crate, which owns its own config type.

pub mod listener;

pub use nimbus_dynamodb::DynamoDbConfig;
