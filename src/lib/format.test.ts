import { describe, expect, it } from "vitest";
import { formatAgo, formatBytes, formatDuration, normalizePrefix } from "./format";

describe("normalizePrefix", () => {
  it("collapses an IPv6 address to its /64", () => {
    expect(normalizePrefix("2606:4700:1234:5678:9abc:def0:1234:5678")).toBe(
      "2606:4700:1234:5678::",
    );
  });

  it("passes non-IPv6 values through unchanged", () => {
    expect(normalizePrefix("1.1.1.1")).toBe("1.1.1.1");
    expect(normalizePrefix("")).toBe("");
  });
});

describe("formatBytes", () => {
  it("formats human readable sizes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(1024 * 1024 * 3)).toBe("3.0 MB");
    expect(formatBytes(null)).toBe("0 B");
  });
});

describe("formatAgo / formatDuration", () => {
  it("describes recent epochs", () => {
    const now = Math.floor(Date.now() / 1000);
    expect(formatAgo(now - 5)).toBe("5s ago");
    expect(formatAgo(now - 120)).toBe("2 min ago");
    expect(formatAgo(null)).toBe("—");
  });

  it("formats durations", () => {
    expect(formatDuration(42)).toBe("42 s");
    expect(formatDuration(120)).toBe("2 min");
    expect(formatDuration(null)).toBe("—");
  });
});
