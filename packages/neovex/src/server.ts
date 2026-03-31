import type {
  ActionReference,
  MutationReference,
  PaginatedQueryReference,
  QueryReference,
  FunctionVisibility,
  JsonValue,
  MutationShape,
  QueryShape,
} from "./internal/shared";
import {
  makeActionReference,
  makeMutationReference,
  makePaginatedQueryReference,
  makeQueryReference,
} from "./internal/shared";
import type { GenericId, Infer, Validator } from "./values";

export type DefaultFunctionArgs = Record<string, unknown>;

export type AuthProvider =
  | {
      applicationID: string;
      domain: string;
    }
  | {
      type: "customJwt";
      applicationID?: string;
      issuer: string;
      jwks: string;
      algorithm: "RS256" | "ES256";
    };

export type AuthConfig = {
  providers: AuthProvider[];
};

export interface UserIdentity {
  readonly tokenIdentifier: string;
  readonly subject: string;
  readonly issuer: string;
  readonly name?: string;
  readonly givenName?: string;
  readonly familyName?: string;
  readonly nickname?: string;
  readonly preferredUsername?: string;
  readonly profileUrl?: string;
  readonly pictureUrl?: string;
  readonly email?: string;
  readonly emailVerified?: boolean;
  readonly gender?: string;
  readonly birthday?: string;
  readonly timezone?: string;
  readonly language?: string;
  readonly phoneNumber?: string;
  readonly phoneNumberVerified?: boolean;
  readonly address?: string;
  readonly updatedAt?: string;
  readonly [key: string]: JsonValue | undefined;
}

export type UserIdentityAttributes = Omit<UserIdentity, "tokenIdentifier">;

export type VerifiedIdentityKind = "oidc" | "custom_jwt";

export interface VerifiedIdentity extends UserIdentity {
  readonly kind: VerifiedIdentityKind;
}

export type VerifiedIdentityAttributes = Omit<
  VerifiedIdentity,
  "kind" | "tokenIdentifier"
>;

export interface Auth {
  getUserIdentity(): Promise<UserIdentity | null>;
  getVerifiedIdentity(): Promise<VerifiedIdentity | null>;
}

type ArgValidators = Record<string, Validator<unknown>>;
type TableIndexes = readonly {
  readonly name: string;
  readonly fields: readonly string[];
}[];

type InferArgs<Args> = Args extends ArgValidators
  ? { [Key in keyof Args]: Infer<Args[Key]> }
  : DefaultFunctionArgs;

type InferReturns<Returns> = Returns extends Validator<unknown>
  ? Infer<Returns>
  : unknown;

type QueryDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericQueryCtx,
    args: InferArgs<Args>,
  ) => unknown | Promise<unknown>;
};

type PaginatedQueryDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericQueryCtx,
    args: InferArgs<Args>,
  ) => unknown | Promise<unknown>;
};

type MutationDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericMutationCtx,
    args: InferArgs<Args>,
  ) => unknown | Promise<unknown>;
};

type ActionDefinition<Args extends ArgValidators | undefined, Returns> = {
  args?: Args;
  returns?: Returns;
  handler: (
    ctx: GenericActionCtx,
    args: InferArgs<Args>,
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
> = QueryReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: QueryDefinition<any, any>["handler"];
};

export type RegisteredPaginatedQuery<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = PaginatedQueryReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: PaginatedQueryDefinition<any, any>["handler"];
};

export type RegisteredMutation<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = MutationReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: MutationDefinition<any, any>["handler"];
};

export type RegisteredAction<
  Visibility extends FunctionVisibility,
  Args extends DefaultFunctionArgs,
  Returns,
> = ActionReference<Args, Returns> & {
  readonly visibility: Visibility;
  readonly handler: ActionDefinition<any, any>["handler"];
};

export type PublicHttpAction = {
  readonly kind: "http_action";
  readonly visibility: "public";
  readonly handler: HttpActionHandler;
};

export type HttpRouteMethod =
  | "GET"
  | "POST"
  | "PUT"
  | "PATCH"
  | "DELETE"
  | "OPTIONS"
  | "HEAD";

export type HttpRouteSpec = {
  readonly path?: string;
  readonly pathPrefix?: string;
  readonly method: HttpRouteMethod;
  readonly handler: PublicHttpAction;
};

export type HttpRouter = {
  readonly routes: readonly HttpRouteSpec[];
  route(spec: HttpRouteSpec): HttpRouter;
};

export type TableDefinition<Fields extends ArgValidators> = {
  readonly kind: "table";
  readonly fields: Fields;
  readonly indexes: TableIndexes;
  index(
    name: string,
    fields: readonly [keyof Fields & string, ...(keyof Fields & string)[]],
  ): TableDefinition<Fields>;
};

export type SchemaDefinition<Tables extends Record<string, TableDefinition<any>>> = {
  readonly tables: Tables;
};

export type FunctionReference<Args extends DefaultFunctionArgs, Returns> =
  | QueryReference<Args, Returns>
  | MutationReference<Args, Returns>
  | ActionReference<Args, Returns>
  | PaginatedQueryReference<Args, Returns>;

export type QueryOrder = "asc" | "desc";

export type IndexRangeBuilder = {
  eq(field: string, value: unknown): IndexRangeBuilder;
  neq(field: string, value: unknown): IndexRangeBuilder;
  gt(field: string, value: unknown): IndexRangeBuilder;
  gte(field: string, value: unknown): IndexRangeBuilder;
  lt(field: string, value: unknown): IndexRangeBuilder;
  lte(field: string, value: unknown): IndexRangeBuilder;
};

export type FilterField = {
  readonly field: string;
};

export type FilterExpressionBuilder = {
  field(name: string): FilterField;
  eq(field: string | FilterField, value: unknown): FilterExpressionBuilder;
  neq(field: string | FilterField, value: unknown): FilterExpressionBuilder;
  gt(field: string | FilterField, value: unknown): FilterExpressionBuilder;
  gte(field: string | FilterField, value: unknown): FilterExpressionBuilder;
  lt(field: string | FilterField, value: unknown): FilterExpressionBuilder;
  lte(field: string | FilterField, value: unknown): FilterExpressionBuilder;
};

export type QueryBuilder = {
  withIndex(
    indexName: string,
    builder?: (query: IndexRangeBuilder) => IndexRangeBuilder,
  ): QueryBuilder;
  filter(
    builder: (query: FilterExpressionBuilder) => FilterExpressionBuilder,
  ): QueryBuilder;
  order(direction: QueryOrder): QueryBuilder;
  collect(): Promise<unknown[]>;
  take(limit: number): Promise<unknown[]>;
  first(): Promise<unknown | null>;
  unique(): Promise<unknown | null>;
};

export type GenericDatabaseReader = {
  query(tableName: string): QueryBuilder;
  get<TableName extends string>(
    id: GenericId<TableName>,
  ): Promise<unknown | null>;
};

export type GenericDatabaseWriter = GenericDatabaseReader & {
  insert(tableName: string, value: Record<string, unknown>): Promise<string>;
  patch<TableName extends string>(
    id: GenericId<TableName>,
    value: Record<string, unknown>,
  ): Promise<GenericId<TableName>>;
  delete<TableName extends string>(id: GenericId<TableName>): Promise<void>;
};

export type Scheduler = {
  runAfter<Args extends DefaultFunctionArgs>(
    delayMs: number,
    functionRef: MutationReference<Args, unknown>,
    args?: Args,
  ): Promise<string>;
  runAt<Args extends DefaultFunctionArgs>(
    timestampMs: number,
    functionRef: MutationReference<Args, unknown>,
    args?: Args,
  ): Promise<string>;
  cancel(jobId: string): Promise<void>;
};

export type GenericQueryCtx = {
  readonly db: GenericDatabaseReader;
  readonly auth: Auth;
};

export type GenericMutationCtx = {
  readonly db: GenericDatabaseWriter;
  readonly scheduler: Scheduler;
  readonly auth: Auth;
};

export type GenericActionCtx = {
  readonly scheduler: Scheduler;
  readonly auth: Auth;
  runQuery<Args extends DefaultFunctionArgs, Returns>(
    functionRef: QueryReference<Args, Returns>,
    args?: Args,
  ): Promise<Returns>;
  runMutation<Args extends DefaultFunctionArgs, Returns>(
    functionRef: MutationReference<Args, Returns>,
    args?: Args,
  ): Promise<Returns>;
  runAction<Args extends DefaultFunctionArgs, Returns>(
    functionRef: ActionReference<Args, Returns>,
    args?: Args,
  ): Promise<Returns>;
};

export function query<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: QueryDefinition<Args, Returns>,
): RegisteredQuery<"public", InferArgs<Args>, InferReturns<Returns>> {
  return registerQuery("public", definition);
}

export function internalQuery<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: QueryDefinition<Args, Returns>,
): RegisteredQuery<"internal", InferArgs<Args>, InferReturns<Returns>> {
  return registerQuery("internal", definition);
}

export function paginatedQuery<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: PaginatedQueryDefinition<Args, Returns>,
): RegisteredPaginatedQuery<"public", InferArgs<Args>, InferReturns<Returns>> {
  return registerPaginatedQuery("public", definition);
}

export function internalPaginatedQuery<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: PaginatedQueryDefinition<Args, Returns>,
): RegisteredPaginatedQuery<"internal", InferArgs<Args>, InferReturns<Returns>> {
  return registerPaginatedQuery("internal", definition);
}

export function mutation<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: MutationDefinition<Args, Returns>,
): RegisteredMutation<"public", InferArgs<Args>, InferReturns<Returns>> {
  return registerMutation("public", definition);
}

export function internalMutation<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: MutationDefinition<Args, Returns>,
): RegisteredMutation<"internal", InferArgs<Args>, InferReturns<Returns>> {
  return registerMutation("internal", definition);
}

export function action<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: ActionDefinition<Args, Returns>,
): RegisteredAction<"public", InferArgs<Args>, InferReturns<Returns>> {
  return registerAction("public", definition);
}

export function internalAction<
  Args extends ArgValidators | undefined = undefined,
  Returns extends Validator<unknown> | undefined = undefined,
>(
  definition: ActionDefinition<Args, Returns>,
): RegisteredAction<"internal", InferArgs<Args>, InferReturns<Returns>> {
  return registerAction("internal", definition);
}

export function httpAction(
  definition:
    | HttpActionHandler
    | {
        handler: HttpActionHandler;
      },
): PublicHttpAction {
  const handler =
    typeof definition === "function" ? definition : definition.handler;
  return {
    kind: "http_action",
    visibility: "public",
    handler,
  };
}

export function httpRouter(): HttpRouter {
  const routes: HttpRouteSpec[] = [];
  const router: HttpRouter = {
    routes,
    route(spec) {
      routes.push(spec);
      return router;
    },
  };
  return router;
}

export function defineTable<Fields extends ArgValidators>(
  fields: Fields,
): TableDefinition<Fields> {
  const indexes: Array<{ name: string; fields: readonly string[] }> = [];
  const table: TableDefinition<Fields> = {
    kind: "table",
    fields,
    indexes,
    index(name, indexFields) {
      indexes.push({
        name,
        fields: [...indexFields],
      });
      return table;
    },
  };
  return table;
}

export function defineSchema<Tables extends Record<string, TableDefinition<any>>>(
  tables: Tables,
): SchemaDefinition<Tables> {
  return { tables };
}

function registerQuery<
  Visibility extends FunctionVisibility,
  Args extends ArgValidators | undefined,
  Returns extends Validator<unknown> | undefined,
>(
  visibility: Visibility,
  definition: QueryDefinition<Args, Returns>,
): RegisteredQuery<Visibility, InferArgs<Args>, InferReturns<Returns>> {
  return {
    ...makeQueryReference("", visibility),
    visibility,
    handler: definition.handler,
  } as RegisteredQuery<Visibility, InferArgs<Args>, InferReturns<Returns>>;
}

function registerPaginatedQuery<
  Visibility extends FunctionVisibility,
  Args extends ArgValidators | undefined,
  Returns extends Validator<unknown> | undefined,
>(
  visibility: Visibility,
  definition: PaginatedQueryDefinition<Args, Returns>,
): RegisteredPaginatedQuery<Visibility, InferArgs<Args>, InferReturns<Returns>> {
  return {
    ...makePaginatedQueryReference("", visibility),
    visibility,
    handler: definition.handler,
  } as RegisteredPaginatedQuery<Visibility, InferArgs<Args>, InferReturns<Returns>>;
}

function registerMutation<
  Visibility extends FunctionVisibility,
  Args extends ArgValidators | undefined,
  Returns extends Validator<unknown> | undefined,
>(
  visibility: Visibility,
  definition: MutationDefinition<Args, Returns>,
): RegisteredMutation<Visibility, InferArgs<Args>, InferReturns<Returns>> {
  return {
    ...makeMutationReference("", visibility),
    visibility,
    handler: definition.handler,
  } as RegisteredMutation<Visibility, InferArgs<Args>, InferReturns<Returns>>;
}

function registerAction<
  Visibility extends FunctionVisibility,
  Args extends ArgValidators | undefined,
  Returns extends Validator<unknown> | undefined,
>(
  visibility: Visibility,
  definition: ActionDefinition<Args, Returns>,
): RegisteredAction<Visibility, InferArgs<Args>, InferReturns<Returns>> {
  return {
    ...makeActionReference("", visibility),
    visibility,
    handler: definition.handler,
  } as RegisteredAction<Visibility, InferArgs<Args>, InferReturns<Returns>>;
}
