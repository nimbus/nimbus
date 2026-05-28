use super::*;

impl ConvexFunctionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::PaginatedQuery => "paginated_query",
            Self::Mutation => "mutation",
            Self::Action => "action",
        }
    }
}

impl ConvexFunctionVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
        }
    }
}
