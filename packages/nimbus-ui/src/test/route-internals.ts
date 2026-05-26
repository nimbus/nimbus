import type { ComponentType } from "react";

type RouteRecord = Record<PropertyKey, unknown>;
type RouteFunction = (...args: never[]) => unknown;

function expectRouteRecord(route: unknown): RouteRecord {
  if (
    (typeof route !== "object" && typeof route !== "function") ||
    route === null
  ) {
    throw new Error(
      "Expected the mocked route to expose a route config object.",
    );
  }
  return route as RouteRecord;
}

function expectRouteFunction(route: unknown, key: string): RouteFunction {
  const value = expectRouteRecord(route)[key];
  if (typeof value !== "function") {
    throw new Error(`Expected mocked route.${key} to be a function.`);
  }
  return value as RouteFunction;
}

export function routeLoader<TArgs, TResult>(
  route: unknown,
): (args: TArgs) => Promise<TResult> {
  return expectRouteFunction(route, "loader") as (
    args: TArgs,
  ) => Promise<TResult>;
}

export function routeLoaderDeps<TResult>(route: unknown): () => TResult {
  return expectRouteFunction(route, "loaderDeps") as () => TResult;
}

export function routeComponent<TProps extends object = Record<string, never>>(
  route: unknown,
): ComponentType<TProps> {
  return expectRouteFunction(route, "component") as ComponentType<TProps>;
}
