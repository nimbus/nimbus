use super::*;

impl ConvexRegistry {
    pub(in crate::adapters::convex) fn resolve_mutation(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableMutation, Error> {
        self.resolve_mutation_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(in crate::adapters::convex) fn resolve_mutation_for_visibility(
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

    pub(in crate::adapters::convex) fn resolve_scheduled_mutation_for_visibility(
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

    pub(in crate::adapters::convex) fn resolve_action(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableAction, Error> {
        self.resolve_action_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(in crate::adapters::convex) fn resolve_action_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<ConvexExecutableAction, Error> {
        self.resolve_typed(name, args, ConvexFunctionKind::Action, required_visibility)
    }
}
