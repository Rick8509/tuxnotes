import { describe, expect, it } from "vitest";
import { daysUntilPurge, formatListDate, purgeLabel } from "./format";

const now = new Date(2026, 10, 18, 15, 30); // Wed 18 Nov 2026, 15:30 local time

describe("formatListDate", () => {
  it("shows the time for today", () => {
    const out = formatListDate(new Date(2026, 10, 18, 9, 5).getTime(), now, "en-GB");
    expect(out).toBe("9:05");
  });

  it("shows Yesterday", () => {
    expect(formatListDate(new Date(2026, 10, 17, 23, 59).getTime(), now, "en-GB")).toBe("Yesterday");
  });

  it("shows the weekday within the last week", () => {
    expect(formatListDate(new Date(2026, 10, 13, 12).getTime(), now, "en-GB")).toBe("Friday");
  });

  it("shows a date for older notes", () => {
    expect(formatListDate(new Date(2026, 10, 11, 12).getTime(), now, "en-GB")).toBe("11/11/2026");
  });

  it("treats future timestamps (clock skew) as today", () => {
    expect(formatListDate(new Date(2026, 10, 19, 1).getTime(), now, "en-GB")).toBe("1:00");
  });
});

describe("daysUntilPurge", () => {
  const day = 24 * 60 * 60 * 1000;
  it("counts down from 30 days", () => {
    expect(daysUntilPurge(0, 0)).toBe(30);
    expect(daysUntilPurge(0, 29.5 * day)).toBe(1);
    expect(daysUntilPurge(0, 31 * day)).toBe(0);
  });
});

describe("purgeLabel", () => {
  const day = 24 * 60 * 60 * 1000;
  it("pluralises", () => {
    expect(purgeLabel(0, 0)).toBe("30 days");
    expect(purgeLabel(0, 29.5 * day)).toBe("1 day");
  });
});
