/** Connects Visual documents to the native picker, file writes, and crash recovery. */
import type { JSONContent } from "@tiptap/core";

import { editorInvoke } from "./markdown-editor";

export interface VisualDocument {
  version: 1;
  content: JSONContent;
}

export interface VisualFile {
  path: string;
  fingerprint: string;
}

export interface VisualSnapshot {
  document: VisualDocument;
  file: VisualFile | null;
}

// Keep paths and revision fingerprints at the file boundary, outside the rich-text schema.
export const visualEditor = {
  pick: (visualOnly = false) => editorInvoke<string | null>("open", "visual_pick_file", { visualOnly }),
  open: (path: string) => editorInvoke<VisualSnapshot>("open", "visual_open", { path }),
  save: (document: VisualDocument, original: VisualFile | null, saveAs: boolean) =>
    editorInvoke<VisualFile | null>(saveAs ? "saveAs" : "save", "visual_save", { document, original, saveAs }),
  readDraft: () => editorInvoke<VisualSnapshot | null>("recoveryRead", "visual_read_draft"),
  writeDraft: (snapshot: VisualSnapshot | null) =>
    editorInvoke<void>("recoveryWrite", "visual_write_draft", { snapshot }),
};

// Library metadata is optional and is read without opening a document session.
export const editorCreatedTimes = (paths: string[]) =>
  editorInvoke<(number | null)[]>("open", "editor_created_times", { paths });
