use super::*;

impl ConvexRegistry {
    pub fn resolve_typed<T>(
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

    /// The visibility half of `resolve_typed`, for runtime-backed named
    /// invocations that have no plan to resolve: client-origin traffic must
    /// enforce `Public` against the registry before any runtime bundle is
    /// invoked — the generated bundle only checks the reference tree of
    /// same-isolate nested calls, never client visibility.
    pub fn ensure_function_visibility(
        &self,
        name: &str,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<(), Error> {
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
        Ok(())
    }
}
