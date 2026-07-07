// UI view of a `functions` row as returned by `api.functions.list`. Kept
// hand-rolled rather than aliased to `Doc<"functions">` because the list
// endpoint enriches each row with derived fields the generated table type does
// not carry (`adapter`, `lastStatus`, `lastRunAt`) and widens `_id` to a plain
// string, so the two shapes are deliberately not the same.
export type FunctionDoc = {
  _id: string;
  _updateTime?: number;
  path?: string;
  kind?: string;
  adapter?: string;
  bundleId?: string;
  argsSchema?: unknown;
  returnsSchema?: unknown;
  lastStatus?: string;
  lastRunAt?: number;
};
