use super::*;

pub(super) fn runtime_request_drop_registry(definitions: serde_json::Value) -> ConvexRegistry {
    let bundle_source =
        runtime_request_drop_bundle_source(&definitions).expect("runtime bundle should serialize");
    convex_registry_with_routes_and_bundle(definitions, json!([]), Some(&bundle_source))
}

pub(super) fn runtime_request_drop_bundle_source(
    definitions: &serde_json::Value,
) -> Result<String, serde_json::Error> {
    let definitions = serde_json::to_string_pretty(definitions)?;
    Ok(format!(
        r#"
const definitions = new Map(
  {definitions}.map((definition) => [definition.name, definition]),
);

function compileRuntimeHandler(definition) {{
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__nimbusInvoke = async function(request) {{
  const definition = definitions.get(request.function_name);
  if (!definition) {{
    return {{
      status: "error",
      error: {{ kind: "internal", message: `missing definition for ${{request.function_name}}` }},
    }};
  }}

  try {{
    const value = await handlers.get(request.function_name)(
      globalThis.__nimbusCreateContext(),
      request.args ?? {{}},
      request,
    );
    return {{ status: "ok", value }};
  }} catch (error) {{
    if (error && typeof error === "object" && "nimbusHostError" in error) {{
      return {{ status: "error", error: error.nimbusHostError }};
    }}
    throw error;
  }}
}};

export {{}};
"#
    ))
}
