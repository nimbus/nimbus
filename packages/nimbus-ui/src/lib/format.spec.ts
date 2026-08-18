import { describe, expect, it } from "vitest";

import {
  formatAbsoluteTime,
  formatDuration,
  formatMemory,
  formatRelativeTime,
  formatUptime,
  looksLikeEpochMs,
  shortHash,
  shortId,
} from "./format";

describe("formatRelativeTime", () => {
  const now = 1_700_000_000_000;

  it("returns 'just now' for under 5 seconds", () => {
    expect(formatRelativeTime(now - 3_000, now)).toBe("just now");
    expect(formatRelativeTime(now, now)).toBe("just now");
  });

  it("returns seconds for under a minute", () => {
    expect(formatRelativeTime(now - 30_000, now)).toBe("30s ago");
  });

  it("returns minutes for under an hour", () => {
    expect(formatRelativeTime(now - 5 * 60_000, now)).toBe("5m ago");
  });

  it("returns hours for under a day", () => {
    expect(formatRelativeTime(now - 3 * 3_600_000, now)).toBe("3h ago");
  });

  it("returns days for older timestamps", () => {
    expect(formatRelativeTime(now - 4 * 86_400_000, now)).toBe("4d ago");
  });

  it("clamps future timestamps to 'just now'", () => {
    expect(formatRelativeTime(now + 60_000, now)).toBe("just now");
  });
});

describe("formatAbsoluteTime", () => {
  it("renders an ISO-shaped string without T or Z", () => {
    expect(formatAbsoluteTime(1_700_000_000_000)).toBe(
      "2023-11-14 22:13:20.000",
    );
  });

  it("falls back to the raw value on invalid input", () => {
    expect(formatAbsoluteTime(Number.NaN)).toBe("NaN");
  });
});

describe("formatUptime", () => {
  const start = 1_700_000_000_000;

  it("renders minutes when under an hour", () => {
    expect(formatUptime(start, start + 7 * 60_000)).toBe("7m");
  });

  it("renders hours + minutes when under a day", () => {
    expect(formatUptime(start, start + (2 * 3_600_000 + 15 * 60_000))).toBe(
      "2h 15m",
    );
  });

  it("renders days + hours + minutes for older windows", () => {
    expect(
      formatUptime(
        start,
        start + (3 * 86_400_000 + 4 * 3_600_000 + 5 * 60_000),
      ),
    ).toBe("3d 4h 5m");
  });
});

describe("formatDuration", () => {
  it("returns an em-dash for null or undefined", () => {
    expect(formatDuration(null)).toBe("—");
    expect(formatDuration(undefined)).toBe("—");
    expect(formatDuration(Number.NaN)).toBe("—");
  });

  it("returns <1ms below 1 millisecond", () => {
    expect(formatDuration(0.4)).toBe("<1ms");
  });

  it("rounds milliseconds under a second", () => {
    expect(formatDuration(123.6)).toBe("124ms");
  });

  it("renders seconds with two decimals under a minute", () => {
    expect(formatDuration(12_345)).toBe("12.35s");
  });

  it("renders minutes + seconds for longer durations", () => {
    expect(formatDuration(3 * 60_000 + 7_000)).toBe("3m 7s");
  });
});

describe("shortId", () => {
  it("returns the original value when it already fits", () => {
    expect(shortId("abc")).toBe("abc");
  });

  it("elides the middle so the tail stays visible", () => {
    // ULIDs share a long time-ordered prefix, so a front truncation renders
    // every id in a table identically. Keep both ends.
    expect(shortId("01ABCDEFGHIJKLMN", 7)).toBe("01A\u2026LMN");
  });

  it("keeps ids distinguishable when the prefix is shared", () => {
    const a = shortId("01M0APVQZ8K3M6DAYN");
    const b = shortId("01M0APVQZ8K3PMMMDZ");
    expect(a).not.toBe(b);
  });

  it("never exceeds the requested length", () => {
    expect(shortId("0123456789", 4)).toHaveLength(4);
    expect(shortId("01ABCDEFGHIJKLMN", 7)).toHaveLength(7);
  });

  it("honors a custom length", () => {
    expect(shortId("0123456789", 4)).toBe("01\u20269");
  });
});

describe("shortHash", () => {
  it("returns the original value when it already fits", () => {
    expect(shortHash("abc123")).toBe("abc123");
  });

  it("takes a leading prefix, because hash prefixes are the identifier", () => {
    expect(shortHash("a".repeat(64), 12)).toBe("a".repeat(12));
  });
});

describe("looksLikeEpochMs", () => {
  it("accepts plausible millisecond timestamps on time-named fields", () => {
    expect(looksLikeEpochMs("_creationTime", 1_755_529_000_000)).toBe(true);
    expect(looksLikeEpochMs("at", 1_755_529_000_000)).toBe(true);
    expect(looksLikeEpochMs("updatedAt", 1_755_529_000_000)).toBe(true);
    expect(looksLikeEpochMs("expires_at", 1_755_529_000_000)).toBe(true);
  });

  it("rejects time-named fields whose value is out of epoch-ms range", () => {
    expect(looksLikeEpochMs("at", 42)).toBe(false);
    expect(looksLikeEpochMs("at", 1_755_529_000)).toBe(false);
  });

  it("rejects plausible values on fields that do not name a time", () => {
    expect(looksLikeEpochMs("bytes", 1_755_529_000_000)).toBe(false);
    expect(looksLikeEpochMs("balance", 1_755_529_000_000)).toBe(false);
  });

  it("rejects non-integers", () => {
    expect(looksLikeEpochMs("at", 1_755_529_000_000.5)).toBe(false);
    expect(looksLikeEpochMs("at", Number.NaN)).toBe(false);
  });
});

describe("formatMemory", () => {
  it("renders an em dash for missing values", () => {
    expect(formatMemory(undefined)).toBe("—");
    expect(formatMemory(null)).toBe("—");
  });

  it("renders MiB below 1024", () => {
    expect(formatMemory(512)).toBe("512 MiB");
  });

  it("renders whole GiB without a fraction", () => {
    expect(formatMemory(2048)).toBe("2 GiB");
  });

  it("renders one fractional digit for partial GiB", () => {
    expect(formatMemory(1536)).toBe("1.5 GiB");
  });
});
