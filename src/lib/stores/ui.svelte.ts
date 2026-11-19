// Window/layout state: which panes are visible, their widths, and the window mode.
// Persisted in localStorage — it's per-device UI preference, not note data.

const STORAGE_KEY = "tuxnotes-ui-v1";

export const SIDEBAR = { min: 170, max: 360, default: 220 };
export const LIST = { min: 220, max: 480, default: 300 };
/** The editor never gets narrower than this while side panes are shown. */
export const EDITOR_MIN = 360;

interface Persisted {
  sidebarVisible: boolean;
  listVisible: boolean;
  sidebarWidth: number;
  listWidth: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

class UiState {
  /** User wants the folders sidebar (may still be auto-hidden when the window is narrow). */
  sidebarVisible = $state(true);
  /** Note list shown. Hiding it is "focus mode": the sidebar hides too. */
  listVisible = $state(true);
  sidebarWidth = $state(SIDEBAR.default);
  listWidth = $state(LIST.default);
  /** Sidebar temporarily floating over the list (narrow windows). */
  sidebarOverlay = $state(false);
  windowWidth = $state(typeof window === "undefined" ? 1200 : window.innerWidth);
  maximized = $state(false);
  fullscreen = $state(false);
  /** True while a divider is being dragged (disables the column animation). */
  resizing = $state(false);

  constructor() {
    try {
      const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const p = JSON.parse(raw) as Partial<Persisted>;
        this.sidebarVisible = p.sidebarVisible ?? true;
        this.listVisible = p.listVisible ?? true;
        this.sidebarWidth = clamp(p.sidebarWidth ?? SIDEBAR.default, SIDEBAR.min, SIDEBAR.max);
        this.listWidth = clamp(p.listWidth ?? LIST.default, LIST.min, LIST.max);
      }
    } catch {
      /* corrupt prefs: keep defaults */
    }
  }

  private save() {
    const p: Persisted = {
      sidebarVisible: this.sidebarVisible,
      listVisible: this.listVisible,
      sidebarWidth: this.sidebarWidth,
      listWidth: this.listWidth,
    };
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(p));
    } catch {
      /* storage unavailable: preferences just won't persist */
    }
  }

  /** Window too narrow to show the sidebar inline next to the list and editor. */
  get narrow(): boolean {
    return this.windowWidth < this.sidebarWidth + this.listWidth + EDITOR_MIN;
  }

  /** Sidebar takes up a column in the layout. */
  get sidebarInline(): boolean {
    return this.listVisible && this.sidebarVisible && !this.narrow;
  }

  /** Sidebar is on screen at all (inline or floating). */
  get sidebarShown(): boolean {
    return this.sidebarInline || this.sidebarOverlay;
  }

  /** Square corners when the window fills the screen. */
  get edgeToEdge(): boolean {
    return this.maximized || this.fullscreen;
  }

  get columns(): string {
    const side = this.sidebarInline ? this.sidebarWidth : 0;
    const list = this.listVisible ? this.listWidth : 0;
    return `${side}px ${list}px minmax(0, 1fr)`;
  }

  toggleSidebar() {
    if (!this.listVisible) {
      // Leaving focus mode via the sidebar button: bring both panes back.
      this.listVisible = true;
      this.sidebarVisible = true;
    } else if (this.narrow) {
      this.sidebarOverlay = !this.sidebarOverlay;
    } else {
      this.sidebarVisible = !this.sidebarVisible;
    }
    this.save();
  }

  /** Focus mode: hide the sidebar and note list to give the editor the whole window. */
  toggleFocus() {
    this.listVisible = !this.listVisible;
    this.sidebarOverlay = false;
    this.save();
  }

  closeOverlay() {
    this.sidebarOverlay = false;
  }

  setSidebarWidth(px: number) {
    const max = Math.min(SIDEBAR.max, this.windowWidth - this.listWidth - EDITOR_MIN);
    this.sidebarWidth = clamp(Math.round(px), SIDEBAR.min, Math.max(SIDEBAR.min, max));
  }

  setListWidth(px: number) {
    const side = this.sidebarInline ? this.sidebarWidth : 0;
    const max = Math.min(LIST.max, this.windowWidth - side - EDITOR_MIN);
    this.listWidth = clamp(Math.round(px), LIST.min, Math.max(LIST.min, max));
  }

  resetWidths() {
    this.sidebarWidth = SIDEBAR.default;
    this.listWidth = LIST.default;
    this.save();
  }

  endResize() {
    this.resizing = false;
    this.save();
  }
}

export const ui = new UiState();
