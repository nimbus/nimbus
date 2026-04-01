use super::*;

impl ConvexRegistry {
    pub(in crate::adapters::convex) fn resolve_query(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableQuery, Error> {
        self.resolve_query_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(in crate::adapters::convex) fn resolve_query_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<ConvexExecutableQuery, Error> {
        self.resolve_typed(name, args, ConvexFunctionKind::Query, required_visibility)
    }

    pub(in crate::adapters::convex) fn resolve_subscription_query(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableQuery, Error> {
        self.resolve_subscription_query_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub(in crate::adapters::convex) fn resolve_subscription_query_for_visibility(
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

    pub(in crate::adapters::convex) fn resolve_paginated_query_for_visibility(
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
}
