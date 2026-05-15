// Minimal local mirrors of nimbus query reference shape so this module avoids
// directly coupling to convex/browser internals.
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export type QueryReference<TArgs, TResult> = {
  __args: TArgs;
  __result: TResult;
};
