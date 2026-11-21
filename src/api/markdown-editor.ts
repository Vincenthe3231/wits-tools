/** Validates the native editor error contract and attaches the operation that failed. */
import { invoke } from "@tauri-apps/api/core";

import { errorCode } from "../utils/errors";

export interface OpenDocument {
  name: string;
  markdown: string;
  fileBacked: boolean;
}

// Keep accepted Rust identifiers explicit and distinguish failures owned by Vue.
const nativeCodes = [
  "invalidPath", "invalidDestination", "fileNotFound", "permissionDenied", "ioFailed",
  "writeFailed", "unsupportedFileType", "fileTooLarge", "invalidEncoding", "externalChange",
  "unsupportedImageType", "imageTooLarge", "imagePreviewUnavailable", "imageChanged",
  "imageConflict", "recoveryFailed", "sessionUnavailable",
] as const;

export type EditorErrorCode = (typeof nativeCodes)[number]
  | "imageStageFailed" | "clipboardWriteFailed" | "visualUnsupportedContent"
  | "visualRoundTripFailed" | "unexpected";

export type EditorOperation =
  | "open" | "save" | "saveAs" | "reset" | "recoveryRead" | "recoveryWrite"
  | "pasteImage" | "insertImage" | "previewImage" | "copyImage" | "visual";

export class EditorFailure extends Error {
  // Retain unknown causes for debugging while exposing only typed fields to presentation.
  constructor(
    readonly code: EditorErrorCode,
    readonly operation: EditorOperation,
    readonly cause?: unknown,
  ) {
    super(code);
  }
}

// Attach operation context once and leave notification ownership to the caller.
async function editorInvoke<T>(
  operation: EditorOperation,
  command: string,
  args?: Parameters<typeof invoke>[1],
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    const code = errorCode(cause, nativeCodes);
    throw new EditorFailure(code, operation, cause);
  }
}

export const markdownEditor = {
  open: () => editorInvoke<OpenDocument | null>("open", "editor_open"),
  save: (markdown: string, saveAs: boolean, imageSources: string[]) =>
    editorInvoke<string | null>(saveAs ? "saveAs" : "save", "editor_save", {
      markdown, saveAs, imageSources,
    }),
  reset: () => editorInvoke<void>("reset", "editor_reset"),
  recoveryInfo: () => editorInvoke<string | null>("recoveryRead", "editor_recovery_info"),
  restore: () => editorInvoke<OpenDocument>("recoveryRead", "editor_restore"),
  updateRecovery: (markdown: string | null) =>
    editorInvoke<void>("recoveryWrite", "editor_update_recovery", { markdown }),
};

// Use raw IPC for image bytes so clipboard and preview do not expand them into JSON arrays.
export const imageTransfer = {
  stage: (bytes: ArrayBuffer, operation: "pasteImage" | "insertImage") =>
    editorInvoke<string>(operation, "editor_stage_image", bytes),
  read: (src: string) => editorInvoke<ArrayBuffer>("previewImage", "editor_read_image", { src }),
};
