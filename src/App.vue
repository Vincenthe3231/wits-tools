<!-- Retains the editor across routes and hosts window-close confirmation. -->
<script setup lang="ts">
import { computed, nextTick, onScopeDispose, ref } from "vue";
import { useRoute, useRouter } from "vue-router";

import AppToaster from "./components/AppToaster.vue";
import { useWindowClose } from "./composables/use-window-close";
import WorkspaceAppearance from "./components/dashboard/WorkspaceAppearance.vue";
import WorkspaceIcon from "./components/dashboard/WorkspaceIcon.vue";
import WorkspaceSidebar from "./components/dashboard/WorkspaceSidebar.vue";
import NavigationTransition from "./components/NavigationTransition.vue";
import MarkdownEditorPage from "./pages/markdown-editor/MarkdownEditorPage.vue";
import "./styles/dashboard/workspace.css";

type EditorLifecycle = "NotOpened" | "Active" | "Suspended";
type CloseChoice = "save" | "discard" | "cancel";

const route = useRoute();
const editor = ref<InstanceType<typeof MarkdownEditorPage>>();
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
const enterEditor = computed(() => route.name === "markdown-editor");
// Once mounted, route changes only hide the editor so its local history survives.
const lifecycle = computed<EditorLifecycle>(() => {
  if (enterEditor.value) {
    return "Active";
  }

  return editor.value ? "Suspended" : "NotOpened";
});

let resolveClose: ((choice: CloseChoice) => void) | undefined;

// Keep native pickers and unresolved editor prompts attached to their initiating page.
const stopGuard = useRouter().beforeEach(() => !editor.value?.navigationBlocked);
onScopeDispose(stopGuard);

// Suspend the native close request until the user chooses how to handle unsaved edits.
function askClose() {
  const dialog = closeDialog.value;
  if (!dialog) {
    return Promise.resolve<CloseChoice>("cancel");
  }

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

const closeError = useWindowClose(async () => editor.value?.requestClose(askClose) ?? true);
</script>

<template>
  <!-- Keep notifications mounted across dashboard and tool routes. -->
  <AppToaster />

  <div class="workspace-shell" :class="{ 'workspace-shell-collapsed': !sidebarOpen }">
    <!-- The same navigation stays available as the user moves between tools. -->
    <NavigationTransition kind="sidebar">
      <WorkspaceSidebar
        v-show="sidebarOpen"
        id="workspace-sidebar"
        :inert="!sidebarOpen"
        @keydown.esc.stop="closeSidebar"
      />
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
        <label v-if="!enterEditor" class="workspace-search workspace-toolbar-search">
          <WorkspaceIcon name="search" />
          <span class="sr-only">Search the tool catalog</span>
          <input v-model="toolQuery" type="search" placeholder="Search tools…" />
        </label>
        <span v-else class="workspace-location">Markdown Editor</span>
        <WorkspaceAppearance />
      </header>

      <!-- Keep close errors visible even when the editor requesting the save is hidden. -->
      <p v-if="closeError" class="px-3" role="alert">{{ closeError }}</p>
      <p v-if="!enterEditor && editor?.closeError" class="px-3 text-status-error-text">
        {{ editor.closeError }}
      </p>

      <!-- Hide the retained editor instead of remounting it when the catalog is visible. -->
      <div class="workspace-content">
        <RouterView v-slot="{ Component }">
          <NavigationTransition kind="page" direction="left">
            <component :is="Component" v-if="!enterEditor" :key="route.path" :query="toolQuery" />
          </NavigationTransition>
        </RouterView>
        <NavigationTransition kind="page" direction="right">
          <MarkdownEditorPage
            v-if="lifecycle !== 'NotOpened'"
            v-show="enterEditor"
            ref="editor"
            :active="enterEditor"
          />
        </NavigationTransition>
      </div>
    </div>

    <dialog ref="closeDialog" aria-labelledby="close-title" @close="finishClosePrompt">
      <form class="dialog-content" method="dialog">
        <h2 id="close-title">Save changes before closing?</h2>
        <div class="dialog-actions">
          <button value="cancel" autofocus>Cancel</button>
          <button value="discard">Discard</button>
          <button class="primary-button" value="save">Save</button>
        </div>
      </form>
    </dialog>
  </div>
</template>
