/** Coordinates document edits, native file actions, and ordered recovery writes. */
import { computed, onScopeDispose, ref } from "vue";

import { isTauri } from "@tauri-apps/api/core";

import { EditorFailure, markdownEditor, type OpenDocument } from "../../api/markdown-editor";
import { formatVisualMarkdown, markdownImageSources } from "../../utils/markdown-editor/markdown-format";

import { editorNotice } from "../../const/markdown-editor/errors";
import { notify } from "../use-notifications";

const RECOVERY_DELAY = 500;

export function useDocumentSession(
  serializeVisual: () => string | undefined,
  recordFile: (path: string, markdown: string) => void,
) {
  const name = ref("Untitled");
  const documentKind = ref<"Untitled" | "FileBacked">("Untitled");
  const mode = ref<"source" | "visual">("source");
  const buffer = ref("");
  const saved = ref<string | null>("");
  const visualEdited = ref(false);
  const operation = ref<"Idle" | "Opening" | "Saving" | "Confirming">("Idle");
  const busy = computed(() => operation.value !== "Idle");
  const snapshotState = ref<"Idle" | "Pending" | "Writing" | "Failed">("Idle");
  const revision = ref(0);
  const error = ref("");
  const recoveryError = ref("");
  const externalChange = ref(false);
  const status = ref("");
  // A pending Visual edit makes the session dirty even before Markdown is serialized.
  const dirty = computed(() => visualEdited.value || buffer.value !== saved.value);
  let recoveryTimer: ReturnType<typeof setTimeout> | undefined;
  let recoveryQueue = Promise.resolve();

  // Replace the session baseline and reset editor history for the new document.
  function adopt(document?: OpenDocument, recovered = false) {
    name.value = document?.name ?? "Untitled";
    documentKind.value = document?.fileBacked ? "FileBacked" : "Untitled";
    buffer.value = document?.markdown ?? "";
    saved.value = recovered ? null : buffer.value;
    mode.value = "source";
    visualEdited.value = false;
    externalChange.value = false;
    status.value = recovered ? "Draft restored" : "";
    revision.value += 1;
  }

  // Serialize Visual edits only when an operation needs the shared Markdown buffer.
  function materialize() {
    if (!visualEdited.value) {
      return;
    }

    let markdown: string | undefined;
    try {
      markdown = serializeVisual();
    } catch (cause) {
      throw new EditorFailure("visualRoundTripFailed", "visual", cause);
    }
    if (markdown === undefined) {
      throw new EditorFailure("visualRoundTripFailed", "visual");
    }

    buffer.value = formatVisualMarkdown(markdown, buffer.value);
    visualEdited.value = false;
  }

  // Report a recovery outage once, keeping its status visible until a later write succeeds.
  function reportRecovery(cause: unknown) {
    const code = cause instanceof EditorFailure ? cause.code : "recoveryFailed";
    const failure = new EditorFailure(code, "recoveryWrite", cause);
    const notice = editorNotice(failure, "recoveryWrite");
    if (!recoveryError.value) notify(notice);
    recoveryError.value = `${notice.title}. ${notice.description}`;
    snapshotState.value = "Failed";
  }

  // Order snapshot writes and clears so a late write cannot restore a discarded draft.
  function queueRecovery() {
    if (!isTauri()) return recoveryQueue;
    clearTimeout(recoveryTimer);
    recoveryTimer = undefined;
    try {
      materialize();
      const snapshot = dirty.value ? buffer.value : null;
      recoveryQueue = recoveryQueue.then(async () => {
        snapshotState.value = "Writing";
        try {
          await markdownEditor.updateRecovery(snapshot);
          recoveryError.value = "";
          snapshotState.value = recoveryTimer ? "Pending" : "Idle";
        } catch (cause) {
          reportRecovery(cause);
        }
      });
    } catch (cause) {
      reportRecovery(cause);
    }
    return recoveryQueue;
  }

  // Restart the quiet-period timer so typing does not write a snapshot on every key.
  function scheduleRecovery() {
    clearTimeout(recoveryTimer);
    recoveryTimer = undefined;
    snapshotState.value = "Pending";
    recoveryTimer = setTimeout(() => void queueRecovery(), RECOVERY_DELAY);
  }

  // Source edits already arrive as Markdown and can update the buffer immediately.
  function editSource(markdown: string) {
    buffer.value = markdown;
    status.value = "";
    scheduleRecovery();
  }

  // Defer serialization while still marking the Visual edit as unsaved.
  function editVisual() {
    visualEdited.value = true;
    status.value = "";
    scheduleRecovery();
  }

  // Serialize user actions and keep command failures available to the UI.
  async function run(
    action: () => Promise<void>,
    phase: "Opening" | "Saving" | "Confirming" = "Confirming",
  ) {
    if (busy.value) {
      return;
    }
    operation.value = phase;
    error.value = "";
    try {
      await action();
    } catch (cause) {
      const fallback = phase === "Opening" ? "open" : phase === "Saving" ? "save" : "reset";
      const notice = editorNotice(cause, fallback);
      error.value = `${notice.title}. ${notice.description}`;
      notify(notice);
      if (notice.code === "externalChange") externalChange.value = true;
    } finally {
      operation.value = "Idle";
    }
  }

  // Advance the saved baseline only after Rust reports a completed write.
  async function save(saveAs = false): Promise<boolean> {
    const previousOperation = operation.value;
    operation.value = "Saving";
    try {
      materialize();
      if (!saveAs && documentKind.value === "FileBacked" && !dirty.value) {
        status.value = "Saved";
        return true;
      }

      // Persist the current draft before a dialog or save, then serialize all recovery writes.
      if (dirty.value) {
        await queueRecovery();
      } else {
        await recoveryQueue;
      }
      // Include image references from both modes so Save As can publish attachments.
      const storedPath = await markdownEditor.save(
        buffer.value,
        saveAs,
        markdownImageSources(buffer.value),
      );
      if (storedPath === null) {
        return false;
      }
      name.value = storedPath.replace(/^.*[\\/]/, "");
      recordFile(storedPath, buffer.value);
      documentKind.value = "FileBacked";
      saved.value = buffer.value;
      externalChange.value = false;
      notify({ severity: "success", title: "Document saved" });

      return true;
    } finally {
      operation.value = previousOperation;
    }
  }

  // Cancel the pending timer and wait for queued IPC before resetting the native session.
  async function settleRecovery() {
    clearTimeout(recoveryTimer);
    recoveryTimer = undefined;
    await recoveryQueue;
  }

  // Mark the native picker as opening and restore the enclosing operation on return.
  async function open(shortcut?: string) {
    const previousOperation = operation.value;
    operation.value = "Opening";
    try {
      return await markdownEditor.open(shortcut);
    } finally {
      operation.value = previousOperation;
    }
  }

  onScopeDispose(() => clearTimeout(recoveryTimer));

  return {
    name,
    documentKind,
    operation,
    snapshotState,
    mode,
    buffer,
    busy,
    revision,
    error,
    recoveryError,
    externalChange,
    status,
    dirty,
    adopt,
    editSource,
    editVisual,
    materialize,
    queueRecovery,
    run,
    save,
    settleRecovery,
    open,
  };
}
