import { computed, onScopeDispose, ref } from "vue";

import { markdownEditor, type OpenDocument } from "../../api/markdown-editor";
import { formatVisualMarkdown } from "../../utils/markdown-editor/markdown-format";

const RECOVERY_DELAY = 500;

function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return typeof error === "string" ? error : "The editor action could not be completed.";
}

export function useDocumentSession(serializeVisual: () => string | undefined) {
  const name = ref("Untitled");
  const hasFile = ref(false);
  const mode = ref<"source" | "visual">("source");
  const buffer = ref("");
  const saved = ref<string | null>("");
  const visualEdited = ref(false);
  const busy = ref(false);
  const revision = ref(0);
  const error = ref("");
  const recoveryError = ref("");
  const externalChange = ref(false);
  const status = ref("");
  const dirty = computed(() => visualEdited.value || buffer.value !== saved.value);
  let recoveryTimer: ReturnType<typeof setTimeout> | undefined;
  let recoveryQueue = Promise.resolve();

  function adopt(document?: OpenDocument, recovered = false) {
    name.value = document?.name ?? "Untitled";
    hasFile.value = Boolean(document && document.name !== "Untitled");
    buffer.value = document?.markdown ?? "";
    saved.value = recovered ? null : buffer.value;
    mode.value = "source";
    visualEdited.value = false;
    externalChange.value = false;
    status.value = recovered ? "Draft restored" : "";
    revision.value += 1;
  }

  function materialize() {
    if (!visualEdited.value) {
      return;
    }

    const markdown = serializeVisual();
    if (markdown === undefined) {
      throw new Error("The Visual editor could not produce Markdown. Your edits are still open.");
    }

    buffer.value = formatVisualMarkdown(markdown, buffer.value);
    visualEdited.value = false;
  }

  function queueRecovery() {
    clearTimeout(recoveryTimer);
    try {
      materialize();
      const snapshot = dirty.value ? buffer.value : null;
      recoveryQueue = recoveryQueue.then(async () => {
        try {
          await markdownEditor.updateRecovery(snapshot);
          recoveryError.value = "";
        } catch (cause) {
          recoveryError.value = "Draft recovery could not be updated. " + errorMessage(cause);
        }
      });
    } catch (cause) {
      recoveryError.value = errorMessage(cause);
    }
    return recoveryQueue;
  }

  function scheduleRecovery() {
    clearTimeout(recoveryTimer);
    recoveryTimer = setTimeout(() => void queueRecovery(), RECOVERY_DELAY);
  }

  function editSource(markdown: string) {
    buffer.value = markdown;
    status.value = "";
    scheduleRecovery();
  }

  function editVisual() {
    visualEdited.value = true;
    status.value = "";
    scheduleRecovery();
  }

  async function run(action: () => Promise<void>) {
    if (busy.value) {
      return;
    }
    busy.value = true;
    error.value = "";
    try {
      await action();
    } catch (cause) {
      error.value = errorMessage(cause);
      if (
        cause &&
        typeof cause === "object" &&
        "code" in cause &&
        cause.code === "externalChange"
      ) {
        externalChange.value = true;
      }
    } finally {
      busy.value = false;
    }
  }

  async function save(saveAs = false): Promise<boolean> {
    materialize();
    if (!saveAs && hasFile.value && !dirty.value) {
      status.value = "Saved";
      return true;
    }

    // Persist the current draft before a dialog or save, then serialize all recovery writes.
    if (dirty.value) {
      await queueRecovery();
    } else {
      await recoveryQueue;
    }
    const storedName = await markdownEditor.save(buffer.value, saveAs);
    if (storedName === null) {
      return false;
    }
    name.value = storedName;
    hasFile.value = true;
    saved.value = buffer.value;
    externalChange.value = false;
    status.value = "Saved";
    await queueRecovery();
    return true;
  }

  async function settleRecovery() {
    clearTimeout(recoveryTimer);
    await recoveryQueue;
  }

  onScopeDispose(() => clearTimeout(recoveryTimer));

  return {
    name,
    hasFile,
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
  };
}
