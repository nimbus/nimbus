use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn should_use_nested_runtime(
        &self,
        kind: InvocationKind,
        name: &str,
        visibility: ConvexFunctionVisibility,
    ) -> Result<bool, Error> {
        let Some(bundle) = self.registry().runtime_bundle() else {
            return Ok(false);
        };
        let _ = bundle;
        let definition = self
            .registry()
            .function_definition(name)
            .ok_or_else(|| Error::InvalidInput(format!("convex function not found: {name}")))?;
        if definition.visibility != visibility {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is {}, not {}",
                definition.visibility.as_str(),
                visibility.as_str()
            )));
        }
        let expected_kind = match kind {
            InvocationKind::Query => ConvexFunctionKind::Query,
            InvocationKind::PaginatedQuery => ConvexFunctionKind::PaginatedQuery,
            InvocationKind::Mutation => ConvexFunctionKind::Mutation,
            InvocationKind::Action => ConvexFunctionKind::Action,
            InvocationKind::CloudflareWorkerFetch => {
                return Err(Error::InvalidInput(
                    "convex nested runtime calls do not support Cloudflare Worker fetch invocations"
                        .to_string(),
                ));
            }
        };
        if definition.kind != expected_kind {
            return Err(Error::InvalidInput(format!(
                "convex function {name} is a {}, not a {}",
                definition.kind.as_str(),
                expected_kind.as_str()
            )));
        }
        Ok(definition.runtime_handler.is_some() && definition.plan.is_null())
    }
}
