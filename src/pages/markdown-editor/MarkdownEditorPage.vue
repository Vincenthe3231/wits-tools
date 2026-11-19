<script setup lang="ts">
import { nextTick, onMounted, onScopeDispose, ref } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";

import { markdownEditor } from "../../api/markdown-editor";
import SourceEditor from "../../components/markdown-editor/SourceEditor.vue";
import VisualEditor from "../../components/markdown-editor/VisualEditor.vue";
import { useDocumentSession } from "../../composables/markdown-editor/use-document-session";

const visual = ref<InstanceType<typeof VisualEditor>>();
const source = ref<InstanceType<typeof SourceEditor>>();
const discardDialog = ref<HTMLDialogElement>();
const restoreDialog = ref<HTMLDialogElement>();
const recoveryName = ref("");
const visualReason = ref("");
const nativeApp = isTauri();
let unlisten: UnlistenFn | undefined;
let disposed = false;
let decision: ((value: string) => void) | undefined;

const session = useDocumentSession(() => visual.value?.serialize());
const { name, mode, buffer, busy, revision, error, recoveryError, externalChange, status, dirty } =
  session;

function switchMode(target: "source" | "visual", focus = true) {
  if (target === "visual") {
    if (mode.value === "visual") {
      return;
    }
    visualReason.value = visual.value?.prepare(buffer.value) ?? "";
    if (!visual.value || visualReason.value) {
      return;
    }
    mode.value = "visual";
  } else {
    session.materialize();
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

function openVisualByDefault() {
  visualReason.value = "";
  switchMode("visual", false);
}

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

function resolveChanges() {
  decision?.(discardDialog.value?.returnValue ?? "cancel");
  decision = undefined;
}

async function mayReplace() {
  if (!dirty.value) {
    return true;
  }
  const result = await confirmChanges();
  if (result === "save") {
    return session.save();
  }
  return result === "discard";
}

function changeDocument(action: "new" | "open") {
  return session.run(async () => {
    if (!(await mayReplace())) {
      return;
    }
    await session.settleRecovery();
    try {
      if (action === "open") {
        const document = await markdownEditor.open();
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

function saveDocument(saveAs = false) {
  return session.run(async () => {
    await session.save(saveAs);
  });
}

function recoveryAction(restore: boolean) {
  return session.run(async () => {
    if (restore) {
      session.adopt(await markdownEditor.restore(), true);
    } else {
      await markdownEditor.reset();
    }
    recoveryName.value = "";
    restoreDialog.value?.close();
    openVisualByDefault();
  });
}

function shortcuts(event: KeyboardEvent) {
  if (
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
  if (busy.value || !nativeApp) {
    return;
  }
  if (key === "s") {
    void saveDocument(event.shiftKey);
  } else {
    void changeDocument(key === "o" ? "open" : "new");
  }
}

onMounted(async () => {
  window.addEventListener("keydown", shortcuts);
  if (!nativeApp) {
    openVisualByDefault();
    return;
  }

  const currentWindow = getCurrentWindow();
  await session.run(async () => {
    unlisten = await currentWindow.onCloseRequested((event) => {
      event.preventDefault();
      void session.run(async () => {
        if (restoreDialog.value?.open) {
          await currentWindow.destroy();
        } else if (await mayReplace()) {
          await session.settleRecovery();
          await markdownEditor.reset();
          await currentWindow.destroy();
        }
      });
    });
    if (disposed) {
      unlisten?.();
      return;
    }

    recoveryName.value = (await markdownEditor.recoveryInfo()) ?? "";
    if (recoveryName.value) {
      restoreDialog.value?.showModal();
    } else {
      openVisualByDefault();
    }
  });
});

onScopeDispose(() => {
  disposed = true;
  unlisten?.();
  window.removeEventListener("keydown", shortcuts);
});
</script>

<template>
  <main class="editor-workspace">
    <header class="workspace-toolbar">
      <div class="file-identity">
        <span class="file-name" :title="name">{{ name }}</span>
        <span class="file-status" role="status">
          {{ busy ? "Working…" : status || (dirty ? "Unsaved changes" : "Markdown") }}
        </span>
      </div>

      <div class="workspace-actions" role="group" aria-label="File actions">
        <button type="button" :disabled="busy || !nativeApp" @click="changeDocument('new')">
          New
        </button>
        <button type="button" :disabled="busy || !nativeApp" @click="changeDocument('open')">
          Open
        </button>
        <button
          type="button"
          class="primary-button"
          :disabled="busy || !nativeApp"
          @click="saveDocument()"
        >
          Save
        </button>
        <button type="button" :disabled="busy || !nativeApp" @click="saveDocument(true)">
          Save As
        </button>
      </div>

      <div class="mode-switch" role="group" aria-label="Editing mode">
        <button
          type="button"
          :disabled="busy"
          :aria-pressed="mode === 'source'"
          @click="mode !== 'source' && switchMode('source')"
        >
          Source
        </button>
        <button
          type="button"
          :disabled="busy"
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
      <div v-if="error" class="workspace-error" role="alert">
        <span>{{ error }}</span>
        <button v-if="externalChange" type="button" :disabled="busy" @click="saveDocument(true)">
          Save As
        </button>
      </div>
      <p v-if="recoveryError" role="alert">{{ recoveryError }}</p>
      <p v-if="visualReason" id="visual-reason" role="status">{{ visualReason }}</p>
    </div>

    <section class="editor-body" aria-label="Markdown document">
      <SourceEditor
        v-if="mode === 'source'"
        :key="revision"
        ref="source"
        :model-value="buffer"
        :locked="busy"
        @update:model-value="session.editSource"
      />
      <VisualEditor
        v-show="mode === 'visual'"
        ref="visual"
        :locked="busy"
        @edited="session.editVisual"
      />
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
  height: 100dvh;
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
