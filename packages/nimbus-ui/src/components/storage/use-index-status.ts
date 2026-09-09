import { useEffect, useState } from "react";

import { schema as schemaApi } from "../../lib/api-mutations";
import type { TableSchemaShape } from "../../lib/types/table";

/** Index name to the state the server last reported for it. */
export type IndexStates = Map<string, string>;

/**
 * The build state of each index in a table, read from the server.
 *
 * The `tables` row the console subscribes to carries the schema as it was
 * committed, and the state inside it is whatever the commit wrote. A build
 * that finishes later would not update that row, so this hook asks the
 * schema route directly and keeps asking while any index is still on its
 * way to `enabled`. A settled table is read once per `schema` change.
 */
export function useIndexStatus(
  tenant: string,
  table: string,
  schema: TableSchemaShape | null,
  pollMs = 1500,
): IndexStates {
  const [states, setStates] = useState<IndexStates>(() => new Map());
  // The schema is a fresh object each render; its serialization is what a
  // new read keys off.
  const schemaKey = JSON.stringify(schema?.indexes ?? []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: schemaKey is the re-read trigger — a schema change (apply, drop) must start a fresh poll
  useEffect(() => {
    if (!tenant || !table) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const read = async () => {
      const result = await schemaApi.get(tenant, table);
      if (cancelled) return;
      // A 404 is "no schema, so no indexes"; any other failure keeps the
      // last states rather than blanking the pills mid-poll.
      if (!result.ok) {
        if (result.status === 404) setStates(new Map());
        return;
      }
      const indexes = (result.data as TableSchemaShape | null)?.indexes ?? [];
      const next: IndexStates = new Map();
      for (const index of indexes) {
        next.set(index.name, index.state ?? "unknown");
      }
      setStates(next);
      const settled = indexes.every((index) => index.state === "enabled");
      if (!settled) timer = setTimeout(() => void read(), pollMs);
    };
    void read();

    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [tenant, table, schemaKey, pollMs]);

  return states;
}
