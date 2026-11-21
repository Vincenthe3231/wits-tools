<!-- Presents the retained Markdown session and handles editor-local prompts. -->
<script setup lang="ts">
import { computed, nextTick, onMounted, onScopeDispose, ref, watch } from "vue";
import { isTauri } from "@tauri-apps/api/core";

import { EditorFailure, markdownEditor } from "../../api/markdown-editor";
import NavigationTransition from "../../components/NavigationTransition.vue";
import SourceEditor from "../../components/markdown-editor/SourceEditor.vue";
import VisualEditor from "../../components/markdown-editor/VisualEditor.vue";
import { useDocumentSession } from "../../composables/markdown-editor/use-document-session";

import { editorNotice } from "../../const/markdown-editor/errors";
import { notify } from "../../composables/use-notifications";

const props = defineProps<{ active: boolean }>();

const visual = ref<InstanceType<typeof VisualEditor>>();
const source = ref<InstanceType<typeof SourceEditor>>();
const discardDialog = ref<HTMLDialogElement>();
const restoreDialog = ref<HTMLDialogElement>();
type RecoveryState =
  | { phase: "Unchecked" | "Checking" | "Resolved" }
  | { phase: "Available"; name: string };

const recovery = ref<RecoveryState>({ phase: "Unchecked" });
const replacement = ref<"new" | "open">();
const recoveryName = computed(() =>
  recovery.value.phase === "Available" ? recovery.value.name : "",
);
const visualReason = ref("");
const nativeApp = isTauri();
let decision: ((value: string) => void) | undefined;

const session = useDocumentSession(() => visual.value?.serialize());
const { name, mode, buffer, busy, revision, error, recoveryError, externalChange, status, dirty } =
  session;

// Finish staging an image before actions that could save or replace its document.
const working = computed(() => busy.value || Boolean(source.value?.pastingImage));

// Clear a previous conversion result once the underlying Markdown changes.
function editSource(markdown: string) {
  visualReason.value = "";
  session.editSource(markdown);
}

// Validate Markdown before activating Visual and serialize Visual changes before returning to Source.
function switchMode(target: "source" | "visual", focus = true) {
  if (source.value?.pastingImage) return;

  if (target === "visual") {
    if (mode.value === "visual") {
      return;
    }
    if (!visual.value) return;

    const issue = visual.value.prepare(buffer.value);
    visualReason.value = issue?.reason ?? "";
    if (issue) {
      // Automatic mode selection can fall back to Source quietly for unsupported files.
      if (focus || issue.code === "visualRoundTripFailed") {
        const notice = editorNotice(new EditorFailure(issue.code, "visual"), "visual");
        notify({ ...notice, description: issue.reason, id: "markdown-visual-compatibility" });
      }
      return;
    }
    mode.value = "visual";
  } else {
    // End pointer capture and dismiss Visual overlays before hiding the pane.
    visual.value?.dismissInteractions();
    try {
      session.materialize();
    } catch (cause) {
      // Keep the current Visual document available if it cannot serialize.
      notify(editorNotice(cause, "visual"));
      return;
    }
    mode.value = "source";
    revision.value += 1;
    visualReason.value = "";
  }

  if (dirty.value && nativeApp) {
    void session.queueRecovery();
  }
  if (focus) {
    void nextTick(() => (target === "visual" ? visual.value?.focus() : source.value?.focus()));
  }
}

// Prefer Visual for a new session when it can safely represent the document.
function openVisualByDefault() {
  visualReason.value = "";
  switchMode("visual", false);
}

// Pause a document replacement until the user chooses how to handle current edits.
function confirmChanges() {
  const dialog = discardDialog.value;
  if (!dialog) {
    return Promise.resolve("cancel");
  }
  dialog.returnValue = "cancel";
  dialog.showModal();
  return new Promise<string>((resolve) => {
    decision = resolve;
  });
}

// Resume the replacement flow with the dialog result and default to cancel.
function resolveChanges() {
  decision?.(discardDialog.value?.returnValue ?? "cancel");
  decision = undefined;
}

// Allow replacement only after a successful save or an explicit discard choice.
async function mayReplace(action: "new" | "open") {
  if (!dirty.value) {
    return true;
  }
  replacement.value = action;
  const result = await confirmChanges();
  replacement.value = undefined;
  if (result === "save") {
    return session.save();
  }
  return result === "discard";
}

// Keep the current document and recovery data until the replacement actually succeeds.
function changeDocument(action: "new" | "open") {
  return session.run(async () => {
    if (!(await mayReplace(action))) {
      return;
    }
    await session.settleRecovery();
    try {
      if (action === "open") {
        const document = await session.open();
        if (!document) {
          return;
        }
        session.adopt(document);
      } else {
        await markdownEditor.reset();
        session.adopt();
      }
      openVisualByDefault();
    } finally {
      if (dirty.value) {
        void session.queueRecovery();
      }
    }
  });
}

// Run toolbar and shortcut saves through the same operation guard.
function saveDocument(saveAs = false) {
  return session.run(async () => {
    await session.save(saveAs);
  });
}

// Resolve the startup prompt only after Rust restores or clears the snapshot.
function recoveryAction(restore: boolean) {
  return session.run(async () => {
    if (restore) {
      session.adopt(await markdownEditor.restore(), true);
      notify({ severity: "info", title: "Draft restored", description: "Your draft is ready to edit." });
    } else {
      await markdownEditor.reset();
    }
    recovery.value = { phase: "Resolved" };
    restoreDialog.value?.close();
    openVisualByDefault();
  });
}

// Keep editor shortcuts inactive on other routes and while a modal owns input.
function shortcuts(event: KeyboardEvent) {
  if (
    !props.active ||
    (!event.ctrlKey && !event.metaKey) ||
    event.altKey ||
    event.isComposing ||
    restoreDialog.value?.open ||
    discardDialog.value?.open
  ) {
    return;
  }
  const key = event.key.toLowerCase();
  if (!["s", "o", "n"].includes(key)) {
    return;
  }
  event.preventDefault();
  if (working.value || !nativeApp) {
    return;
  }
  if (key === "s") {
    void saveDocument(event.shiftKey);
  } else {
    void changeDocument(key === "o" ? "open" : "new");
  }
}

// Check startup recovery once, when the retained editor first mounts.
onMounted(async () => {
  window.addEventListener("keydown", shortcuts);
  if (!nativeApp) {
    openVisualByDefault();
    return;
  }

  recovery.value = { phase: "Checking" };
  await session.run(async () => {
    const draftName = await markdownEditor.recoveryInfo();
    if (draftName !== null) {
      recovery.value = { phase: "Available", name: draftName };
      restoreDialog.value?.showModal();
    } else {
      recovery.value = { phase: "Resolved" };
      openVisualByDefault();
    }
  });
});

// Keep the editor DOM and history alive while the dashboard is visible.
watch(
  () => props.active,
  (active) => {
    if (!active) {
      visual.value?.dismissInteractions();
    }

    if (!active && nativeApp && dirty.value) {
      void session.queueRecovery();
    }
  },
);

// Let the shell own the dialog and keep the editor responsible for saving or resetting its session.
async function requestClose(ask: () => Promise<"save" | "discard" | "cancel">) {
  if (working.value) {
    return false;
  }

  // An unread or unresolved startup snapshot must survive closing the app.
  if (!dirty.value) {
    return true;
  }

  let authorized = false;
  await session.run(async () => {
    const choice = await ask();
    if (choice === "cancel" || (choice === "save" && !(await session.save()))) {
      return;
    }

    // Drain recovery writes before clearing persisted data on an authorized close.
    await session.settleRecovery();
    await markdownEditor.reset();
    authorized = true;
  });
  return authorized;
}

onScopeDispose(() => window.removeEventListener("keydown", shortcuts));

defineExpose({
  requestClose,
  navigationBlocked: computed(
    () => working.value || replacement.value !== undefined || recovery.value.phase === "Available",
  ),
  closeError: error,
});
</script>

<template>
  <main class="editor-workspace">
    <header class="workspace-toolbar">
      <div class="file-identity">
        <span class="file-name" :title="name">{{ name }}</span>
        <span class="file-status" role="status">
          {{ source?.pastingImage ? "Pasting image…" : busy ? "Working…" : status || (dirty ? "Unsaved changes" : "Markdown") }}
        </span>
      </div>

      <div class="workspace-actions" role="group" aria-label="File actions">
        <button type="button" :disabled="working || !nativeApp" @click="changeDocument('new')">
          New
        </button>
        <button type="button" :disabled="working || !nativeApp" @click="changeDocument('open')">
          Open
        </button>
        <button
          type="button"
          class="primary-button"
          :disabled="working || !nativeApp"
          @click="saveDocument()"
        >
          Save
        </button>
        <button type="button" :disabled="working || !nativeApp" @click="saveDocument(true)">
          Save As
        </button>
      </div>

      <div class="mode-switch" role="group" aria-label="Editing mode">
        <button
          type="button"
          :disabled="working"
          :aria-pressed="mode === 'source'"
          @click="mode !== 'source' && switchMode('source')"
        >
          Source
        </button>
        <button
          type="button"
          :disabled="working"
          :aria-pressed="mode === 'visual'"
          :aria-describedby="visualReason ? 'visual-reason' : undefined"
          @click="switchMode('visual')"
        >
          Visual
        </button>
      </div>
    </header>

    <div v-if="error || recoveryError || visualReason || !nativeApp" class="workspace-notices">
      <p v-if="!nativeApp">Open WITS Tools on your desktop to use local files and recovery.</p>
      <div
        v-if="error"
        class="workspace-error border-s-2"
        :class="externalChange
          ? 'border-s-status-warning-indicator bg-status-warning-surface text-status-warning-text'
          : 'border-s-status-error-indicator bg-status-error-surface text-status-error-text'"
      >
        <span>{{ error }}</span>
        <button v-if="externalChange" type="button" :disabled="working" @click="saveDocument(true)">
          Save As
        </button>
      </div>
      <p v-if="recoveryError" class="text-status-warning-text">{{ recoveryError }}</p>
      <p v-if="visualReason" id="visual-reason" class="text-status-warning-text">{{ visualReason }}</p>
    </div>

    <section class="editor-body" aria-label="Markdown document">
      <!-- Fade in Source after the shared Markdown buffer is ready. -->
      <NavigationTransition kind="panel" direction="left">
        <SourceEditor
          v-if="mode === 'source'"
          :key="revision"
          ref="source"
          :model-value="buffer"
          :locked="busy"
          @update:model-value="editSource"
        />
      </NavigationTransition>
      <!-- Reveal Visual without changing its existing mounted lifecycle. -->
      <NavigationTransition kind="panel" direction="right">
        <VisualEditor
          v-show="mode === 'visual'"
          ref="visual"
          :locked="busy"
          @edited="session.editVisual"
        />
      </NavigationTransition>
    </section>

    <dialog ref="discardDialog" aria-labelledby="discard-title" @close="resolveChanges">
      <form class="dialog-content" method="dialog">
        <h2 id="discard-title">Save your changes?</h2>
        <p>Changes to {{ name }} will be lost if you discard them.</p>
        <div class="dialog-actions">
          <button value="cancel" autofocus>Cancel</button>
          <button value="discard">Discard</button>
          <button value="save" class="primary-button">Save</button>
        </div>
      </form>
    </dialog>

    <dialog ref="restoreDialog" aria-labelledby="recovery-title" @cancel.prevent>
      <div class="dialog-content">
        <h2 id="recovery-title">Restore unsaved draft?</h2>
        <p>WITS Tools recovered unsaved changes to {{ recoveryName }}.</p>
        <p v-if="error" role="alert">{{ error }}</p>
        <div class="dialog-actions">
          <button type="button" :disabled="busy" @click="recoveryAction(false)">Discard</button>
          <button
            type="button"
            autofocus
            :disabled="busy"
            class="primary-button"
            @click="recoveryAction(true)"
          >
            Restore
          </button>
        </div>
      </div>
    </dialog>
  </main>
</template>

<style scoped>
.editor-workspace {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

.workspace-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.4rem 0.75rem;
  padding: 0.55rem 0.75rem;
  background: var(--surface-toolbar);
  border-bottom: 1px solid var(--surface-selected);
}

.file-identity {
  min-width: min(8rem, 100%);
  flex: 1 1 8rem;
  display: grid;
  overflow: hidden;
}

.file-name {
  font-weight: 650;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-status {
  font-size: 0.8rem;
  color: var(--text-muted);
}

.workspace-actions,
.mode-switch {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.workspace-notices {
  display: grid;
  gap: 0.25rem;
  padding: 0.5rem 0.75rem;
  font-size: 0.875rem;
  border-bottom: 1px solid var(--surface-selected);
}

.workspace-error {
  padding: 0.35rem 0.6rem;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.45rem;
}

.editor-body {
  flex: 1;
  min-height: 0;
}
</style>
