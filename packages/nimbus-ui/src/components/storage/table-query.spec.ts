import { describe, expect, it } from "vitest";

import {
  describeFilter,
  indexBackedFields,
  parseFilters,
  parseFilterValue,
  parseOrder,
} from "./table-query";

describe("parseFilters", () => {
  it("keeps well-formed filters and drops everything else", () => {
    expect(
      parseFilters([
        { field: "name", op: "eq", value: "ada" },
        { field: "n", op: "gte", value: 3 },
        { field: "", op: "eq", value: 1 },
        { field: "x", op: "like", value: 1 },
        { field: "y" },
        "nonsense",
        null,
      ]),
    ).toEqual([
      { field: "name", op: "eq", value: "ada" },
      { field: "n", op: "gte", value: 3 },
    ]);
  });

  // `validateSearch` runs on every navigation, including one driven by a
  // hand-edited or stale URL. Throwing there blanks the route, so the parser
  // has to be total.
  it("returns an empty list for junk rather than throwing", () => {
    expect(parseFilters(undefined)).toEqual([]);
    expect(parseFilters("not-an-array")).toEqual([]);
    expect(parseFilters(42)).toEqual([]);
  });
});

describe("parseOrder", () => {
  it("defaults to ascending and rejects an empty field", () => {
    expect(parseOrder("name", undefined)).toEqual({
      field: "name",
      direction: "asc",
    });
    expect(parseOrder("name", "desc")).toEqual({
      field: "name",
      direction: "desc",
    });
    expect(parseOrder("name", "sideways")).toEqual({
      field: "name",
      direction: "asc",
    });
    expect(parseOrder("", "asc")).toBeNull();
    expect(parseOrder(undefined, "asc")).toBeNull();
  });
});

describe("parseFilterValue", () => {
  // Documents are JSON. A filter on a numeric field that sends the string
  // "42" matches nothing, and the operator sees an empty page with no
  // explanation, so typed input has to be read as its JSON type.
  it("reads numbers, booleans and null as their JSON types", () => {
    expect(parseFilterValue("42")).toBe(42);
    expect(parseFilterValue("-1.5e3")).toBe(-1500);
    expect(parseFilterValue("true")).toBe(true);
    expect(parseFilterValue("false")).toBe(false);
    expect(parseFilterValue("null")).toBeNull();
  });

  it("keeps bare text as a string and lets quotes force the string reading", () => {
    expect(parseFilterValue("ada")).toBe("ada");
    expect(parseFilterValue('"42"')).toBe("42");
    expect(parseFilterValue('"true"')).toBe("true");
  });

  it("parses arrays and objects, falling back to the raw text when invalid", () => {
    expect(parseFilterValue("[1,2]")).toEqual([1, 2]);
    expect(parseFilterValue('{"a":1}')).toEqual({ a: 1 });
    expect(parseFilterValue("{not json")).toBe("{not json");
  });
});

describe("describeFilter", () => {
  it("renders the operator as a symbol and quotes an empty string", () => {
    expect(describeFilter({ field: "n", op: "gte", value: 3 })).toBe("n ≥ 3");
    expect(describeFilter({ field: "s", op: "eq", value: "" })).toBe('s = ""');
    expect(describeFilter({ field: "b", op: "neq", value: true })).toBe(
      "b ≠ true",
    );
  });
});

describe("indexBackedFields", () => {
  it("always includes _id, the primary key", () => {
    expect(indexBackedFields(null)).toEqual(new Set(["_id"]));
  });

  // A composite index on (a, b) orders by `a`, not by `b`: sorting on `b`
  // still scans, so only the leading field counts as index-backed.
  it("takes only the leading field of each index", () => {
    const fields = indexBackedFields({
      indexes: [
        { name: "by_author", fields: ["author", "createdAt"] },
        { name: "by_room", fields: ["room"] },
        { name: "empty", fields: [] },
      ],
    });
    expect(fields.has("author")).toBe(true);
    expect(fields.has("room")).toBe(true);
    expect(fields.has("createdAt")).toBe(false);
    expect(fields.size).toBe(3);
  });
});
