// Convex compatibility surface for the shared client/function-reference shapes.
//
// The canonical definitions live in `@nimbus/nimbus/internal/shared`. This module
// re-exports them and aliases the Nimbus reference/page types under their
// Convex-branded names, so the Convex-facing API keeps its historical
// identifiers without forking the implementation. Keep this a pure re-export:
// add new shared shapes to the Nimbus module, never here.
import type {
  ActionReference,
  FunctionReference,
  MutationReference,
  Page,
  PaginatedQueryReference,
  QueryReference,
} from "@nimbus/nimbus/internal/shared";

export type {
  ActionShape,
  FilterOp,
  FunctionVisibility,
  InferArgs,
  InferResult,
  JsonValue,
  MutationShape,
  OrderDirection,
  PaginatedQueryShape,
  QueryShape,
} from "@nimbus/nimbus/internal/shared";

export {
  defineAction,
  defineMutation,
  definePaginatedQuery,
  defineQuery,
  makeActionReference,
  makeMutationReference,
  makePaginatedQueryReference,
  makeQueryReference,
} from "@nimbus/nimbus/internal/shared";

export type ConvexQueryReference<Args, Result> = QueryReference<Args, Result>;
export type ConvexPaginatedQueryReference<Args, Item> = PaginatedQueryReference<
  Args,
  Item
>;
export type ConvexMutationReference<Args, Result> = MutationReference<Args, Result>;
export type ConvexActionReference<Args, Result> = ActionReference<Args, Result>;
export type ConvexFunctionReference<Args, Result> = FunctionReference<Args, Result>;
export type ConvexPage<T> = Page<T>;
