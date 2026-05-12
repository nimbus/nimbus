use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConvexRegistryDeploySummary {
    pub(crate) functions: Vec<ConvexFunctionDeploySummary>,
    pub(crate) http_routes: Vec<ConvexHttpRouteDeploySummary>,
    pub(crate) schema_fingerprint: Option<String>,
    pub(crate) index_fingerprint: Option<String>,
    pub(crate) runtime_bundle_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConvexFunctionDeploySummary {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConvexHttpRouteDeploySummary {
    pub(crate) key: String,
    pub(crate) fingerprint: String,
}

impl ConvexRegistry {
    pub(crate) fn deploy_summary(&self) -> ConvexRegistryDeploySummary {
        let mut functions = self
            .functions
            .values()
            .map(|function| ConvexFunctionDeploySummary {
                name: function.name.clone(),
                kind: function.kind.as_str(),
                fingerprint: function.deploy_fingerprint(),
            })
            .collect::<Vec<_>>();
        functions.sort_by(|left, right| left.name.cmp(&right.name));

        let mut http_routes = self
            .http_routes
            .iter()
            .map(|route| ConvexHttpRouteDeploySummary {
                key: route.deploy_key(),
                fingerprint: route.deploy_fingerprint(),
            })
            .collect::<Vec<_>>();
        http_routes.sort_by(|left, right| left.key.cmp(&right.key));

        ConvexRegistryDeploySummary {
            functions,
            http_routes,
            schema_fingerprint: self.schema.as_ref().map(schema_deploy_fingerprint),
            index_fingerprint: self.schema.as_ref().map(index_deploy_fingerprint),
            runtime_bundle_fingerprint: self
                .runtime_bundle
                .as_ref()
                .and_then(|bundle| bundle.identity().expected_sha256())
                .map(str::to_owned),
        }
    }
}

fn schema_deploy_fingerprint(schema: &Schema) -> String {
    let mut tables = schema.tables.values().collect::<Vec<_>>();
    tables.sort_by(|left, right| left.table.cmp(&right.table));
    let canonical = tables
        .into_iter()
        .map(|table| {
            let mut fields = table.fields.clone();
            fields.sort_by(|left, right| left.name.cmp(&right.name));
            let mut indexes = table.indexes.clone();
            indexes.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.fields.cmp(&right.fields))
            });
            serde_json::json!({
                "table": table.table.as_str(),
                "fields": fields,
                "indexes": indexes,
                "access_policy": &table.access_policy,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&canonical).unwrap_or_default()
}

fn index_deploy_fingerprint(schema: &Schema) -> String {
    let mut tables = schema.tables.values().collect::<Vec<_>>();
    tables.sort_by(|left, right| left.table.cmp(&right.table));
    let canonical = tables
        .into_iter()
        .map(|table| {
            let mut indexes = table.indexes.clone();
            indexes.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.fields.cmp(&right.fields))
            });
            serde_json::json!({
                "table": table.table.as_str(),
                "indexes": indexes,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&canonical).unwrap_or_default()
}

impl ConvexFunctionDefinition {
    fn deploy_fingerprint(&self) -> String {
        serde_json::to_string(&serde_json::json!({
            "kind": self.kind.as_str(),
            "visibility": self.visibility.as_str(),
            "schedulable": self.schedulable,
            "runtime_handler": self.runtime_handler,
            "plan": self.plan,
        }))
        .unwrap_or_else(|_| self.name.clone())
    }
}

impl ConvexHttpRouteDefinition {
    fn deploy_key(&self) -> String {
        let path = self
            .path
            .as_deref()
            .or(self.path_prefix.as_deref())
            .unwrap_or("<runtime>");
        format!("{} {path}", self.method.as_str())
    }

    fn deploy_fingerprint(&self) -> String {
        serde_json::to_string(&serde_json::json!({
            "name": self.name,
            "method": self.method.as_str(),
            "path": self.path,
            "path_prefix": self.path_prefix,
            "plan": self.plan,
        }))
        .unwrap_or_else(|_| self.deploy_key())
    }
}

impl ConvexHttpMethod {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
        }
    }
}
