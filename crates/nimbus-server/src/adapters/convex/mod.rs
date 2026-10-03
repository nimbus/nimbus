use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
pub use nimbus_convex::ConvexRegistry;
pub(crate) use nimbus_convex::*;
pub use nimbus_convex::{ConvexSiloAuthRegistry, ConvexTenancyConfig, SiloTeamRegistry, TeamId};
use nimbus_core::{InvocationAuth, Query, TenantId};
use nimbus_engine::SubscriptionUpdate;
use nimbus_runtime::HostCallCancellation;
use serde_json::Value;
use tokio::sync::mpsc;

pub(in crate::adapters::convex) mod handlers;
mod http_actions;
mod socket_auth;
mod subscriptions;

pub(in crate::adapters::convex) use self::http_actions::ConvexHttpRouteRequest;

pub(crate) use self::handlers::{
    action, cancel_scheduled_job, http_route, http_route_root, mutation, paginated_query, query,
    schedule_after, schedule_at, ws,
};

use crate::protocol::ServerMessage;
use crate::state::{AppError, AppState};
