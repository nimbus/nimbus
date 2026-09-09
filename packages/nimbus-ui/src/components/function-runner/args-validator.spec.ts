import { describe, expect, it } from "vitest";

import {
  buildArgs,
  parseArgsValidator,
  valuesFromArgs,
} from "./args-validator";

describe("parseArgsValidator", () => {
  it("reads the Nimbus SDK object validator", () => {
    const fields = parseArgsValidator({
      kind: "object",
      fields: {
        conversationId: { kind: "string" },
        limit: { kind: "optional", inner: { kind: "number" } },
        archived: { kind: "boolean" },
        owner: { kind: "id", tableName: "users" },
        tags: { kind: "array", element: { kind: "string" } },
      },
    });
    expect(fields).toEqual([
      {
        name: "conversationId",
        type: "string",
        optional: false,
        input: "text",
      },
      { name: "limit", type: "number?", optional: true, input: "number" },
      { name: "archived", type: "boolean", optional: false, input: "boolean" },
      { name: "owner", type: "id<users>", optional: false, input: "text" },
      { name: "tags", type: "array", optional: false, input: "json" },
    ]);
  });

  it("reads the Convex JSON validator", () => {
    const fields = parseArgsValidator({
      type: "object",
      value: {
        text: { fieldType: { type: "string" }, optional: false },
        count: { fieldType: { type: "float64" }, optional: true },
      },
    });
    expect(fields).toEqual([
      { name: "text", type: "string", optional: false, input: "text" },
      { name: "count", type: "number?", optional: true, input: "number" },
    ]);
  });

  it("returns null for a missing or non-object validator", () => {
    expect(parseArgsValidator(undefined)).toBeNull();
    expect(parseArgsValidator(null)).toBeNull();
    expect(parseArgsValidator({ kind: "string" })).toBeNull();
    expect(parseArgsValidator("object")).toBeNull();
  });
});

describe("buildArgs", () => {
  const fields = parseArgsValidator({
    kind: "object",
    fields: {
      text: { kind: "string" },
      limit: { kind: "optional", inner: { kind: "number" } },
      archived: { kind: "boolean" },
      meta: { kind: "optional", inner: { kind: "object", fields: {} } },
    },
  }) as NonNullable<ReturnType<typeof parseArgsValidator>>;

  it("converts each input to its argument value and omits empty optionals", () => {
    expect(
      buildArgs(fields, { text: "hi", limit: "3", archived: true, meta: "" }),
    ).toEqual({ ok: true, args: { text: "hi", limit: 3, archived: true } });
  });

  it("sends an empty required string and a false required boolean", () => {
    expect(buildArgs(fields, {})).toEqual({
      ok: true,
      args: { text: "", archived: false },
    });
  });

  it("reports a per-field error for a bad number or bad JSON", () => {
    expect(
      buildArgs(fields, { text: "x", limit: "many", meta: "{oops" }),
    ).toEqual({
      ok: false,
      errors: { limit: "Enter a number.", meta: "Enter valid JSON." },
    });
  });

  it("round-trips through valuesFromArgs", () => {
    const values = valuesFromArgs(fields, {
      text: "hi",
      limit: 3,
      archived: true,
      meta: { a: 1 },
    });
    expect(values).toEqual({
      text: "hi",
      limit: "3",
      archived: true,
      meta: '{"a":1}',
    });
    expect(buildArgs(fields, values)).toEqual({
      ok: true,
      args: { text: "hi", limit: 3, archived: true, meta: { a: 1 } },
    });
  });
});
