import { describe, expect, it } from "vitest";

import {
  buildFunctionTree,
  filterFunctionTree,
  parseFunctionPath,
} from "./function-tree";

describe("parseFunctionPath", () => {
  it("splits module:function paths", () => {
    expect(parseFunctionPath("messages:send")).toEqual({
      folders: [],
      module: "messages",
      fn: "send",
    });
  });

  it("handles nested folder paths", () => {
    expect(
      parseFunctionPath("billing/transfers:cancelMySubscriptionInternal"),
    ).toEqual({
      folders: ["billing"],
      module: "transfers",
      fn: "cancelMySubscriptionInternal",
    });
  });

  it("handles deeply nested folder paths", () => {
    expect(parseFunctionPath("a/b/c/d:fn")).toEqual({
      folders: ["a", "b", "c"],
      module: "d",
      fn: "fn",
    });
  });

  it("treats colon-less paths as default function", () => {
    expect(parseFunctionPath("internalUtil")).toEqual({
      folders: [],
      module: "internalUtil",
      fn: "default",
    });
  });

  it("handles colon at end (empty function name treated literally)", () => {
    expect(parseFunctionPath("messages:")).toEqual({
      folders: [],
      module: "messages",
      fn: "",
    });
  });
});

describe("buildFunctionTree", () => {
  it("returns an empty tree for no inputs", () => {
    const tree = buildFunctionTree([]);
    expect(tree).toEqual({ folders: [], modules: [], count: 0 });
  });

  it("ignores entries without a path", () => {
    const tree = buildFunctionTree([{}, { path: "" }, { path: "messages:send" }]);
    expect(tree.count).toBe(1);
    expect(tree.modules.map((m) => m.name)).toEqual(["messages"]);
  });

  it("groups root-level modules and nested folders", () => {
    const tree = buildFunctionTree([
      { path: "messages:send" },
      { path: "messages:list" },
      { path: "billing/transfers:cancel" },
      { path: "billing/transfers:refund" },
      { path: "billing/invoices:list" },
      { path: "auth:login" },
    ]);
    expect(tree.count).toBe(6);
    expect(tree.modules.map((m) => m.name)).toEqual(["auth", "messages"]);
    expect(tree.folders.map((f) => f.name)).toEqual(["billing"]);
    const billing = tree.folders[0];
    expect(billing.modules.map((m) => m.name)).toEqual(["invoices", "transfers"]);
    const transfers = billing.modules.find((m) => m.name === "transfers")!;
    expect(transfers.functions.map((f) => f.name)).toEqual(["cancel", "refund"]);
  });

  it("sorts folders, modules, and functions alphabetically", () => {
    const tree = buildFunctionTree([
      { path: "z:c" },
      { path: "z:a" },
      { path: "z:b" },
      { path: "m:fn" },
      { path: "a:fn" },
    ]);
    expect(tree.modules.map((m) => m.name)).toEqual(["a", "m", "z"]);
    const z = tree.modules.find((m) => m.name === "z")!;
    expect(z.functions.map((f) => f.name)).toEqual(["a", "b", "c"]);
  });

  it("carries lastStatus into leaves", () => {
    const tree = buildFunctionTree([
      { path: "messages:send", lastStatus: "ok" },
    ]);
    expect(tree.modules[0].functions[0].lastStatus).toBe("ok");
  });
});

describe("filterFunctionTree", () => {
  const tree = buildFunctionTree([
    { path: "messages:send" },
    { path: "billing/transfers:cancel" },
    { path: "billing/invoices:list" },
    { path: "auth:login" },
  ]);

  it("returns the original tree when filter is empty", () => {
    expect(filterFunctionTree(tree, "")).toBe(tree);
    expect(filterFunctionTree(tree, "   ")).toBe(tree);
  });

  it("matches by function name", () => {
    const filtered = filterFunctionTree(tree, "cancel");
    expect(filtered.modules).toEqual([]);
    expect(filtered.folders.length).toBe(1);
    expect(filtered.folders[0].modules.length).toBe(1);
    expect(filtered.folders[0].modules[0].functions.map((f) => f.name)).toEqual(
      ["cancel"],
    );
  });

  it("matches by module name and includes all its functions", () => {
    const filtered = filterFunctionTree(tree, "transfers");
    expect(filtered.folders.length).toBe(1);
    const transfers = filtered.folders[0].modules[0];
    expect(transfers.name).toBe("transfers");
    expect(transfers.functions.length).toBe(1);
  });

  it("matches by folder name and keeps the whole subtree", () => {
    const filtered = filterFunctionTree(tree, "billing");
    expect(filtered.folders.length).toBe(1);
    expect(filtered.folders[0].modules.map((m) => m.name)).toEqual([
      "invoices",
      "transfers",
    ]);
  });

  it("returns an empty tree when nothing matches", () => {
    const filtered = filterFunctionTree(tree, "nope");
    expect(filtered.folders).toEqual([]);
    expect(filtered.modules).toEqual([]);
  });
});
