import type {
  AuthConfig as NimbusAuthConfig,
  AuthProvider as NimbusAuthProvider,
  Cursor as NimbusCursor,
  DefaultFunctionArgs as NimbusDefaultFunctionArgs,
  FilterExpressionBuilder as NimbusFilterExpressionBuilder,
  FilterField as NimbusFilterField,
  GenericDatabaseReader as NimbusGenericDatabaseReader,
  GenericDatabaseWriter as NimbusGenericDatabaseWriter,
  HttpRouteMethod as NimbusHttpRouteMethod,
  HttpRouteSpec as NimbusHttpRouteSpec,
  HttpRouter as NimbusHttpRouter,
  IndexRangeBuilder as NimbusIndexRangeBuilder,
  PaginationOptions as NimbusPaginationOptions,
  PaginationResult as NimbusPaginationResult,
  PaginationStatus as NimbusPaginationStatus,
  QueryBuilder as NimbusQueryBuilder,
  QueryOrder as NimbusQueryOrder,
  SchemaDefinition as NimbusSchemaDefinition,
  TableDefinition as NimbusTableDefinition,
  UserIdentity as NimbusUserIdentity,
  UserIdentityAttributes as NimbusUserIdentityAttributes,
} from "nimbus/server";
import {
  action as nimbusAction,
  defineSchema as nimbusDefineSchema,
  defineTable as nimbusDefineTable,
  httpAction as nimbusHttpAction,
  httpRouter as nimbusHttpRouter,
  internalAction as nimbusInternalAction,
  internalMutation as nimbusInternalMutation,
  internalPaginatedQuery as nimbusInternalPaginatedQuery,
  internalQuery as nimbusInternalQuery,
  mutation as nimbusMutation,
  paginatedQuery as nimbusPaginatedQuery,
  paginationOptsValidator as nimbusPaginationOptsValidator,
  paginationResultValidator as nimbusPaginationResultValidator,
  query as nimbusQuery,
} from "nimbus/server";

import type {
  ConvexActionReference,
  ConvexMutationReference,
  ConvexPaginatedQueryReference,
  ConvexQueryReference,
  FunctionVisibility,
} from "./internal/shared.ts";
import type { Infer, Validator } from "./values.ts";

export type DefaultFunctionArgs = NimbusDefaultFunctionArgs;
export type AuthProvider = NimbusAuthProvider;
export type AuthConfig = NimbusAuthConfig;
export type UserIdentity = NimbusUserIdentity;
export type UserIdentityAttributes = NimbusUserIdentityAttributes;

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

export type QueryOrder = NimbusQueryOrder;
export type IndexRangeBuilder = NimbusIndexRangeBuilder;
export type FilterField = NimbusFilterField;
export type FilterExpressionBuilder = NimbusFilterExpressionBuilder;
export type QueryBuilder = NimbusQueryBuilder;
export type GenericDatabaseReader = NimbusGenericDatabaseReader;
export type GenericDatabaseWriter = NimbusGenericDatabaseWriter;
export type Cursor = NimbusCursor;
export type PaginationStatus = NimbusPaginationStatus;
export type PaginationOptions = NimbusPaginationOptions;
export type PaginationResult<Item> = NimbusPaginationResult<Item>;

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

export type HttpRouteMethod = NimbusHttpRouteMethod;

export type HttpRouteSpec = Omit<NimbusHttpRouteSpec, "handler"> & {
  readonly handler: PublicHttpAction;
};

export type HttpRouter = Omit<NimbusHttpRouter, "route"> & {
  route(spec: HttpRouteSpec): HttpRouter;
};

export type TableDefinition<Fields extends ArgValidators> = NimbusTableDefinition<Fields>;
export type SchemaDefinition<Tables extends Record<string, TableDefinition<any>>> =
  NimbusSchemaDefinition<Tables>;

export const paginationOptsValidator =
  nimbusPaginationOptsValidator as Validator<PaginationOptions>;

export function paginationResultValidator<Item>(
  itemValidator: Validator<Item>,
): Validator<PaginationResult<Item>> {
  return nimbusPaginationResultValidator(
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
  return nimbusQuery(definition as unknown as Parameters<typeof nimbusQuery>[0]) as RegisteredQuery<
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
  return nimbusInternalQuery(
    definition as unknown as Parameters<typeof nimbusInternalQuery>[0],
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
  return nimbusPaginatedQuery(
    definition as unknown as Parameters<typeof nimbusPaginatedQuery>[0],
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
  return nimbusInternalPaginatedQuery(
    definition as unknown as Parameters<typeof nimbusInternalPaginatedQuery>[0],
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
  return nimbusMutation(
    definition as unknown as Parameters<typeof nimbusMutation>[0],
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
  return nimbusInternalMutation(
    definition as unknown as Parameters<typeof nimbusInternalMutation>[0],
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
  return nimbusAction(definition as unknown as Parameters<typeof nimbusAction>[0]) as RegisteredAction<
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
  return nimbusInternalAction(
    definition as unknown as Parameters<typeof nimbusInternalAction>[0],
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
  return nimbusHttpAction(
    definition as Parameters<typeof nimbusHttpAction>[0],
  ) as PublicHttpAction;
}

export function httpRouter(): HttpRouter {
  return nimbusHttpRouter() as HttpRouter;
}

export function defineTable<Fields extends ArgValidators>(
  fields: Fields,
): TableDefinition<Fields> {
  return nimbusDefineTable(fields) as TableDefinition<Fields>;
}

export function defineSchema<Tables extends Record<string, TableDefinition<any>>>(
  tables: Tables,
): SchemaDefinition<Tables> {
  return nimbusDefineSchema(tables) as SchemaDefinition<Tables>;
}
