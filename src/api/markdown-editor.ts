import { invoke } from "@tauri-apps/api/core";

export interface OpenDocument {
  name: string;
  markdown: string;
}

export const markdownEditor = {
  open: () => invoke<OpenDocument | null>("editor_open"),
  save: (markdown: string, saveAs: boolean) =>
    invoke<string | null>("editor_save", { markdown, saveAs }),
  reset: () => invoke<void>("editor_reset"),
  recoveryInfo: () => invoke<string | null>("editor_recovery_info"),
  restore: () => invoke<OpenDocument>("editor_restore"),
  updateRecovery: (markdown: string | null) => invoke<void>("editor_update_recovery", { markdown }),
};
