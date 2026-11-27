<!-- Hosts workspace navigation, retained tool sessions, and window-close confirmation. -->
<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onMounted, onScopeDispose, ref } from "vue";
import { useRoute, useRouter } from "vue-router";

import { browser } from "./api/browser";
import { crawler } from "./api/crawler";
import { downloader } from "./api/downloader";
import { cancelVaultTransfer, vaultAction } from "./api/vault";
import { visualEditor } from "./api/visual-editor";
import { editorNotice } from "./const/markdown-editor/errors";
import { notify } from "./composables/use-notifications";
import AppToaster from "./components/AppToaster.vue";
import { useWindowClose } from "./composables/use-window-close";
import { useEditorLibrary } from "./composables/editor/use-editor-library";
import WorkspaceAppearance from "./components/dashboard/WorkspaceAppearance.vue";
import WorkspaceIcon from "./components/ui/WorkspaceIcon.vue";
import WorkspaceSidebar from "./components/dashboard/WorkspaceSidebar.vue";
import EditorLibrarySidebar from "./components/editor/EditorLibrarySidebar.vue";
import NavigationTransition from "./components/NavigationTransition.vue";
import { tools } from "./const/dashboard/tools";
import MarkdownEditorPage from "./pages/markdown-editor/MarkdownEditorPage.vue";
import VisualEditorPage from "./pages/visual-editor/VisualEditorPage.vue";
import "./styles/dashboard/workspace.css";
import "./styles/editor/editor-home.css";

type EditorLifecycle = "NotOpened" | "Active" | "Suspended";
type CloseChoice = "save" | "discard" | "cancel";

const route = useRoute();
const router = useRouter();

// Installer launches use a dedicated window view without normal workspace navigation.
const VaultPage = defineAsyncComponent(() => import("./pages/vault/VaultPage.vue"));
const installerWizard = ref(false);
onMounted(async () => {
  installerWizard.value = (await vaultAction({ kind: "status" }).catch(() => null))?.uninstall ?? false;
});
const editor = ref<InstanceType<typeof MarkdownEditorPage>>();
const visual = ref<InstanceType<typeof VisualEditorPage>>();
const pickingDocument = ref(false);
const library = useEditorLibrary();
const { entries, storageError } = library;

// Keep search on the dashboard and derive tool names from the shared catalog.
const isDashboard = computed(() => route.name === "dashboard");
const isEditorHome = computed(() => route.name === "editor");
const editorQuery = ref("");
const pinnedDocuments = computed(() => entries.value.filter((entry) => entry.pinned));
const toolName = computed(() => {
  if (route.name === "markdown-editor") return "Markdown Editor";
  if (route.name === "visual-editor") return "Visual Editor";
  return tools.find((tool) => tool.routeName === route.name)?.name;
});

// Preserve the catalog filter while a tool is open.
const toolQuery = ref("");
// Keep navigation visible by default and give its toggle a stable focus target.
const sidebarOpen = ref(true);
const sidebarToggle = ref<HTMLButtonElement>();

// Move focus out of the sidebar when Escape dismisses it.
async function closeSidebar() {
  sidebarOpen.value = false;
  await nextTick();
  sidebarToggle.value?.focus();
}

const closeDialog = ref<HTMLDialogElement>();
const closeDocumentName = ref("");
const enterEditor = computed(() => route.name === "markdown-editor");
const enterVisualEditor = computed(() => route.name === "visual-editor");
// Once mounted, route changes only hide the editor so its local history survives.
const lifecycle = computed<EditorLifecycle>(() => {
  if (enterEditor.value) {
    return "Active";
  }

  return editor.value ? "Suspended" : "NotOpened";
});

// Route home actions through the retained Markdown session and its dirty-document guard.
async function enterMarkdown(action: "new" | "open", path?: string, repair?: string) {
  const failure = await router.push({ name: "markdown-editor" });
  if (failure) return;
  await nextTick();
  const opened = await editor.value?.enter(action, path, (newPath) => {
    if (repair) library.relocate(repair, newPath);
  });
  if (!opened) await router.push({ name: "editor" });
}

// File choices route by format and keep each editor's live session and history retained.
async function enterVisual(path?: string, repair?: string) {
  const failure = await router.push({ name: "visual-editor" });
  if (failure) return;
  await nextTick();
  const opened = await visual.value?.enter(path, (newPath) => {
    if (repair) library.relocate(repair, newPath);
  });
  if (!opened) await router.push({ name: "editor" });
}

async function openDocument(path?: string, repair?: string) {
  const visualFile = (value: string) => /\.witsdoc$/i.test(value);
  if (!path && repair && !visualFile(repair)) {
    return enterMarkdown("open", undefined, repair);
  }
  if (!path) {
    if (pickingDocument.value) return;
    pickingDocument.value = true;
    try {
      path = await visualEditor.pick(Boolean(repair)) ?? undefined;
    } catch (cause) {
      notify(editorNotice(cause, "open"));
    } finally {
      pickingDocument.value = false;
    }
  }
  if (!path) return;
  return visualFile(path) ? enterVisual(path, repair) : enterMarkdown("open", path, repair);
}

let resolveClose: ((choice: CloseChoice) => void) | undefined;

// Keep native pickers and unresolved editor prompts attached to their initiating page.
const stopGuard = router.beforeEach(() =>
  !pickingDocument.value && !editor.value?.navigationBlocked && !visual.value?.navigationBlocked,
);
onScopeDispose(stopGuard);

// Suspend the native close request until the user chooses how to handle unsaved edits.
function askClose(name: string) {
  const dialog = closeDialog.value;
  if (!dialog) {
    return Promise.resolve<CloseChoice>("cancel");
  }

  closeDocumentName.value = name;
  dialog.returnValue = "cancel";
  dialog.showModal();
  return new Promise<CloseChoice>((resolve) => {
    resolveClose = resolve;
  });
}

// Treat Escape and any unrecognized dialog result as cancellation.
function finishClosePrompt() {
  const value = closeDialog.value?.returnValue;
  resolveClose?.(value === "save" || value === "discard" ? value : "cancel");
  resolveClose = undefined;
}

// Resolve both window decisions before discarding drafts or stopping active tools.
const closeError = useWindowClose(async () => {
  if (pickingDocument.value) return false;
  if (installerWizard.value) {
    await cancelVaultTransfer().catch(() => undefined);
    await vaultAction({ kind: "finishUninstall", proceed: false });
    return false;
  }
  if (!(await browser.prepareClose())) return false;
  try {
    if (visual.value && !(await visual.value.requestClose())) return false;
    if (editor.value && !(await editor.value.requestClose(() => askClose("Markdown document")))) return false;
    await visual.value?.finishClose();
    await browser.finishClose();
  } finally {
    await browser.cancelClose().catch(() => undefined);
  }
  await cancelVaultTransfer().catch(() => undefined);
  await Promise.allSettled([crawler.shutdown(), downloader.shutdown(), vaultAction({ kind: "lock" })]);
  return true;
});
</script>

<template>
  <!-- Keep notifications mounted across dashboard and tool routes. -->
  <AppToaster />

  <VaultPage v-if="installerWizard" installer />
  <div v-else class="workspace-shell" :class="{ 'workspace-shell-collapsed': !sidebarOpen, 'editor-home-shell': isEditorHome }">
    <!-- The same navigation stays available as the user moves between tools. -->
    <NavigationTransition kind="sidebar">
      <WorkspaceSidebar
        v-show="sidebarOpen"
        id="workspace-sidebar"
        :inert="!sidebarOpen"
        :editor="isEditorHome"
        @keydown.esc.stop="closeSidebar"
      >
        <EditorLibrarySidebar
          v-if="isEditorHome"
          v-model="editorQuery"
          :pinned="pinnedDocuments"
          @open="(path) => openDocument(path)"
          @locate="(path) => openDocument(undefined, path)"
          @pin="library.togglePin"
          @remove="library.remove"
        />
      </WorkspaceSidebar>
    </NavigationTransition>

    <div class="workspace-stage">
      <!-- The toolbar identifies the current view and keeps appearance controls within reach. -->
      <header class="workspace-header">
        <!-- The toolbar toggle stays reachable when the navigation is hidden. -->
        <button
          ref="sidebarToggle"
          class="workspace-sidebar-toggle"
          type="button"
          aria-controls="workspace-sidebar"
          :aria-expanded="sidebarOpen"
          :aria-label="sidebarOpen ? 'Hide sidebar' : 'Show sidebar'"
          @click="sidebarOpen = !sidebarOpen"
        >
          <WorkspaceIcon name="sidebar" />
        </button>
        <label v-if="isDashboard" class="workspace-search workspace-toolbar-search">
          <WorkspaceIcon name="search" />
          <span class="sr-only">Search the tool catalog</span>
          <input v-model="toolQuery" type="search" placeholder="Search tools…" />
        </label>
        <span v-else-if="!isEditorHome" class="workspace-location">{{ toolName }}</span>
        <WorkspaceAppearance />
      </header>

      <!-- Keep close errors visible even when the editor requesting the save is hidden. -->
      <p v-if="closeError" class="px-3" role="alert">{{ closeError }}</p>
      <p v-if="!enterEditor && editor?.closeError" class="px-3 text-status-error-text">
        {{ editor.closeError }}
      </p>

      <!-- Retain tool drafts and selections while navigating through the workspace. -->
      <div class="workspace-content">
        <RouterView v-slot="{ Component }">
          <NavigationTransition kind="page" direction="left">
            <KeepAlive include="ApiTesterPage,CrawlerPage,DownloaderPage,EditorHomePage">
              <component
                :is="Component"
                v-if="!enterEditor && !enterVisualEditor"
                :key="route.path"
                v-bind="isDashboard ? { query: toolQuery } : route.name === 'editor' ? {
                  entries, storageError, enterMarkdown, enterVisual, openDocument,
                  hasSession: Boolean(editor), hasVisualSession: Boolean(visual),
                  pickingDocument, sidebarOpen, query: editorQuery,
                  'onUpdate:query': (value: string) => editorQuery = value,
                  togglePin: library.togglePin, remove: library.remove,
                } : {}"
              />
            </KeepAlive>
          </NavigationTransition>
        </RouterView>
        <NavigationTransition kind="page" direction="right">
          <MarkdownEditorPage
            v-if="lifecycle !== 'NotOpened'"
            v-show="enterEditor"
            ref="editor"
            :active="enterEditor"
            @file="library.record"
            @missing="library.markMissing"
          />
        </NavigationTransition>
        <NavigationTransition kind="page" direction="right">
          <VisualEditorPage
            v-if="enterVisualEditor || visual"
            v-show="enterVisualEditor"
            ref="visual"
            :active="enterVisualEditor"
            :decide="askClose"
            @file="library.record"
            @missing="library.markMissing"
          />
        </NavigationTransition>
      </div>
    </div>

    <dialog ref="closeDialog" aria-labelledby="close-title" @close="finishClosePrompt">
      <form class="dialog-content" method="dialog">
        <h2 id="close-title">Save unsaved changes?</h2>
        <p>{{ closeDocumentName }}</p>
        <div class="dialog-actions">
          <button value="cancel" autofocus>Cancel</button>
          <button value="discard">Discard</button>
          <button class="primary-button" value="save">Save</button>
        </div>
      </form>
    </dialog>
  </div>
</template>
