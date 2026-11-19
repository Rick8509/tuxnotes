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

/** "512 B", "1.2 MB" … */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/**
 * Apple-Notes-style section for a note in the list:
 * "Today", "Yesterday", "Previous 7 Days", "Previous 30 Days",
 * then the month name for this year ("October") and the year for older notes ("2025").
 */
export function dateSection(timestamp: number, now: Date = new Date(), locale?: string): string {
  const date = new Date(timestamp);
  const daysAgo = Math.round((startOfDay(now) - startOfDay(date)) / DAY_MS);
  if (daysAgo <= 0) return "Today";
  if (daysAgo === 1) return "Yesterday";
  if (daysAgo < 7) return "Previous 7 Days";
  if (daysAgo < 30) return "Previous 30 Days";
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString(locale, { month: "long" });
  }
  return String(date.getFullYear());
}

/** Split an already-sorted list into consecutive sections by `dateSection` of `key` (default: last edit). */
export function groupByDate<T extends { updatedAt: number }>(
  notes: T[],
  now: Date = new Date(),
  locale?: string,
  key: (note: T) => number = (note) => note.updatedAt,
): { label: string; notes: T[] }[] {
  const groups: { label: string; notes: T[] }[] = [];
  for (const note of notes) {
    const label = dateSection(key(note), now, locale);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.notes.push(note);
    else groups.push({ label, notes: [note] });
  }
  return groups;
}

/** "Today at 2:05 PM", "Yesterday at 9:00 AM", otherwise the full date and time. */
export function formatRelativeDateTime(timestamp: number, now: Date = new Date(), locale?: string): string {
  const date = new Date(timestamp);
  const daysAgo = Math.round((startOfDay(now) - startOfDay(date)) / DAY_MS);
  const time = date.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
  if (daysAgo <= 0) return `Today at ${time}`;
  if (daysAgo === 1) return `Yesterday at ${time}`;
  return formatEditorDate(timestamp, locale);
}
