/** Coordinates rich-text file actions, unsaved changes, and ordered recovery writes. */
import { computed, onScopeDispose, ref, shallowRef } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import type { Editor } from "@tiptap/vue-3";
import type { JSONContent } from "@tiptap/core";

import { EditorFailure, type EditorOperation } from "../../api/markdown-editor";
import { visualEditor, type VisualFile, type VisualSnapshot } from "../../api/visual-editor";
import { visualNotice } from "../../const/visual-editor/errors";
import { emptyVisualContent, validateVisualDocument } from "../../utils/visual-editor/visual-document";
import { notify } from "../use-notifications";

export type VisualCloseChoice = "save" | "discard" | "cancel";

export function useVisualSession(
  current: () => Editor | undefined,
  replaceContent: (content: JSONContent) => void,
  ask: (name: string) => Promise<VisualCloseChoice>,
  remember: (path: string, excerpt: string) => void,
  missing: (path: string) => void,
) {
  const file = shallowRef<VisualFile | null>(null);
  const pendingDraft = shallowRef<VisualSnapshot>();
  const dirty = ref(false);
  const busy = ref(false);
  const externalChange = ref(false);
  const recoveryError = ref("");
  const native = isTauri();
  const name = computed(() => file.value?.path.split(/[\\/]/).pop() ?? "Untitled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let writes = Promise.resolve();
  let discardOnClose = false;

  // Serialize draft writes so a late timer cannot overwrite a deliberate clear.
  function queueDraft(snapshot: VisualSnapshot | null) {
    clearTimeout(timer);
    if (!native) return writes;
    writes = writes.then(async () => {
      try {
        await visualEditor.writeDraft(snapshot);
        recoveryError.value = "";
      } catch (cause) {
        const notice = visualNotice(cause, "recoveryWrite");
        if (!recoveryError.value) notify(notice);
        recoveryError.value = notice.description;
      }
    });
    return writes;
  }

  function snapshot(): VisualSnapshot {
    return { document: { version: 1, content: current()!.getJSON() }, file: file.value };
  }

  // Batch quick edits while retaining the complete file and its original fingerprint.
  function edited() {
    dirty.value = true;
    if (!native) return;
    clearTimeout(timer);
    timer = setTimeout(() => void queueDraft(snapshot()), 500);
  }

  // Apply native errors in the owning view and serialize file actions and confirmation.
  async function run(operation: EditorOperation, action: () => Promise<boolean>) {
    if (busy.value || !current()) return false;
    busy.value = true;
    try {
      return await action();
    } catch (cause) {
      const notice = visualNotice(cause, operation);
      externalChange.value ||= notice.code === "externalChange";
      notify(notice);
      return false;
    } finally {
      busy.value = false;
    }
  }

  function adopt(next: VisualSnapshot, recovered = false) {
    const editor = current()!;
    validateVisualDocument(next.document, editor.schema);
    replaceContent(next.document.content);
    file.value = next.file;
    dirty.value = recovered;
    externalChange.value = false;
  }

  // Save the current draft first so a cancelled picker or a failed write preserves recoverability.
  async function write(saveAs = false) {
    const next = snapshot();
    if (dirty.value) await queueDraft(next);
    const saved = await visualEditor.save(next.document, next.file, saveAs);
    if (!saved) return false;
    file.value = saved;
    dirty.value = false;
    externalChange.value = false;
    await queueDraft(null);
    remember(saved.path, current()!.getText());
    notify({ severity: "success", title: "Visual document saved" });
    return true;
  }

  async function mayReplace() {
    if (!dirty.value) return true;
    const choice = await ask(name.value);
    return choice === "discard" || (choice === "save" && await write());
  }

  // Read and validate a candidate before clearing the draft or changing the editing surface.
  function open(path?: string, onOpened?: (path: string) => void) {
    return run(path ? "open" : "reset", async () => {
      if (pendingDraft.value) return true;
      let next: VisualSnapshot = { document: { version: 1, content: emptyVisualContent() }, file: null };
      if (path) {
        try {
          next = await visualEditor.open(path);
        } catch (cause) {
          if (cause instanceof EditorFailure && cause.code === "fileNotFound") missing(path);
          throw cause;
        }
        validateVisualDocument(next.document, current()!.schema);
      }
      if (!(await mayReplace())) return false;
      if (path && next.file?.path === file.value?.path) next = await visualEditor.open(path);
      clearTimeout(timer);
      await writes;
      if (native && dirty.value) await visualEditor.writeDraft(null);
      adopt(next);
      if (next.file) {
        remember(next.file.path, current()!.getText());
        onOpened?.(next.file.path);
      }
      return true;
    });
  }

  const initialize = () => native && run("recoveryRead", async () => {
    pendingDraft.value = await visualEditor.readDraft() ?? undefined;
    return true;
  });

  function resolveDraft(restore: boolean) {
    return run("recoveryRead", async () => {
      if (!pendingDraft.value) return false;
      if (restore) adopt(pendingDraft.value, true);
      else await visualEditor.writeDraft(null);
      pendingDraft.value = undefined;
      return true;
    });
  }

  // Clear a discarded draft only once the shell confirms both editors can close.
  function requestClose() {
    discardOnClose = false;
    if (busy.value) return Promise.resolve(false);
    if (!dirty.value) return Promise.resolve(true);
    return run("save", async () => {
      const choice = await ask(name.value);
      discardOnClose = choice === "discard";
      return discardOnClose || (choice === "save" && await write());
    });
  }

  async function finishClose() {
    if (!discardOnClose || !native) return;
    clearTimeout(timer);
    await writes;
    await visualEditor.writeDraft(null);
  }

  onScopeDispose(() => clearTimeout(timer));

  return {
    name, native, dirty, busy, pendingDraft, externalChange, recoveryError,
    edited, initialize, open, resolveDraft, requestClose, finishClose,
    save: (saveAs = false) => run(saveAs ? "saveAs" : "save", () => write(saveAs)),
    flush: () => { if (dirty.value && current()) void queueDraft(snapshot()); },
  };
}
