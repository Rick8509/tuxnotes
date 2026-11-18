// Note-list dates, styled like Apple Notes:
// today → "14:05", yesterday → "Yesterday", this week → "Tuesday", older → "12/03/2026".

const DAY_MS = 24 * 60 * 60 * 1000;

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

export function formatListDate(timestamp: number, now: Date = new Date(), locale?: string): string {
  const date = new Date(timestamp);
  const daysAgo = Math.round((startOfDay(now) - startOfDay(date)) / DAY_MS);

  if (daysAgo <= 0) {
    return date.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
  }
  if (daysAgo === 1) return "Yesterday";
  if (daysAgo < 7) return date.toLocaleDateString(locale, { weekday: "long" });
  return date.toLocaleDateString(locale, { year: "numeric", month: "2-digit", day: "2-digit" });
}

/** Full timestamp shown at the top of the editor, e.g. "18 November 2026 at 14:05". */
export function formatEditorDate(timestamp: number, locale?: string): string {
  return new Date(timestamp).toLocaleString(locale, { dateStyle: "long", timeStyle: "short" });
}

/** Days left before a trashed note is purged (minimum 0). */
export function daysUntilPurge(deletedAt: number, now: number = Date.now(), retentionDays = 30): number {
  return Math.max(0, Math.ceil((deletedAt + retentionDays * DAY_MS - now) / DAY_MS));
}

/** "30 days", "1 day", "0 days" — shown on rows in Recently Deleted. */
export function purgeLabel(deletedAt: number, now: number = Date.now()): string {
  const days = daysUntilPurge(deletedAt, now);
  return `${days} day${days === 1 ? "" : "s"}`;
}
