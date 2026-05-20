import { describe, expect, it } from "vitest";

import { resolveLensView } from "./system-tenant-lens";

describe("resolveLensView", () => {
  it("maps /app/storage to the tables view", () => {
    expect(resolveLensView("/developer/storage")).toEqual({
      kind: "tables",
      label: "tables",
    });
  });

  it("maps /app/compute to the functions view", () => {
    expect(resolveLensView("/developer/compute")).toEqual({
      kind: "functions",
      label: "functions",
    });
  });

  it("maps /app/observability to the runs view", () => {
    expect(resolveLensView("/developer/observability")).toEqual({
      kind: "runs",
      label: "runs",
    });
  });

  it("maps /admin/machines to the machines view", () => {
    expect(resolveLensView("/operator/machines")).toEqual({
      kind: "machines",
      label: "machines",
    });
  });

  it("maps /admin/network to the listeners view", () => {
    expect(resolveLensView("/operator/network")).toEqual({
      kind: "listeners",
      label: "listeners",
    });
  });

  it("falls back to system.status on /admin/settings", () => {
    expect(resolveLensView("/operator/settings")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("falls back to system.status on /app/settings", () => {
    expect(resolveLensView("/developer/settings")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("falls back to system.status on bare /app", () => {
    expect(resolveLensView("/developer")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("falls back to system.status on bare /admin", () => {
    expect(resolveLensView("/operator")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });

  it("matches the observability view across personas", () => {
    expect(resolveLensView("/developer/observability").kind).toBe("runs");
    expect(resolveLensView("/operator/observability").kind).toBe("runs");
  });

  it("strips query strings and hash before matching", () => {
    expect(resolveLensView("/developer/storage?tenant=demo").kind).toBe("tables");
    expect(resolveLensView("/operator/machines#detail").kind).toBe("machines");
  });

  it("returns system.status for an unrelated pathname", () => {
    expect(resolveLensView("/unknown/route")).toEqual({
      kind: "system",
      label: "system.status",
    });
  });
});
