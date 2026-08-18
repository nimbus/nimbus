import { useCallback, useEffect, useRef, useState } from "react";

import type { DocumentJson } from "../../lib/types/table";

const PREFIX = "nimbus-ui:columns:";

export type ColumnPrefs = {
  /** Fields the operator hid. Everything else is visible. */
  hidden: string[];
  /** Explicit left-to-right order. Fields not listed keep discovery order. */
  order: string[];
};

const EMPTY: ColumnPrefs = { hidden: [], order: [] };

function storageKey(tenant: string, table: string): string {
  return `${PREFIX}${tenant}:${table}`;
}

function readPrefs(tenant: string, table: string): ColumnPrefs {
  if (typeof window === "undefined" || !tenant || !table) return EMPTY;
  try {
    const raw = window.localStorage.getItem(storageKey(tenant, table));
    if (!raw) return EMPTY;
    const parsed = JSON.parse(raw) as Partial<ColumnPrefs>;
    return {
      hidden: Array.isArray(parsed.hidden)
        ? parsed.hidden.filter((f): f is string => typeof f === "string")
        : [],
      order: Array.isArray(parsed.order)
        ? parsed.order.filter((f): f is string => typeof f === "string")
        : [],
    };
  } catch {
    return EMPTY;
  }
}

function writePrefs(tenant: string, table: string, prefs: ColumnPrefs): void {
  if (typeof window === "undefined" || !tenant || !table) return;
  try {
    if (prefs.hidden.length === 0 && prefs.order.length === 0) {
      window.localStorage.removeItem(storageKey(tenant, table));
      return;
    }
    window.localStorage.setItem(
      storageKey(tenant, table),
      JSON.stringify(prefs),
    );
  } catch {
    // A full or disabled localStorage must not break the browser.
  }
}

/**
 * Column visibility and order, persisted per tenant+table in `localStorage`
 * under the console's `nimbus-ui:` prefix (DESIGN.md:1132 — "Column resize,
 * visibility, and order persist per resource type, per user, in localStorage").
 */
export function useColumnPrefs(tenant: string, table: string) {
  const [prefs, setPrefs] = useState<ColumnPrefs>(() =>
    readPrefs(tenant, table),
  );

  // Re-read when the operator switches table or tenant; each pair has its own
  // saved layout.
  useEffect(() => {
    setPrefs(readPrefs(tenant, table));
  }, [tenant, table]);

  const update = useCallback(
    (next: (prev: ColumnPrefs) => ColumnPrefs) => {
      setPrefs((prev) => {
        const value = next(prev);
        writePrefs(tenant, table, value);
        return value;
      });
    },
    [tenant, table],
  );

  const setHidden = useCallback(
    (field: string, hidden: boolean) => {
      update((prev) => ({
        ...prev,
        hidden: hidden
          ? prev.hidden.includes(field)
            ? prev.hidden
            : [...prev.hidden, field]
          : prev.hidden.filter((f) => f !== field),
      }));
    },
    [update],
  );

  // `visible` is the resolved order the table is rendering, so a move is
  // expressed against what the operator can actually see rather than against a
  // sparse saved list.
  const moveColumn = useCallback(
    (visible: string[], field: string, delta: number) => {
      const from = visible.indexOf(field);
      const to = from + delta;
      if (from < 0 || to < 0 || to >= visible.length) return;
      const next = visible.slice();
      next.splice(to, 0, ...next.splice(from, 1));
      update((prev) => ({ ...prev, order: next }));
    },
    [update],
  );

  const reset = useCallback(() => {
    update(() => EMPTY);
  }, [update]);

  return { prefs, setHidden, moveColumn, reset };
}

/**
 * Every field seen since this table was opened.
 *
 * A schemaless table has no field list, and the page-local key set only covers
 * the 25 documents currently on screen — which is exactly how an operator
 * concludes a field does not exist. Accumulating across visited pages does not
 * make the set complete, but it makes it monotonic and honest: the chooser
 * labels it "fields seen so far".
 */
export function useDiscoveredFields(
  tenant: string,
  table: string,
  rows: DocumentJson[] | undefined,
): string[] {
  const [fields, setFields] = useState<string[]>([]);
  const scope = useRef<string>(`${tenant} ${table}`);

  // Resetting during render rather than in an effect keeps one table's fields
  // from ever being offered as columns for another.
  const key = `${tenant} ${table}`;
  if (scope.current !== key) {
    scope.current = key;
    setFields([]);
  }

  useEffect(() => {
    if (!rows) return;
    setFields((prev) => {
      let next = prev;
      for (const row of rows) {
        for (const field of Object.keys(row)) {
          if (field.startsWith("_")) continue;
          if (!next.includes(field)) {
            if (next === prev) next = prev.slice();
            next.push(field);
          }
        }
      }
      return next;
    });
  }, [rows]);

  return fields;
}

/**
 * Apply saved order and visibility to the discovered/declared field list.
 * `_id` is pinned first and can never be hidden — it is the row's identity and
 * the anchor of the sticky identity column.
 */
export function resolveColumns(
  available: string[],
  prefs: ColumnPrefs,
): string[] {
  const pool = available.filter((f) => f !== "_id");
  const ordered = [
    ...prefs.order.filter((f) => pool.includes(f)),
    ...pool.filter((f) => !prefs.order.includes(f)),
  ];
  return ["_id", ...ordered.filter((f) => !prefs.hidden.includes(f))];
}
