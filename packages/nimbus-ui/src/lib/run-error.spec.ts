import { describe, expect, it } from "vitest";

import { locationLine, parseRunError } from "./run-error";

describe("parseRunError", () => {
  it("reads message + structured location off a run error object", () => {
    const out = parseRunError({
      message: "boom (at messages:24)",
      location: "messages:24",
    });
    expect(out.message).toBe("boom (at messages:24)");
    expect(out.location).toBe("messages:24");
  });

  it("omits location when absent or non-string", () => {
    expect(parseRunError({ message: "boom" }).location).toBeUndefined();
    expect(
      parseRunError({ message: "boom", location: 24 }).location,
    ).toBeUndefined();
  });

  it("falls back to the string for unstructured errors", () => {
    expect(parseRunError("plain error")).toEqual({ message: "plain error" });
  });

  it("stringifies a message-less object so nothing is lost", () => {
    const out = parseRunError({ code: "x" });
    expect(out.message).toContain("code");
    expect(out.location).toBeUndefined();
  });
});

describe("locationLine", () => {
  it("extracts the 1-based line from module:line", () => {
    expect(locationLine("messages:24")).toBe(24);
    expect(locationLine("admin/users:7")).toBe(7);
  });

  it("returns undefined for malformed locations", () => {
    expect(locationLine("messages")).toBeUndefined();
    expect(locationLine("messages:abc")).toBeUndefined();
    expect(locationLine("messages:0")).toBeUndefined();
  });
});
