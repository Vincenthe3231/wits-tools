<!-- Provides the Editor launcher and locally saved document shortcuts. -->
<script setup lang="ts">
import { computed, onActivated, ref } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { DropdownMenuContent, DropdownMenuItem, DropdownMenuPortal, DropdownMenuRoot, DropdownMenuTrigger } from "reka-ui";

import EditorDocumentList from "../../components/editor/EditorDocumentList.vue";
import EditorLibrarySearch from "../../components/editor/EditorLibrarySearch.vue";
import WorkspaceIcon from "../../components/ui/WorkspaceIcon.vue";
import type { EditorShortcut } from "../../composables/editor/use-editor-library";

const props = defineProps<{
  entries: EditorShortcut[];
  storageError: boolean;
  enterMarkdown: (action: "new" | "open", path?: string, repair?: string) => Promise<void>;
  enterVisual: () => Promise<void>;
  openDocument: (path?: string, repair?: string) => Promise<void>;
  pickingDocument: boolean;
  sidebarOpen: boolean;
  hasSession: boolean;
  hasVisualSession: boolean;
  togglePin: (path: string) => void;
  remove: (path: string) => void;
}>();

// Use the sidebar query even when the fallback field is shown in the page.
const query = defineModel<string>("query", { required: true });
const layout = ref<"list" | "grid">("list");
const native = isTauri();
const page = ref<HTMLElement>();
// Put focus on the page when returning from a document, including a cancelled picker.
onActivated(() => page.value?.focus({ preventScroll: true }));
const filtered = computed(() => {
  const words = query.value.toLowerCase().trim().split(/\s+/);
  return props.entries.filter((entry) => {
    const format = /\.witsdoc$/i.test(entry.name) ? "Visual" : "Markdown";
    const searchable = `${entry.name} ${entry.folder} ${format}`.toLowerCase();
    return words.every((word) => searchable.includes(word));
  }).sort((first, second) => second.opened - first.opened);
});
</script>

<template>
  <main ref="page" class="editor-home" tabindex="-1">
    <!-- Give document creation its own tile and keep file opening a secondary action. -->
    <header class="editor-home-heading"><h1>Home</h1></header>
    <div class="editor-home-actions">
        <DropdownMenuRoot>
          <DropdownMenuTrigger class="editor-create-tile">
            <span class="editor-create-icon"><WorkspaceIcon name="new-document" /></span>
            <span class="editor-create-label"><strong>New</strong><small>Create a new document</small></span>
            <WorkspaceIcon class="editor-create-chevron" name="chevron" />
          </DropdownMenuTrigger>
          <DropdownMenuPortal>
            <DropdownMenuContent class="editor-menu" align="start" :side-offset="5">
              <DropdownMenuItem class="editor-menu-item" @select="enterMarkdown('new')">
                Markdown Editor
              </DropdownMenuItem>
              <DropdownMenuItem class="editor-menu-item" @select="enterVisual()">
                Visual Editor
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenuPortal>
        </DropdownMenuRoot>
        <button class="editor-open-file" type="button" :disabled="!native || pickingDocument" @click="openDocument()">
          <WorkspaceIcon name="folder" />Open file
        </button>
        <RouterLink v-if="hasSession" class="editor-resume" :to="{ name: 'markdown-editor' }">Resume Markdown</RouterLink>
        <RouterLink v-if="hasVisualSession" class="editor-resume" :to="{ name: 'visual-editor' }">Resume Visual</RouterLink>
    </div>

    <!-- List and Grid share one Recent region. Search moves here when the sidebar is collapsed. -->
    <section class="editor-home-group" aria-labelledby="editor-recent-title">
      <div class="editor-recent-toolbar">
        <h2 id="editor-recent-title">Recent</h2>
        <EditorLibrarySearch v-if="!sidebarOpen" v-model="query" />
        <div class="editor-layout-switch" role="group" aria-label="Document layout">
          <button type="button" aria-label="List view" title="List view" :aria-pressed="layout === 'list'" @click="layout = 'list'"><WorkspaceIcon name="list" /></button>
          <button type="button" aria-label="Grid view" title="Grid view" :aria-pressed="layout === 'grid'" @click="layout = 'grid'"><WorkspaceIcon name="grid" /></button>
        </div>
      </div>
      <p v-if="storageError" class="text-status-warning-text" role="status">Document shortcuts could not be saved.</p>
      <EditorDocumentList
        v-if="layout === 'list' || filtered.length"
        :entries="filtered"
        :layout="layout"
        @open="(path) => openDocument(path)"
        @locate="(path) => openDocument(undefined, path)"
        @pin="togglePin"
        @remove="remove"
      />
      <div v-if="!filtered.length" class="editor-home-empty" role="status">
        <WorkspaceIcon :name="query.trim() ? 'search' : 'document'" />
        <strong>{{ query.trim() ? "No matching documents" : "No recent documents" }}</strong>
        <span>{{ query.trim() ? "Try a different name or folder." : "Create a document or open a file to get started." }}</span>
      </div>
    </section>
  </main>
</template>
