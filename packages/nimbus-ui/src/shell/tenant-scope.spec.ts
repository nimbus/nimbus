import { describe, expect, it } from "vitest";

import {
  describeTenantScope,
  parseTenantScope,
  serializeTenantScope,
  type TenantScope,
} from "./tenant-scope";

describe("tenant-scope", () => {
  describe("parseTenantScope", () => {
    it("returns kind: 'all' for undefined", () => {
      expect(parseTenantScope(undefined)).toEqual({ kind: "all" });
    });

    it("returns kind: 'all' for null", () => {
      expect(parseTenantScope(null)).toEqual({ kind: "all" });
    });

    it("returns kind: 'all' for empty string", () => {
      expect(parseTenantScope("")).toEqual({ kind: "all" });
    });

    it("returns kind: 'all' for whitespace-only string", () => {
      expect(parseTenantScope("   ")).toEqual({ kind: "all" });
    });

    it("returns kind: 'specific' for a tenant id", () => {
      expect(parseTenantScope("beta")).toEqual({
        kind: "specific",
        tenantId: "beta",
      });
    });

    it("trims surrounding whitespace", () => {
      expect(parseTenantScope("  beta  ")).toEqual({
        kind: "specific",
        tenantId: "beta",
      });
    });
  });

  describe("serializeTenantScope", () => {
    it("returns undefined for kind: 'all'", () => {
      expect(serializeTenantScope({ kind: "all" })).toBeUndefined();
    });

    it("returns the tenant id for kind: 'specific'", () => {
      expect(serializeTenantScope({ kind: "specific", tenantId: "beta" })).toBe(
        "beta",
      );
    });
  });

  describe("describeTenantScope", () => {
    it("returns 'all tenants' for kind: 'all'", () => {
      expect(describeTenantScope({ kind: "all" })).toBe("all tenants");
    });

    it("returns the tenant id for kind: 'specific'", () => {
      expect(describeTenantScope({ kind: "specific", tenantId: "beta" })).toBe(
        "beta",
      );
    });
  });

  describe("round-trip", () => {
    it("parse(serialize(specific)) preserves the scope", () => {
      const scope: TenantScope = { kind: "specific", tenantId: "beta" };
      expect(parseTenantScope(serializeTenantScope(scope))).toEqual(scope);
    });

    it("parse(serialize(all)) preserves the scope", () => {
      const scope: TenantScope = { kind: "all" };
      expect(parseTenantScope(serializeTenantScope(scope))).toEqual(scope);
    });
  });
});
