// User preferences from the Settings window (General + Appearance).
// Stored in localStorage: they're about how this computer shows notes, not note data.

const STORAGE_KEY = "tuxnotes-prefs-v1";

export type Theme = "system" | "light" | "dark";
export type TextSize = "small" | "medium" | "large" | "xlarge";
export type SortBy = "edited" | "created" | "title";
export type FirstLine = "title" | "heading" | "body";

/** Editor body font size in px for each text size. Headings scale with it. */
export const TEXT_SIZE_PX: Record<TextSize, number> = { small: 13, medium: 15, large: 17, xlarge: 20 };

interface Persisted {
  theme: Theme;
  textSize: TextSize;
  sortBy: SortBy;
  groupByDate: boolean;
  firstLine: FirstLine;
}

const DEFAULTS: Persisted = {
  theme: "system",
  textSize: "medium",
  sortBy: "edited",
  groupByDate: true,
  firstLine: "title",
};

const pick = <T extends string>(value: unknown, allowed: readonly T[], fallback: T): T =>
  allowed.includes(value as T) ? (value as T) : fallback;

function load(): Persisted {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULTS };
    const p = JSON.parse(raw) as Partial<Persisted>;
    return {
      theme: pick(p.theme, ["system", "light", "dark"], DEFAULTS.theme),
      textSize: pick(p.textSize, ["small", "medium", "large", "xlarge"], DEFAULTS.textSize),
      sortBy: pick(p.sortBy, ["edited", "created", "title"], DEFAULTS.sortBy),
      groupByDate: typeof p.groupByDate === "boolean" ? p.groupByDate : DEFAULTS.groupByDate,
      firstLine: pick(p.firstLine, ["title", "heading", "body"], DEFAULTS.firstLine),
    };
  } catch {
    return { ...DEFAULTS };
  }
}

const darkQuery = typeof window !== "undefined" ? window.matchMedia("(prefers-color-scheme: dark)") : null;

class Prefs {
  theme = $state<Theme>(DEFAULTS.theme);
  textSize = $state<TextSize>(DEFAULTS.textSize);
  sortBy = $state<SortBy>(DEFAULTS.sortBy);
  groupByDate = $state(DEFAULTS.groupByDate);
  firstLine = $state<FirstLine>(DEFAULTS.firstLine);
  /** Follows the desktop's dark-mode setting live. */
  systemDark = $state(darkQuery?.matches ?? false);

  constructor() {
    Object.assign(this, load());
    darkQuery?.addEventListener("change", (e) => {
      this.systemDark = e.matches;
      this.applyTheme();
    });
    this.applyTheme();
  }

  get effectiveTheme(): "light" | "dark" {
    return this.theme === "system" ? (this.systemDark ? "dark" : "light") : this.theme;
  }

  /** Put the theme on <html> so CSS (and native form controls) pick it up. */
  applyTheme() {
    if (typeof document === "undefined") return;
    document.documentElement.dataset.theme = this.effectiveTheme;
  }

  set<K extends keyof Persisted>(key: K, value: Persisted[K]) {
    (this as unknown as Persisted)[key] = value;
    if (key === "theme") this.applyTheme();
    const p: Persisted = {
      theme: this.theme,
      textSize: this.textSize,
      sortBy: this.sortBy,
      groupByDate: this.groupByDate,
      firstLine: this.firstLine,
    };
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(p));
    } catch {
      /* storage unavailable: the choice just won't be remembered */
    }
  }
}

export const prefs = new Prefs();
