use std::collections::BTreeSet;

use nimbus_code_index::analyze_module;
use serde::Serialize;

use super::*;
use nimbus_system::{DiskSourcePackageStore, read_active_source_package_modules_async};

/// A function node in the deployment call graph (`module:export`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct GraphNode {
    id: String,
    module: String,
    name: String,
}

/// A directed call edge between two function nodes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct GraphEdge {
    from: String,
    to: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CallGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

/// The active deployment's function call graph (oxc, FSV7):
/// `GET /api/console/graph`. Nodes are functions; edges are `api.*`/`internal.*`
/// calls between deployment functions.
pub(crate) async fn call_graph(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CallGraph>, AppError> {
    let store = DiskSourcePackageStore::new(state.engine.data_dir().join("source-packages"));
    let modules = read_active_source_package_modules_async(&state.engine, &store).await?;
    Ok(Json(build_call_graph(&modules)))
}

/// Build the call graph from every module: each export is a node; each
/// `api.*`/`internal.*` reference becomes an edge from its enclosing export to
/// the target. Edges to unknown targets (external/`_generated`) are dropped.
fn build_call_graph(modules: &[(String, String)]) -> CallGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut node_ids: BTreeSet<String> = BTreeSet::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    for (module, source) in modules {
        let analysis = analyze_module(source);
        let mut exports = analysis.exports;
        exports.sort_by_key(|export| export.line);
        for export in &exports {
            let id = format!("{module}:{}", export.name);
            if node_ids.insert(id.clone()) {
                nodes.push(GraphNode {
                    id,
                    module: module.clone(),
                    name: export.name.clone(),
                });
            }
        }
        for reference in &analysis.references {
            let Some(caller) = exports
                .iter()
                .rev()
                .find(|export| export.line <= reference.line)
            else {
                continue;
            };
            edges.push(GraphEdge {
                from: format!("{module}:{}", caller.name),
                to: reference.target.clone(),
            });
        }
    }

    edges.retain(|edge| node_ids.contains(&edge.from) && node_ids.contains(&edge.to));
    edges.sort();
    edges.dedup();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    CallGraph { nodes, edges }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_call_graph_links_callers_to_callees_and_drops_externals() {
        let modules = vec![
            (
                "messages".to_owned(),
                "export const list = query({});\nexport const send = mutation({});\n".to_owned(),
            ),
            (
                "notifications".to_owned(),
                concat!(
                    "import { api } from \"./_generated/api\";\n",
                    "import { mutation } from \"./_generated/server\";\n",
                    "export const announce = mutation({\n",
                    "  handler: async (ctx) => {\n",
                    "    await ctx.runQuery(api.messages.list, {});\n",
                    "    await ctx.runMutation(api.missing.nope, {});\n",
                    "  },\n",
                    "});\n",
                )
                .to_owned(),
            ),
        ];
        let graph = build_call_graph(&modules);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"messages:list"));
        assert!(ids.contains(&"notifications:announce"));
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.from == "notifications:announce" && e.to == "messages:list"),
            "expected announce -> messages:list, got {:?}",
            graph.edges
        );
        // The edge to api.missing.nope (no such node) is dropped.
        assert!(
            graph
                .edges
                .iter()
                .all(|e| ids.contains(&e.from.as_str()) && ids.contains(&e.to.as_str())),
            "external edges must be dropped"
        );
    }
}
