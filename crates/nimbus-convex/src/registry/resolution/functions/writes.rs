use super::*;

impl ConvexRegistry {
    pub fn resolve_mutation(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableMutation, Error> {
        self.resolve_mutation_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub fn resolve_mutation_for_visibility(
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

    pub fn resolve_scheduled_mutation_for_visibility(
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
        let mutation: Mutation = serde_json::from_value(resolved).map_err(|error| {
            Error::InvalidInput(format!(
                "convex function {name} resolved to invalid mutation: {error}"
            ))
        })?;
        resolve_scheduled_mutation_document_ids(mutation)
    }

    pub fn resolve_action(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<ConvexExecutableAction, Error> {
        self.resolve_action_for_visibility(name, args, ConvexFunctionVisibility::Public)
    }

    pub fn resolve_action_for_visibility(
        &self,
        name: &str,
        args: &Value,
        required_visibility: ConvexFunctionVisibility,
    ) -> Result<ConvexExecutableAction, Error> {
        self.resolve_typed(name, args, ConvexFunctionKind::Action, required_visibility)
    }
}

// A v.id("table")-typed argument is always client-facing table-scoped
// ("table:rawId") — the same convention host_bridge::db_ops applies via
// resolve_convex_document_id before ctx.db.get/patch/delete touch storage.
// A plan-compiled scheduled mutation never passes through that host bridge:
// resolve_template substitutes the raw arg value directly into the Mutation
// it hands to the engine, which stores and looks documents up by their bare,
// unscoped DocumentId. Without this step, an id threaded through
// ctx.scheduler.runAfter into an Update/Delete/Insert-with-id target
// resolves to a DocumentId that still carries its table prefix, which never
// matches a stored document — the scheduled job then fails silently with
// "document not found" in the server log, with no error surfaced to the
// caller that scheduled it.
fn resolve_scheduled_mutation_document_ids(mutation: Mutation) -> Result<Mutation, Error> {
    Ok(match mutation {
        Mutation::Insert {
            table,
            id: Some(id),
            fields,
        } => Mutation::Insert {
            id: Some(resolve_convex_document_id(&table, id)?.into_document_id()),
            table,
            fields,
        },
        Mutation::Update { table, id, patch } => Mutation::Update {
            id: resolve_convex_document_id(&table, id)?.into_document_id(),
            table,
            patch,
        },
        Mutation::Delete { table, id } => Mutation::Delete {
            id: resolve_convex_document_id(&table, id)?.into_document_id(),
            table,
        },
        other => other,
    })
}

#[cfg(all(test, not(feature = "bun-jsc-linked-adapter")))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    /// Builds a registry with a single schedulable mutation whose plan is the
    /// given template, so `resolve_scheduled_mutation_for_visibility` can be
    /// exercised the same way a fired `ctx.scheduler.runAfter` timer would.
    fn registry_with_scheduled_mutation(name: &str, plan: Value) -> ConvexRegistry {
        let tempdir = tempdir().expect("convex manifest tempdir should build");
        let convex_dir = tempdir.path().join(".nimbus").join("convex");
        fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
        fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec_pretty(&json!({
                "functions": [
                    {
                        "name": name,
                        "kind": "mutation",
                        "visibility": "internal",
                        "schedulable": true,
                        "plan": plan,
                        "runtime_handler": null
                    }
                ]
            }))
            .expect("convex manifest json should serialize"),
        )
        .expect("convex manifest should write");
        fs::write(
            convex_dir.join("http_routes.json"),
            serde_json::to_vec_pretty(&json!({ "routes": [] }))
                .expect("convex http route manifest should serialize"),
        )
        .expect("convex http route manifest should write");

        ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load")
    }

    #[test]
    fn resolve_scheduled_mutation_unscopes_table_prefixed_update_id() {
        let registry = registry_with_scheduled_mutation(
            "worker:processJob",
            json!({
                "type": "update",
                "table": "jobs",
                "id": { "$arg": "jobId" },
                "patch": { "status": "done" }
            }),
        );

        // A client-facing id is always table-scoped ("table:rawId"), exactly
        // what a v.id("jobs")-typed scheduler argument carries.
        let mutation = registry
            .resolve_scheduled_mutation_for_visibility(
                "worker:processJob",
                &json!({ "jobId": "jobs:abc123" }),
                ConvexFunctionVisibility::Internal,
            )
            .expect("scheduled update mutation should resolve");

        match mutation {
            Mutation::Update { table, id, .. } => {
                assert_eq!(table.as_str(), "jobs");
                assert_eq!(
                    id.as_str(),
                    "abc123",
                    "scheduled update must unscope the table-prefixed id before it \
                     reaches storage, the same way host_bridge::db_ops does for a \
                     direct ctx.db.patch call"
                );
            }
            other => panic!("expected Mutation::Update, got {other:?}"),
        }
    }

    #[test]
    fn resolve_scheduled_mutation_unscopes_table_prefixed_delete_id() {
        let registry = registry_with_scheduled_mutation(
            "worker:cleanupJob",
            json!({
                "type": "delete",
                "table": "jobs",
                "id": { "$arg": "jobId" }
            }),
        );

        let mutation = registry
            .resolve_scheduled_mutation_for_visibility(
                "worker:cleanupJob",
                &json!({ "jobId": "jobs:abc123" }),
                ConvexFunctionVisibility::Internal,
            )
            .expect("scheduled delete mutation should resolve");

        match mutation {
            Mutation::Delete { table, id } => {
                assert_eq!(table.as_str(), "jobs");
                assert_eq!(id.as_str(), "abc123");
            }
            other => panic!("expected Mutation::Delete, got {other:?}"),
        }
    }

    #[test]
    fn resolve_scheduled_mutation_unscopes_table_prefixed_insert_id() {
        let registry = registry_with_scheduled_mutation(
            "worker:insertJob",
            json!({
                "type": "insert",
                "table": "jobs",
                "id": { "$arg": "jobId" },
                "fields": { "label": "seeded" }
            }),
        );

        let mutation = registry
            .resolve_scheduled_mutation_for_visibility(
                "worker:insertJob",
                &json!({ "jobId": "jobs:abc123" }),
                ConvexFunctionVisibility::Internal,
            )
            .expect("scheduled insert-with-id mutation should resolve");

        match mutation {
            Mutation::Insert { table, id, .. } => {
                assert_eq!(table.as_str(), "jobs");
                assert_eq!(id.expect("id should be preserved").as_str(), "abc123");
            }
            other => panic!("expected Mutation::Insert, got {other:?}"),
        }
    }

    #[test]
    fn resolve_scheduled_mutation_leaves_insert_without_id_untouched() {
        let registry = registry_with_scheduled_mutation(
            "worker:createJob",
            json!({
                "type": "insert",
                "table": "jobs",
                "fields": { "label": { "$arg": "label" } }
            }),
        );

        let mutation = registry
            .resolve_scheduled_mutation_for_visibility(
                "worker:createJob",
                &json!({ "label": "fetch report" }),
                ConvexFunctionVisibility::Internal,
            )
            .expect("scheduled insert-without-id mutation should resolve");

        match mutation {
            Mutation::Insert { table, id, fields } => {
                assert_eq!(table.as_str(), "jobs");
                assert!(
                    id.is_none(),
                    "an omitted id must stay omitted, not fabricated"
                );
                assert_eq!(
                    fields.get("label").and_then(Value::as_str),
                    Some("fetch report")
                );
            }
            other => panic!("expected Mutation::Insert, got {other:?}"),
        }
    }
}
