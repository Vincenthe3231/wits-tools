/** Defines the Markdown editor's typed calls to the native desktop backend. */
import { invoke } from "@tauri-apps/api/core";

export interface OpenDocument {
  name: string;
  markdown: string;
  fileBacked: boolean;
}

export const markdownEditor = {
  open: () => invoke<OpenDocument | null>("editor_open"),
  save: (markdown: string, saveAs: boolean, imageSources: string[]) =>
    invoke<string | null>("editor_save", { markdown, saveAs, imageSources }),
  reset: () => invoke<void>("editor_reset"),
  recoveryInfo: () => invoke<string | null>("editor_recovery_info"),
  restore: () => invoke<OpenDocument>("editor_restore"),
  updateRecovery: (markdown: string | null) => invoke<void>("editor_update_recovery", { markdown }),
};

// Use raw IPC for image bytes so clipboard and preview do not expand them into JSON arrays.
export const imageTransfer = {
  stage: (bytes: ArrayBuffer) => invoke<string>("editor_stage_image", bytes),
  read: (src: string) => invoke<ArrayBuffer>("editor_read_image", { src }),
};
