use super::auth::{ConvexAuthVerifier, read_auth_config};
use super::*;
use axum::http::HeaderMap;
use neovex_runtime::InvocationAuth;

impl ConvexRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_app_dir(app_dir: impl AsRef<Path>) -> Result<Self, Error> {
        let convex_dir = app_dir.as_ref().join(".neovex").join("convex");
        Self::from_manifest_paths(
            convex_dir.join("functions.json"),
            Some(convex_dir.join("http_routes.json")),
        )
    }

    pub fn from_manifest_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::from_manifest_paths(path, None::<&Path>)
    }

    pub fn from_manifest_paths(
        functions_path: impl AsRef<Path>,
        http_routes_path: Option<impl AsRef<Path>>,
    ) -> Result<Self, Error> {
        let path = functions_path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to read Convex manifest {}: {error}",
                path.display()
            ))
        })?;
        let manifest: ConvexManifest = serde_json::from_str(&contents).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to parse Convex manifest {}: {error}",
                path.display()
            ))
        })?;

        let functions = manifest
            .functions
            .into_iter()
            .map(|function| (function.name.clone(), function))
            .collect();
        let http_routes = match http_routes_path {
            Some(path) => read_http_route_manifest(path.as_ref())?,
            None => Vec::new(),
        };
        let runtime_bundle = path
            .parent()
            .map(|directory| directory.join("bundle.mjs"))
            .filter(|bundle_path| bundle_path.is_file())
            .map(|bundle_path| load_runtime_bundle(&bundle_path))
            .transpose()?;
        let auth_verifier = path
            .parent()
            .map(|directory| directory.join("auth.config.json"))
            .map(read_auth_config)
            .transpose()?
            .map(ConvexAuthVerifier::from_config)
            .map(Arc::new)
            .unwrap_or_else(|| Arc::new(ConvexAuthVerifier::empty()));

        let runtime_policy = Arc::new(RuntimePolicy::default());
        let runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy.clone()));
        let runtime_host_executor = Arc::new(RuntimeHostExecutor::new(runtime_policy.clone()));
        Ok(Self {
            functions,
            http_routes,
            runtime_bundle,
            auth_verifier,
            runtime_policy,
            runtime_executor,
            runtime_host_executor,
        })
    }

    pub fn with_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        let policy = Arc::new(RuntimePolicy::new(limits));
        self.runtime_policy = policy.clone();
        self.runtime_executor = Arc::new(RuntimeExecutor::new(policy.clone()));
        self.runtime_host_executor = Arc::new(RuntimeHostExecutor::new(policy));
        self
    }

    pub(super) fn runtime_bundle(&self) -> Option<&RuntimeBundle> {
        self.runtime_bundle.as_ref()
    }

    pub(super) async fn verify_authorization_header(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<InvocationAuth>, AppError> {
        self.auth_verifier
            .verify_authorization_header(headers)
            .await
    }

    pub(super) async fn verify_socket_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, AppError> {
        self.auth_verifier.verify_socket_token(token).await
    }

    pub(super) fn runtime_policy(&self) -> Arc<RuntimePolicy> {
        self.runtime_policy.clone()
    }

    pub(super) fn runtime_executor(&self) -> Arc<RuntimeExecutor> {
        self.runtime_executor.clone()
    }

    pub(super) fn runtime_host_executor(&self) -> Arc<RuntimeHostExecutor> {
        self.runtime_host_executor.clone()
    }

    pub fn runtime_metrics_snapshot(&self) -> neovex_runtime::RuntimeMetricsSnapshot {
        self.runtime_policy.metrics_snapshot()
    }

    pub fn runtime_limits(&self) -> RuntimeLimits {
        self.runtime_policy.limits().clone()
    }

    pub(super) fn runtime_subscription_kind(
        &self,
        name: &str,
        required_visibility: ConvexFunctionVisibility,
    ) -> Option<ConvexFunctionKind> {
        let definition = self.functions.get(name)?;
        if self.runtime_bundle.is_none()
            || definition.visibility != required_visibility
            || definition.runtime_handler.is_none()
            || !definition.plan.is_null()
        {
            return None;
        }
        match definition.kind {
            ConvexFunctionKind::Query | ConvexFunctionKind::PaginatedQuery => Some(definition.kind),
            ConvexFunctionKind::Mutation | ConvexFunctionKind::Action => None,
        }
    }

    pub(super) fn resolve_query(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableQuery, Error> {
        self.resolve_query_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(super) fn resolve_query_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<ConvexExecutableQuery, Error> {
        self.resolve_typed(name, args, ConvexFunctionKind::Query, required_visibility)
    }

    pub(super) fn resolve_subscription_query(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableQuery, Error> {
        self.resolve_subscription_query_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(super) fn resolve_subscription_query_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<ConvexExecutableQuery, Error> {
        let definition = self
            .functions
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.visibility != required_visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                required_visibility.as_str()
            )));
        }

        match definition.kind {
            ConvexFunctionKind::Query => {
                self.resolve_query_for_visibility(name, args, required_visibility)
            }
            ConvexFunctionKind::PaginatedQuery => {
                Ok(ConvexExecutableQuery::Query(self.resolve_typed(
                    name,
                    args,
                    ConvexFunctionKind::PaginatedQuery,
                    required_visibility,
                )?))
            }
            _ => Err(Error::InvalidInput(format!(
                "convex function {name} is a {}, not subscribable query",
                definition.kind.as_str()
            ))),
        }
    }

    pub fn resolve_paginated_query(
        &self,
        name: &str,
        args: &Value,
        page_size: usize,
        cursor: Option<String>,
    ) -> Result<PaginatedQuery, Error> {
        self.resolve_paginated_query_for_visibility(
            name,
            args,
            page_size,
            cursor,
            ConvexFunctionVisibility::Public,
        )
    }

    pub(super) fn resolve_paginated_query_for_visibility(
        &self,
        name: &str,
        args: &Value,
        page_size: usize,
        cursor: Option<String>,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<PaginatedQuery, Error> {
        let query = self.resolve_typed(
            name,
            args,
            ConvexFunctionKind::PaginatedQuery,
            required_visibility,
        )?;
        Ok(PaginatedQuery {
            query,
            page_size,
            after: cursor.map(Cursor),
        })
    }

    pub(super) fn resolve_mutation(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableMutation, Error> {
        self.resolve_mutation_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(super) fn resolve_mutation_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<ConvexExecutableMutation, Error> {
        self.resolve_typed(
            name,
            args,
            ConvexFunctionKind::Mutation,
            required_visibility,
        )
    }

    pub fn resolve_scheduled_mutation(&self, name: &str, args: &Value) -> Result<Mutation, Error> {
        self.resolve_scheduled_mutation_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(super) fn resolve_scheduled_mutation_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<Mutation, Error> {
        let definition = self
            .functions
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.kind != ConvexFunctionKind::Mutation {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is a {}, not mutation",
                definition.kind.as_str()
            )));
        }
        if definition.visibility != required_visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                required_visibility.as_str()
            )));
        }
        if !definition.schedulable {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is not schedulable"
            )));
        }

        let resolved = resolve_template(&definition.plan, args)?;
        serde_json::from_value(resolved).map_err(|error| {
            Error::InvalidInput(format!(
                "convex function {name} resolved to invalid mutation: {error}"
            ))
        })
    }

    pub(super) fn resolve_action(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableAction, Error> {
        self.resolve_action_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(super) fn resolve_action_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<ConvexExecutableAction, Error> {
        self.resolve_typed(name, args, ConvexFunctionKind::Action, required_visibility)
    }

    pub(super) fn resolve_typed<T>(
        &self,
        name: &str,
        args: &Value,
        expected_kind: ConvexFunctionKind,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let definition = self
            .functions
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.kind != expected_kind {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is a {}, not a {}",
                definition.kind.as_str(),
                expected_kind.as_str()
            )));
        }
        if definition.visibility != required_visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                required_visibility.as_str()
            )));
        }

        let resolved = resolve_template(&definition.plan, args)?;
        serde_json::from_value(resolved).map_err(|error| {
            Error::InvalidInput(format!(
                "convex function {name} resolved to invalid {}: {error}",
                expected_kind.as_str()
            ))
        })
    }

    pub(super) fn resolve_http_route(
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

    pub(super) fn resolve_http_route_for_method(
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

    pub(super) fn has_http_route_for_path(&self, request_path: &str) -> bool {
        self.http_routes.iter().any(|route| {
            route.path.as_deref() == Some(request_path)
                || route
                    .path_prefix
                    .as_deref()
                    .is_some_and(|prefix| request_path.starts_with(prefix))
        })
    }
}

impl ConvexFunctionKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::PaginatedQuery => "paginated_query",
            Self::Mutation => "mutation",
            Self::Action => "action",
        }
    }
}

impl ConvexFunctionVisibility {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
        }
    }
}

impl ConvexHttpMethod {
    pub(super) fn matches(self, method: &str) -> bool {
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
pub(super) fn read_http_route_manifest(
    path: &Path,
) -> Result<Vec<ConvexHttpRouteDefinition>, Error> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::InvalidInput(format!(
                "failed to read convex HTTP route manifest {}: {error}",
                path.display()
            )));
        }
    };
    let manifest: ConvexHttpRouteManifest = serde_json::from_str(&contents).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse convex HTTP route manifest {}: {error}",
            path.display()
        ))
    })?;
    Ok(manifest.routes)
}

fn load_runtime_bundle(bundle_path: &Path) -> Result<RuntimeBundle, Error> {
    let hash_path = bundle_path.with_extension("sha256");
    let expected_sha256 = std::fs::read_to_string(&hash_path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read convex runtime bundle hash {}: {error}",
            hash_path.display()
        ))
    })?;
    RuntimeBundle::with_expected_sha256(bundle_path, expected_sha256).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to load convex runtime bundle {}: {error}",
            bundle_path.display()
        ))
    })
}

pub(super) fn validate_runtime_definition(
    request: &InvocationRequest,
    definition: &ConvexFunctionDefinition,
) -> std::result::Result<(), NeovexRuntimeError> {
    if request.function_name != definition.name {
        return Err(NeovexRuntimeError::Contract(format!(
            "runtime bundle definition mismatch: request was {}, bundle provided {}",
            request.function_name, definition.name
        )));
    }

    let expected_kind = match request.kind {
        InvocationKind::Query => ConvexFunctionKind::Query,
        InvocationKind::PaginatedQuery => ConvexFunctionKind::PaginatedQuery,
        InvocationKind::Mutation => ConvexFunctionKind::Mutation,
        InvocationKind::Action => ConvexFunctionKind::Action,
    };
    if definition.kind != expected_kind {
        return Err(NeovexRuntimeError::Contract(format!(
            "runtime bundle definition {} had kind {}, expected {}",
            definition.name,
            definition.kind.as_str(),
            expected_kind.as_str()
        )));
    }

    Ok(())
}

pub(super) fn validate_runtime_http_route(
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
