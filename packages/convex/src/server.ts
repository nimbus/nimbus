import type {
  AuthConfig as NeovexAuthConfig,
  AuthProvider as NeovexAuthProvider,
  Cursor as NeovexCursor,
  DefaultFunctionArgs as NeovexDefaultFunctionArgs,
  FilterExpressionBuilder as NeovexFilterExpressionBuilder,
  FilterField as NeovexFilterField,
  GenericDatabaseReader as NeovexGenericDatabaseReader,
  GenericDatabaseWriter as NeovexGenericDatabaseWriter,
  HttpRouteMethod as NeovexHttpRouteMethod,
  HttpRouteSpec as NeovexHttpRouteSpec,
  HttpRouter as NeovexHttpRouter,
  IndexRangeBuilder as NeovexIndexRangeBuilder,
  PaginationOptions as NeovexPaginationOptions,
  PaginationResult as NeovexPaginationResult,
  PaginationStatus as NeovexPaginationStatus,
  QueryBuilder as NeovexQueryBuilder,
  QueryOrder as NeovexQueryOrder,
  SchemaDefinition as NeovexSchemaDefinition,
  TableDefinition as NeovexTableDefinition,
  UserIdentity as NeovexUserIdentity,
  UserIdentityAttributes as NeovexUserIdentityAttributes,
} from "neovex/server";
import {
  action as neovexAction,
  defineSchema as neovexDefineSchema,
  defineTable as neovexDefineTable,
  httpAction as neovexHttpAction,
  httpRouter as neovexHttpRouter,
  internalAction as neovexInternalAction,
  internalMutation as neovexInternalMutation,
  internalPaginatedQuery as neovexInternalPaginatedQuery,
  internalQuery as neovexInternalQuery,
  mutation as neovexMutation,
  paginatedQuery as neovexPaginatedQuery,
  paginationOptsValidator as neovexPaginationOptsValidator,
  paginationResultValidator as neovexPaginationResultValidator,
  query as neovexQuery,
} from "neovex/server";

import type {
  ConvexActionReference,
  ConvexMutationReference,
  ConvexPaginatedQueryReference,
  ConvexQueryReference,
  FunctionVisibility,
} from "./internal/shared.ts";
import type { Infer, Validator } from "./values.ts";

export type DefaultFunctionArgs = NeovexDefaultFunctionArgs;
export type AuthProvider = NeovexAuthProvider;
export type AuthConfig = NeovexAuthConfig;
export type UserIdentity = NeovexUserIdentity;
export type UserIdentityAttributes = NeovexUserIdentityAttributes;

export interface Auth {
  getUserIdentity(): Promise<UserIdentity | null>;
}

type ArgValidators = Record<string, Validator<unknown>>;

type InferDefinitionArgs<Args> = Args extends ArgValidators
  ? { [Key in keyof Args]: Infer<Args[Key]> }
  : DefaultFunctionArgs;

type InferDefinitionReturns<Returns> = Returns extends Validator<unknown>
  ? Infer<Returns>
  : unknown;

export type QueryOrder = NeovexQueryOrder;
export type IndexRangeBuilder = NeovexIndexRangeBuilder;
export type FilterField = NeovexFilterField;
export type FilterExpressionBuilder = NeovexFilterExpressionBuilder;
export type QueryBuilder = NeovexQueryBuilder;
export type GenericDatabaseReader = NeovexGenericDatabaseReader;
export type GenericDatabaseWriter = NeovexGenericDatabaseWriter;
export type Cursor = NeovexCursor;
export type PaginationStatus = NeovexPaginationStatus;
export type PaginationOptions = NeovexPaginationOptions;
export type PaginationResult<Item> = NeovexPaginationResult<Item>;

export type Scheduler = {
  runAfter<Args extends DefaultFunctionArgs>(
    delayMs: number,
    functionRef: ConvexMutationReference<Args, unknown>,
    args?: Args,
  ): Promise<string>;
  runAt<Args extends DefaultFunctionArgs>(
    timestampMs: number,
    functionRef: ConvexMutationReference<Args, unknown>,
    args?: Args,
  ): Promise<string>;
  cancel(jobId: string): Promise<void>;
};

export type GenericQueryCtx = {
  readonly db: GenericDatabaseReader;
  readonly auth: Auth;
};
export type QueryCtx = GenericQueryCtx;

export type GenericMutationCtx = {
  readonly db: GenericDatabaseWriter;
  readonly scheduler: Scheduler;
  readonly auth: Auth;
};
export type MutationCtx = GenericMutationCtx;

export type GenericActionCtx = {
  readonly scheduler: Scheduler;
  readonly auth: Auth;
  runQuery<Args extends DefaultFunctionArgs, Returns>(
    functionRef: ConvexQueryReference<Args, Returns>,
    args?: Args,
  ): Promise<Returns>;
  runMutation<Args extends DefaultFunctionArgs, Returns>(
    functionRef: ConvexMutationReference<Args, Returns>,
    args?: Args,
  ): Promise<Returns>;
  runAction<Args extends DefaultFunctionArgs, Returns>(
    functionRef: ConvexActionReference<Args, Returns>,
    args?: Args,
  ): Promise<Returns>;
};
export type ActionCtx = GenericActionCtx;

type QueryDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericQueryCtx,
    args: InferDefinitionArgs<Args>,
  ) => unknown | Promise<unknown>;
};

type PaginatedQueryDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericQueryCtx,
    args: InferDefinitionArgs<Args>,
  ) => unknown | Promise<unknown>;
};

type MutationDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericMutationCtx,
    args: InferDefinitionArgs<Args>,
  ) => unknown | Promise<unknown>;
};

type ActionDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericActionCtx,
    args: InferDefinitionArgs<Args>,
  ) => unknown | Promise<unknown>;
};

type HttpActionHandler = (
  ctx: GenericActionCtx,
  request: Request,
) => Response | Promise<Response>;

export type RegisteredQuery<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = ConvexQueryReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: QueryDefinition<any, any>["handler"];
};

export type RegisteredPaginatedQuery<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = ConvexPaginatedQueryReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: PaginatedQueryDefinition<any, any>["handler"];
};

export type RegisteredMutation<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = ConvexMutationReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: MutationDefinition<any, any>["handler"];
};

export type RegisteredAction<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = ConvexActionReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: ActionDefinition<any, any>["handler"];
};

export type PublicHttpAction = {
  readonly kind: "http_action";
  readonly visibility: "public";
  readonly handler: HttpActionHandler;
};

export type HttpRouteMethod = NeovexHttpRouteMethod;

export type HttpRouteSpec = Omit<NeovexHttpRouteSpec, "handler"> & {
  readonly handler: PublicHttpAction;
};

export type HttpRouter = Omit<NeovexHttpRouter, "route"> & {
  route(spec: HttpRouteSpec): HttpRouter;
};

export type TableDefinition<Fields extends ArgValidators> = NeovexTableDefinition<Fields>;
export type SchemaDefinition<Tables extends Record<string, TableDefinition<any>>> =
  NeovexSchemaDefinition<Tables>;

export const paginationOptsValidator =
  neovexPaginationOptsValidator as Validator<PaginationOptions>;

export function paginationResultValidator<Item>(
  itemValidator: Validator<Item>,
): Validator<PaginationResult<Item>> {
  return neovexPaginationResultValidator(
    itemValidator,
  ) as Validator<PaginationResult<Item>>;
}

export type FunctionReference<Args extends DefaultFunctionArgs, Returns> =
  ConvexQueryReference<Args, Returns>
  | ConvexMutationReference<Args, Returns>
  | ConvexActionReference<Args, Returns>
  | ConvexPaginatedQueryReference<Args, Returns>;

export function query<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: QueryDefinition<Args, Returns>,
): RegisteredQuery<"public", InferDefinitionArgs<Args>, InferDefinitionReturns<Returns>> {
  return neovexQuery(definition as unknown as Parameters<typeof neovexQuery>[0]) as RegisteredQuery<
    "public",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function internalQuery<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: QueryDefinition<Args, Returns>,
): RegisteredQuery<"internal", InferDefinitionArgs<Args>, InferDefinitionReturns<Returns>> {
  return neovexInternalQuery(
    definition as unknown as Parameters<typeof neovexInternalQuery>[0],
  ) as RegisteredQuery<
    "internal",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function paginatedQuery<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: PaginatedQueryDefinition<Args, Returns>,
): RegisteredPaginatedQuery<
  "public",
  InferDefinitionArgs<Args>,
  InferDefinitionReturns<Returns>
> {
  return neovexPaginatedQuery(
    definition as unknown as Parameters<typeof neovexPaginatedQuery>[0],
  ) as RegisteredPaginatedQuery<
    "public",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function internalPaginatedQuery<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: PaginatedQueryDefinition<Args, Returns>,
): RegisteredPaginatedQuery<
  "internal",
  InferDefinitionArgs<Args>,
  InferDefinitionReturns<Returns>
> {
  return neovexInternalPaginatedQuery(
    definition as unknown as Parameters<typeof neovexInternalPaginatedQuery>[0],
  ) as RegisteredPaginatedQuery<
    "internal",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function mutation<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: MutationDefinition<Args, Returns>,
): RegisteredMutation<"public", InferDefinitionArgs<Args>, InferDefinitionReturns<Returns>> {
  return neovexMutation(
    definition as unknown as Parameters<typeof neovexMutation>[0],
  ) as RegisteredMutation<
    "public",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function internalMutation<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: MutationDefinition<Args, Returns>,
): RegisteredMutation<"internal", InferDefinitionArgs<Args>, InferDefinitionReturns<Returns>> {
  return neovexInternalMutation(
    definition as unknown as Parameters<typeof neovexInternalMutation>[0],
  ) as RegisteredMutation<
    "internal",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function action<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: ActionDefinition<Args, Returns>,
): RegisteredAction<"public", InferDefinitionArgs<Args>, InferDefinitionReturns<Returns>> {
  return neovexAction(definition as unknown as Parameters<typeof neovexAction>[0]) as RegisteredAction<
    "public",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function internalAction<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: ActionDefinition<Args, Returns>,
): RegisteredAction<"internal", InferDefinitionArgs<Args>, InferDefinitionReturns<Returns>> {
  return neovexInternalAction(
    definition as unknown as Parameters<typeof neovexInternalAction>[0],
  ) as RegisteredAction<
    "internal",
    InferDefinitionArgs<Args>,
    InferDefinitionReturns<Returns>
  >;
}

export function httpAction(
  definition:
    | HttpActionHandler
    | {
        handler: HttpActionHandler;
      },
): PublicHttpAction {
  return neovexHttpAction(
    definition as Parameters<typeof neovexHttpAction>[0],
  ) as PublicHttpAction;
}

export function httpRouter(): HttpRouter {
  return neovexHttpRouter() as HttpRouter;
}

export function defineTable<Fields extends ArgValidators>(
  fields: Fields,
): TableDefinition<Fields> {
  return neovexDefineTable(fields) as TableDefinition<Fields>;
}

export function defineSchema<Tables extends Record<string, TableDefinition<any>>>(
  tables: Tables,
): SchemaDefinition<Tables> {
  return neovexDefineSchema(tables) as SchemaDefinition<Tables>;
}
