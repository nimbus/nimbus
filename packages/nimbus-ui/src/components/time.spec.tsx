import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RelativeTime, Uptime } from "./time";

const FROZEN_NOW = new Date("2026-05-15T12:00:00Z").getTime();

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(FROZEN_NOW);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("RelativeTime", () => {
  it("renders the relative diff against the frozen now", () => {
    render(<RelativeTime epochMs={FROZEN_NOW - 90_000} />);
    expect(screen.getByText("1m ago")).toBeInTheDocument();
  });

  it("exposes an ISO dateTime + absolute title for assistive tech", () => {
    render(<RelativeTime epochMs={FROZEN_NOW - 5 * 60_000} />);
    const time = screen.getByText("5m ago");
    expect(time.tagName).toBe("TIME");
    expect(time).toHaveAttribute("datetime", "2026-05-15T11:55:00.000Z");
    expect(time).toHaveAttribute("title", "2026-05-15 11:55:00.000");
  });

  it("re-renders when the 15s ticker fires", () => {
    render(<RelativeTime epochMs={FROZEN_NOW - 4_000} />);
    expect(screen.getByText("just now")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(15_000);
    });
    expect(screen.getByText("19s ago")).toBeInTheDocument();
  });
});

describe("Uptime", () => {
  it("renders human-readable uptime from the start timestamp", () => {
    render(<Uptime startedAtMs={FROZEN_NOW - (2 * 3_600_000 + 5 * 60_000)} />);
    expect(screen.getByText("2h 5m")).toBeInTheDocument();
  });
});
