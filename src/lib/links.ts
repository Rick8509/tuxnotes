// Turning what people type into a safe link, and showing links compactly.

/**
 * Normalise user input into a link we're willing to create, or null if it isn't one.
 * Accepts http(s), mailto and tel links; bare domains get https://, bare emails mailto:.
 * Anything else (javascript:, file:, data: …) is refused.
 */
export function normalizeUrl(input: string): string | null {
  const v = input.trim();
  if (!v || /\s/.test(v)) return null;
  if (/^(https?:\/\/)[^\s/]+/i.test(v)) return v;
  if (/^(mailto|tel):\S+$/i.test(v)) return v;
  if (/^[a-z][a-z0-9+.-]*:/i.test(v) && !/^[^:]+:\d+([/?#]|$)/.test(v)) return null; // other schemes
  if (/^[^@/]+@[^@\s]+\.[a-z]{2,}$/i.test(v)) return `mailto:${v}`;
  if (/^(localhost|[\w-]+(\.[\w-]+)+)(:\d+)?([/?#].*)?$/i.test(v)) return `https://${v}`;
  return null;
}

/** Short form for display: no "https://", no trailing slash, truncated in the middle. */
export function displayUrl(href: string, max = 48): string {
  let s = href.replace(/^https?:\/\//i, "").replace(/^mailto:/i, "").replace(/\/$/, "");
  if (s.length > max) s = `${s.slice(0, max - 16)}…${s.slice(-15)}`;
  return s;
}

/** Links we'll open in the browser (or mail app). */
export function isOpenableUrl(href: string): boolean {
  return /^(https?|mailto|tel):/i.test(href);
}
