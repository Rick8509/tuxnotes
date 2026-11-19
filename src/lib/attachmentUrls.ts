// Where the editor loads attachment files from.
// In the app: the `attachment://` protocol served by Rust (see src-tauri/src/attachments.rs).
// In the browser preview: object URLs registered by the mock backend.

import { convertFileSrc } from "@tauri-apps/api/core";

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const previewUrls = new Map<string, string>();

export function registerPreviewUrl(id: string, url: string) {
  previewUrls.set(id, url);
}

export function attachmentUrl(id: string | null | undefined): string {
  if (!id) return "";
  return isTauri ? convertFileSrc(id, "attachment") : (previewUrls.get(id) ?? "");
}
