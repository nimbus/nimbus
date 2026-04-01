use super::*;

impl ConvexRegistry {
    pub(in crate::adapters::convex) fn resolve_typed<T>(
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
}
