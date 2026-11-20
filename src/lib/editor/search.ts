// Find in note (Ctrl+F) and highlighting search words in the open note.
// Matching ignores case and accents ("cafe" finds "Café"), like the full-text search.

import { Extension } from "@tiptap/core";
import type { Node as PMNode } from "@tiptap/pm/model";
import { Plugin, PluginKey, TextSelection, type EditorState } from "@tiptap/pm/state";
import { Decoration, DecorationSet, type EditorView } from "@tiptap/pm/view";

/** "find": the whole query as typed. "terms": each word, matched at the start of words (like search). */
export type SearchMode = "find" | "terms";

export interface Match {
  from: number;
  to: number;
}

interface SearchState {
  query: string;
  mode: SearchMode;
  matches: Match[];
  current: number;
  decorations: DecorationSet;
}

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
const isWordChar = (c: string | undefined) => !!c && /[\p{L}\p{N}]/u.test(c);

/** Case/accent-folded text, plus the original UTF-16 index of every folded character. */
export function foldWithMap(text: string): { folded: string; map: number[] } {
  let folded = "";
  const map: number[] = [];
  for (let i = 0; i < text.length; ) {
    const len = (text.codePointAt(i) ?? 0) > 0xffff ? 2 : 1;
    const f = fold(text.slice(i, i + len));
    // One map entry per UTF-16 unit, so indices line up even for emoji (two units).
    for (let j = 0; j < f.length; j++) {
      folded += f[j];
      map.push(i);
    }
    i += len;
  }
  return { folded, map };
}

/** Ranges [start, end) in `text` (UTF-16 indices) matching `query`. */
/** Letters of scripts usually written without spaces (same ranges as the notes search). */
const UNSPACED = /^(?:[\u3040-\u30FF\u31F0-\u31FF\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF\uFF66-\uFF9F\u0E00-\u0EFF\u1000-\u109F\u1780-\u17FF]|[\u{20000}-\u{2FFFF}])/u;

export function findInText(text: string, query: string, wordStart = false): [number, number][] {
  const q = fold(query);
  if (!q) return [];
  const { folded, map } = foldWithMap(text);
  const out: [number, number][] = [];
  for (let i = folded.indexOf(q); i >= 0; i = folded.indexOf(q, i + 1)) {
    // Scripts written without spaces (Chinese, Japanese, Thai…) have no word starts to
    // match: their words are found anywhere, like the notes search does.
    if (wordStart && isWordChar(folded[i - 1]) && !UNSPACED.test(q)) continue;
    const start = map[i];
    const lastOrig = map[i + q.length - 1];
    let end = lastOrig + ((text.codePointAt(lastOrig) ?? 0) > 0xffff ? 2 : 1);
    // Keep combining accents (e.g. "e" + "\u0301") with the letter they belong to.
    while (end < text.length && /\p{M}/u.test(text[end])) end++;
    if (out.length && start < out[out.length - 1][1]) continue; // no overlapping matches
    out.push([start, end]);
  }
  return out;
}

/** All matches in the document, in order. Matches never cross a line or block. */
export function findMatches(doc: PMNode, query: string, mode: SearchMode): Match[] {
  const terms = mode === "terms" ? query.split(/\s+/).filter(Boolean) : [query.trim()].filter(Boolean);
  if (terms.length === 0) return [];
  const matches: Match[] = [];
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    let text = "";
    const posOf: number[] = [];
    node.forEach((child, offset) => {
      const s = child.isText ? (child.text ?? "") : "\n"; // inline nodes (line breaks) separate text
      for (let i = 0; i < s.length; i++) posOf.push(pos + 1 + offset + (child.isText ? i : 0));
      text += s;
    });
    const found: Match[] = [];
    for (const term of terms) {
      for (const [s, e] of findInText(text, term, mode === "terms")) found.push({ from: posOf[s], to: posOf[e - 1] + 1 });
    }
    found.sort((a, b) => a.from - b.from);
    for (const m of found) if (!matches.length || m.from >= matches[matches.length - 1].to) matches.push(m);
    return false;
  });
  return matches;
}

export const searchKey = new PluginKey<SearchState>("search");

function build(doc: PMNode, query: string, mode: SearchMode, current: number): SearchState {
  const matches = query ? findMatches(doc, query, mode) : [];
  const cur = matches.length ? Math.min(Math.max(current, 0), matches.length - 1) : -1;
  const decorations = DecorationSet.create(
    doc,
    matches.map((m, i) => Decoration.inline(m.from, m.to, { class: i === cur && mode === "find" ? "search-match current" : "search-match" })),
  );
  return { query, mode, matches, current: cur, decorations };
}

export const SearchHighlight = Extension.create({
  name: "searchHighlight",
  addProseMirrorPlugins() {
    return [
      new Plugin<SearchState>({
        key: searchKey,
        state: {
          init: (_config, state) => build(state.doc, "", "find", 0),
          apply(tr, prev, _old, state) {
            const meta = tr.getMeta(searchKey) as Partial<Pick<SearchState, "query" | "mode" | "current">> | undefined;
            if (!meta && !tr.docChanged) return prev;
            const next = { query: prev.query, mode: prev.mode, current: prev.current, ...meta };
            return build(state.doc, next.query, next.mode, next.current);
          },
        },
        props: {
          decorations: (state) => searchKey.getState(state)?.decorations,
        },
      }),
    ];
  },
});

export function getSearch(state: EditorState): SearchState | undefined {
  return searchKey.getState(state);
}

export function setSearch(view: EditorView, query: string, mode: SearchMode, current = 0) {
  view.dispatch(view.state.tr.setMeta(searchKey, { query, mode, current }));
}

/** Select match `index` (wrapping) and scroll it into view. Returns the new index. */
export function goToMatch(view: EditorView, index: number): number {
  const s = getSearch(view.state);
  if (!s || s.matches.length === 0) return -1;
  const i = ((index % s.matches.length) + s.matches.length) % s.matches.length;
  const m = s.matches[i];
  // Select the match, so closing the find bar leaves the cursor right there.
  const tr = view.state.tr.setMeta(searchKey, { current: i }).setSelection(TextSelection.create(view.state.doc, m.from, m.to));
  view.dispatch(tr);
  scrollToPos(view, m.from);
  return i;
}

/** Scroll so that a document position is comfortably visible (centered). */
export function scrollToPos(view: EditorView, pos: number) {
  const { node } = view.domAtPos(pos);
  const el = (node.nodeType === Node.TEXT_NODE ? node.parentElement : node) as HTMLElement | null;
  el?.scrollIntoView({ block: "center" });
}
