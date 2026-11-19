import { describe, expect, it } from "vitest";
import { dateSection, daysUntilPurge, formatBytes, formatListDate, formatRelativeDateTime, groupByDate, purgeLabel } from "./format";

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

describe("formatBytes", () => {
  it("picks sensible units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(25 * 1024 * 1024)).toBe("25 MB");
    expect(formatBytes(3 * 1024 ** 3)).toBe("3.0 GB");
  });
});

describe("dateSection", () => {
  const at = (y: number, m: number, d: number, h = 12) => new Date(y, m, d, h).getTime();
  it("uses Apple Notes' buckets", () => {
    expect(dateSection(at(2026, 10, 18, 1), now)).toBe("Today");
    expect(dateSection(at(2026, 10, 17, 23), now)).toBe("Yesterday");
    expect(dateSection(at(2026, 10, 12), now)).toBe("Previous 7 Days");
    expect(dateSection(at(2026, 10, 11), now)).toBe("Previous 30 Days");
    expect(dateSection(at(2026, 9, 20), now)).toBe("Previous 30 Days");
    expect(dateSection(at(2026, 9, 1), now, "en-GB")).toBe("October");
    expect(dateSection(at(2026, 0, 1), now, "en-GB")).toBe("January");
    expect(dateSection(at(2025, 11, 31), now)).toBe("2025");
  });
});

describe("groupByDate", () => {
  it("groups consecutive notes and keeps order", () => {
    const day = 24 * 60 * 60 * 1000;
    const t = now.getTime();
    const notes = [t - 1000, t - 2000, t - day, t - 3 * day, t - 4 * day].map((updatedAt, i) => ({ id: i, updatedAt }));
    const groups = groupByDate(notes, now);
    expect(groups.map((g) => [g.label, g.notes.map((n) => n.id)])).toEqual([
      ["Today", [0, 1]],
      ["Yesterday", [2]],
      ["Previous 7 Days", [3, 4]],
    ]);
    expect(groupByDate([], now)).toEqual([]);
  });

  it("can group by another date, e.g. creation", () => {
    const day = 24 * 60 * 60 * 1000;
    const t = now.getTime();
    const notes = [{ updatedAt: t, createdAt: t - 3 * day }];
    expect(groupByDate(notes, now, undefined, (n) => n.createdAt)[0].label).toBe("Previous 7 Days");
  });
});

describe("formatRelativeDateTime", () => {
  it("uses Today / Yesterday, then the full date", () => {
    expect(formatRelativeDateTime(new Date(2026, 10, 18, 9, 5).getTime(), now, "en-GB")).toBe("Today at 9:05");
    expect(formatRelativeDateTime(new Date(2026, 10, 17, 21, 0).getTime(), now, "en-GB")).toBe("Yesterday at 21:00");
    expect(formatRelativeDateTime(new Date(2026, 10, 10, 8, 0).getTime(), now, "en-GB")).toContain("2026");
  });
});
