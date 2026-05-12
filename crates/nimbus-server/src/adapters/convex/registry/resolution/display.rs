use super::*;

impl ConvexFunctionKind {
    pub(in crate::adapters::convex) fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::PaginatedQuery => "paginated_query",
            Self::Mutation => "mutation",
            Self::Action => "action",
        }
    }
}

impl ConvexFunctionVisibility {
    pub(in crate::adapters::convex) fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
        }
    }
}
