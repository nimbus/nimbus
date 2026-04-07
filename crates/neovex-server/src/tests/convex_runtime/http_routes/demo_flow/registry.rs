use super::*;

use super::bundle::http_demo_runtime_bundle_source;
use super::manifest::{http_demo_functions_with_runtime_delay, http_demo_routes, http_demo_schema};

pub(super) fn http_demo_registry(runtime_schedule_delay_ms: u64) -> ConvexRegistry {
    let functions = http_demo_functions_with_runtime_delay(runtime_schedule_delay_ms);
    let routes = http_demo_routes();
    let bundle = http_demo_runtime_bundle_source(&functions, &routes);
    convex_registry_with_routes_and_bundle_and_auth_and_schema(
        functions,
        routes,
        Some(&bundle),
        None,
        Some(http_demo_schema()),
    )
}
