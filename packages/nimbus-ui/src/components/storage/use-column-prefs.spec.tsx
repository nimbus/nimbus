import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { DocumentJson } from "../../lib/types/table";
import {
  resolveColumns,
  useColumnPrefs,
  useDiscoveredFields,
} from "./use-column-prefs";

const KEY = "nimbus-ui:columns:demo:messages";

beforeEach(() => window.localStorage.clear());

describe("useColumnPrefs", () => {
  it("persists hidden columns under the console's nimbus-ui: prefix", () => {
    const { result } = renderHook(() => useColumnPrefs("demo", "messages"));

    act(() => result.current.setHidden("body", true));
    expect(result.current.prefs.hidden).toEqual(["body"]);
    expect(JSON.parse(window.localStorage.getItem(KEY) ?? "{}")).toEqual({
      hidden: ["body"],
      order: [],
    });
  });

  it("reloads the saved layout on a later mount", () => {
    window.localStorage.setItem(
      KEY,
      JSON.stringify({ hidden: ["body"], order: ["author", "room"] }),
    );
    const { result } = renderHook(() => useColumnPrefs("demo", "messages"));
    expect(result.current.prefs).toEqual({
      hidden: ["body"],
      order: ["author", "room"],
    });
  });

  it("keeps each tenant and table on its own layout", async () => {
    const { result, rerender } = renderHook(
      ({ table }: { table: string }) => useColumnPrefs("demo", table),
      { initialProps: { table: "messages" } },
    );

    act(() => result.current.setHidden("body", true));
    rerender({ table: "users" });
    await waitFor(() => expect(result.current.prefs.hidden).toEqual([]));
    expect(
      window.localStorage.getItem("nimbus-ui:columns:demo:users"),
    ).toBeNull();

    rerender({ table: "messages" });
    await waitFor(() => expect(result.current.prefs.hidden).toEqual(["body"]));
  });

  it("moves a column and records the resolved order", () => {
    const { result } = renderHook(() => useColumnPrefs("demo", "messages"));

    act(() => result.current.moveColumn(["_id", "a", "b", "c"], "c", -1));
    expect(result.current.prefs.order).toEqual(["_id", "a", "c", "b"]);
  });

  it("refuses a move that would leave the row", () => {
    const { result } = renderHook(() => useColumnPrefs("demo", "messages"));

    act(() => result.current.moveColumn(["_id", "a"], "_id", -1));
    act(() => result.current.moveColumn(["_id", "a"], "a", 1));
    act(() => result.current.moveColumn(["_id", "a"], "missing", 1));
    expect(result.current.prefs.order).toEqual([]);
  });

  it("removes the key entirely on reset rather than storing an empty layout", () => {
    const { result } = renderHook(() => useColumnPrefs("demo", "messages"));

    act(() => result.current.setHidden("body", true));
    expect(window.localStorage.getItem(KEY)).not.toBeNull();
    act(() => result.current.reset());
    expect(result.current.prefs).toEqual({ hidden: [], order: [] });
    expect(window.localStorage.getItem(KEY)).toBeNull();
  });

  it("survives a corrupt stored value", () => {
    window.localStorage.setItem(KEY, "{not json");
    const { result } = renderHook(() => useColumnPrefs("demo", "messages"));
    expect(result.current.prefs).toEqual({ hidden: [], order: [] });
  });
});

describe("resolveColumns", () => {
  // `_id` anchors the sticky identity column, so it is pinned first and is not
  // hideable no matter what the saved layout says.
  it("pins _id first and refuses to hide it", () => {
    expect(
      resolveColumns(["body", "_id", "author"], {
        hidden: ["_id"],
        order: [],
      }),
    ).toEqual(["_id", "body", "author"]);
  });

  it("applies the saved order and appends fields it has never seen", () => {
    expect(
      resolveColumns(["a", "b", "c", "_id"], { hidden: [], order: ["c", "a"] }),
    ).toEqual(["_id", "c", "a", "b"]);
  });

  it("ignores a saved field the table no longer has", () => {
    expect(
      resolveColumns(["a"], { hidden: ["gone"], order: ["gone", "a"] }),
    ).toEqual(["_id", "a"]);
  });

  it("drops hidden fields", () => {
    expect(resolveColumns(["a", "b"], { hidden: ["b"], order: [] })).toEqual([
      "_id",
      "a",
    ]);
  });
});

describe("useDiscoveredFields", () => {
  const page = (...fields: string[]): DocumentJson[] => [
    Object.fromEntries([
      ["_id", "x"],
      ["_creationTime", 1],
      ...fields.map((f) => [f, 1] as const),
    ]),
  ];

  // A schemaless table has no field list, and one page of 25 documents is
  // exactly how an operator wrongly concludes a field does not exist.
  it("accumulates fields across pages and skips reserved columns", async () => {
    const { result, rerender } = renderHook(
      ({ rows }: { rows: DocumentJson[] }) =>
        useDiscoveredFields("demo", "messages", rows),
      { initialProps: { rows: page("a", "b") } },
    );

    await waitFor(() => expect(result.current).toEqual(["a", "b"]));
    rerender({ rows: page("b", "c") });
    await waitFor(() => expect(result.current).toEqual(["a", "b", "c"]));
  });

  it("starts over when the table changes", async () => {
    const { result, rerender } = renderHook(
      ({ table, rows }: { table: string; rows: DocumentJson[] }) =>
        useDiscoveredFields("demo", table, rows),
      { initialProps: { table: "messages", rows: page("a") } },
    );

    await waitFor(() => expect(result.current).toEqual(["a"]));
    rerender({ table: "users", rows: page("email") });
    await waitFor(() => expect(result.current).toEqual(["email"]));
  });
});
