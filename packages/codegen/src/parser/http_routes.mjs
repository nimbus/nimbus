import path from "node:path";

import { readUtf8FileIfExists } from "../app.mjs";
import { unsupportedError } from "../errors.mjs";
import {
  extractCallExpression,
  findCallOpenParen,
  splitTopLevel,
} from "../syntax.mjs";

import { buildFunctionIndex, createCompileBindings } from "./helpers.mjs";
import { resolveHttpRouteHandler } from "./http_route_handlers.mjs";
import { createLocalHttpActionImportMap } from "./http_route_imports.mjs";
import { parseHttpRouteDefinition } from "./http_route_definition.mjs";

async function parseHttpRoutes(convexDir, schema, modules) {
  const httpPath = path.join(convexDir, "http.ts");
  const source = await readHttpRouterSource(httpPath);
  if (source === null) {
    return [];
  }

  const routerName = extractRouterName(source, httpPath);
  const compileBindings = createCompileBindings(source);
  const functionIndex = buildFunctionIndex(modules);
  const importedHttpActions = createLocalHttpActionImportMap(
    source,
    convexDir,
    httpPath,
    functionIndex,
  );

  const routes = [];
  const routePattern = new RegExp(`\\b${routerName}\\.route\\b`, "g");
  let inlineIndex = 0;
  for (const match of source.matchAll(routePattern)) {
    const callExpression = extractCallExpression(
      source,
      match.index + match[0].lastIndexOf("route"),
      httpPath,
    );
    routes.push(
      await parseHttpRouteCall(
        callExpression,
        httpPath,
        schema,
        compileBindings,
        importedHttpActions,
        inlineIndex,
      ),
    );
    inlineIndex += 1;
  }

  return routes;
}

async function readHttpRouterSource(httpPath) {
  return readUtf8FileIfExists(httpPath);
}

function extractRouterName(source, httpPath) {
  const exportMatch = /export\s+default\s+([A-Za-z_$][\w$]*)\s*;?/.exec(source);
  if (!exportMatch) {
    throw unsupportedError(httpPath, 'http.ts must use "export default router"');
  }
  const routerName = exportMatch[1];
  if (!new RegExp(`\\b(?:const|let|var)\\s+${routerName}\\s*=\\s*httpRouter\\s*\\(`).test(source)) {
    throw unsupportedError(httpPath, "http.ts must initialize its default export with httpRouter()");
  }
  return routerName;
}

async function parseHttpRouteCall(
  callExpression,
  filePath,
  schema,
  compileBindings,
  importedHttpActions,
  inlineIndex,
) {
  const openParen = findCallOpenParen(callExpression, "route".length, filePath);
  const args = splitTopLevel(callExpression.slice(openParen + 1, -1), ",", filePath);
  if (args.length !== 1) {
    throw unsupportedError(filePath, "http.route(...) arity");
  }

  const routeDefinition = parseHttpRouteDefinition(args[0].trim(), filePath);
  const handler = await resolveHttpRouteHandler(
    routeDefinition.handlerExpression,
    filePath,
    schema,
    compileBindings,
    importedHttpActions,
    inlineIndex,
  );

  const route = {
    method: routeDefinition.method,
    plan: handler.plan,
  };
  if (routeDefinition.path !== undefined) {
    route.path = routeDefinition.path;
  }
  if (routeDefinition.pathPrefix !== undefined) {
    route.path_prefix = routeDefinition.pathPrefix;
  }
  if (handler.name !== undefined) {
    route.name = handler.name;
  }
  return route;
}

export { parseHttpRoutes };
