use super::*;

impl ConvexRegistry {
    pub(in crate::adapters::convex) fn resolve_http_route(
        &self,
        method: &Method,
        request_path: &str,
    ) -> Option<&ConvexHttpRouteDefinition> {
        self.resolve_http_route_for_method(method_name(method), request_path)
            .or_else(|| {
                if *method == Method::HEAD {
                    self.resolve_http_route_for_method("GET", request_path)
                } else {
                    None
                }
            })
    }

    pub(in crate::adapters::convex) fn resolve_http_route_for_method(
        &self,
        method: &str,
        request_path: &str,
    ) -> Option<&ConvexHttpRouteDefinition> {
        self.http_routes
            .iter()
            .find(|route| {
                route.method.matches(method) && route.path.as_deref() == Some(request_path)
            })
            .or_else(|| {
                self.http_routes
                    .iter()
                    .filter(|route| {
                        route.method.matches(method)
                            && route
                                .path_prefix
                                .as_deref()
                                .is_some_and(|prefix| request_path.starts_with(prefix))
                    })
                    .max_by_key(|route| route.path_prefix.as_ref().map_or(0, String::len))
            })
    }

    pub(in crate::adapters::convex) fn has_http_route_for_path(&self, request_path: &str) -> bool {
        self.http_routes.iter().any(|route| {
            route.path.as_deref() == Some(request_path)
                || route
                    .path_prefix
                    .as_deref()
                    .is_some_and(|prefix| request_path.starts_with(prefix))
        })
    }
}

impl ConvexHttpMethod {
    pub(in crate::adapters::convex) fn matches(self, method: &str) -> bool {
        matches!(
            (self, method),
            (Self::Get, "GET")
                | (Self::Post, "POST")
                | (Self::Put, "PUT")
                | (Self::Patch, "PATCH")
                | (Self::Delete, "DELETE")
                | (Self::Options, "OPTIONS")
                | (Self::Head, "HEAD")
        )
    }
}

pub(in crate::adapters::convex) fn validate_runtime_http_route(
    request: &InvocationRequest,
    route: &ConvexHttpRouteDefinition,
) -> std::result::Result<(), NeovexRuntimeError> {
    if request.kind != InvocationKind::Action {
        return Err(NeovexRuntimeError::Contract(format!(
            "runtime http route {} expected action invocation, received {:?}",
            route.name.as_deref().unwrap_or("<unnamed>"),
            request.kind
        )));
    }

    let route_name = route.name.as_deref().ok_or_else(|| {
        NeovexRuntimeError::Contract("runtime http route is missing a route name".to_string())
    })?;
    if request.function_name != route_name {
        return Err(NeovexRuntimeError::Contract(format!(
            "runtime http route mismatch: request was {}, route was {}",
            request.function_name, route_name
        )));
    }

    Ok(())
}
