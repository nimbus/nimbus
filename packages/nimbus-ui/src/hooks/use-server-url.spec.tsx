import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { useNimbusMock } = vi.hoisted(() => ({ useNimbusMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useNimbus: () => useNimbusMock(),
}));

import { useServerUrl } from "./use-server-url";

afterEach(() => {
  useNimbusMock.mockReset();
});

describe("useServerUrl", () => {
  it("is the origin of the client's Convex endpoint", () => {
    useNimbusMock.mockReturnValue({
      url: "http://nimbus.example:9000/convex/_nimbus",
    });
    expect(renderHook(() => useServerUrl()).result.current).toBe(
      "http://nimbus.example:9000",
    );
  });

  it("falls back to the page origin when the client has no URL", () => {
    useNimbusMock.mockReturnValue({ url: "" });
    expect(renderHook(() => useServerUrl()).result.current).toBe(
      window.location.origin,
    );
  });

  it("returns an unparsable URL as given", () => {
    useNimbusMock.mockReturnValue({ url: "not a url" });
    expect(renderHook(() => useServerUrl()).result.current).toBe("not a url");
  });
});
