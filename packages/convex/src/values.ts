import type { JsonValue } from "./internal/shared";

export type GenericId<TableName extends string> = string & {
  readonly __tableName?: TableName;
};

export type Validator<T> = {
  readonly kind: string;
  readonly value?: unknown;
  readonly tableName?: string;
  readonly fields?: Record<string, Validator<unknown>>;
  readonly element?: Validator<unknown>;
  readonly members?: readonly Validator<unknown>[];
  readonly inner?: Validator<unknown>;
  readonly _type?: T;
};

export type Infer<Schema> = Schema extends Validator<infer T> ? T : never;

function validator<T>(
  kind: string,
  extra: Omit<Validator<T>, "kind" | "_type"> = {},
): Validator<T> {
  return { kind, ...extra };
}

export const v = {
  any(): Validator<JsonValue> {
    return validator("any");
  },
  null(): Validator<null> {
    return validator("null");
  },
  string(): Validator<string> {
    return validator("string");
  },
  number(): Validator<number> {
    return validator("number");
  },
  boolean(): Validator<boolean> {
    return validator("boolean");
  },
  id<TableName extends string>(tableName: TableName): Validator<GenericId<TableName>> {
    return validator("id", { tableName });
  },
  literal<Value extends JsonValue>(value: Value): Validator<Value> {
    return validator("literal", { value });
  },
  array<Value>(element: Validator<Value>): Validator<Value[]> {
    return validator("array", { element });
  },
  object<Fields extends Record<string, Validator<unknown>>>(
    fields: Fields,
  ): Validator<{ [Key in keyof Fields]: Infer<Fields[Key]> }> {
    return validator("object", { fields });
  },
  optional<Value>(inner: Validator<Value>): Validator<Value | undefined> {
    return validator("optional", { inner });
  },
  union<Members extends readonly Validator<unknown>[]>(
    ...members: Members
  ): Validator<Infer<Members[number]>> {
    return validator("union", { members });
  },
};
