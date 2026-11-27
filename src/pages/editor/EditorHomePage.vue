<!-- Provides the Editor launcher and locally saved document shortcuts. -->
<script setup lang="ts">
import { computed, onActivated, ref } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { DropdownMenuContent, DropdownMenuItem, DropdownMenuPortal, DropdownMenuRoot, DropdownMenuTrigger } from "reka-ui";

import EditorDocumentList from "../../components/editor/EditorDocumentList.vue";
import WorkspaceIcon from "../../components/ui/WorkspaceIcon.vue";
import type { EditorShortcut } from "../../composables/editor/use-editor-library";
import "../../styles/editor/editor-home.css";

const props = defineProps<{
  entries: EditorShortcut[];
  storageError: boolean;
  enterMarkdown: (action: "new" | "open", path?: string, repair?: string) => Promise<void>;
  hasSession: boolean;
  togglePin: (path: string) => void;
  remove: (path: string) => void;
}>();

// Apply a single search and layout to both document groups.
const query = ref("");
const layout = ref<"list" | "grid">("list");
const native = isTauri();
const page = ref<HTMLElement>();
// Put focus on the page when returning from a document, including a cancelled picker.
onActivated(() => page.value?.focus({ preventScroll: true }));
const filtered = computed(() => {
  const words = query.value.toLowerCase().trim().split(/\s+/);
  return props.entries.filter((entry) => {
    const searchable = `${entry.name} ${entry.folder} Markdown`.toLowerCase();
    return words.every((word) => searchable.includes(word));
  });
});
const groups = computed(() => [
  { name: "Pinned", entries: filtered.value.filter((entry) => entry.pinned), empty: "Pin a document to keep it within reach." },
  { name: "Recent", entries: filtered.value.filter((entry) => !entry.pinned), empty: "Your opened and saved documents will appear here." },
]);
</script>

<template>
  <main ref="page" class="editor-home" tabindex="-1">
    <!-- Keep creation and file access ahead of document search. -->
    <header class="editor-home-header">
      <WorkspaceIcon name="document" />
      <h1>Editor</h1>
      <div class="editor-home-actions">
        <DropdownMenuRoot>
          <DropdownMenuTrigger class="primary-button">New <span aria-hidden="true">⌄</span></DropdownMenuTrigger>
          <DropdownMenuPortal>
            <DropdownMenuContent class="editor-menu" align="start" :side-offset="5">
              <DropdownMenuItem class="editor-menu-item" @select="enterMarkdown('new')">
                Markdown Editor
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenuPortal>
        </DropdownMenuRoot>
        <button type="button" :disabled="!native" @click="enterMarkdown('open')">Open file</button>
        <RouterLink v-if="hasSession" class="editor-resume" :to="{ name: 'markdown-editor' }">Resume Markdown</RouterLink>
      </div>
    </header>

    <!-- List and Grid share the same filtered collection and shortcut actions. -->
    <div class="editor-library-tools">
      <label class="workspace-search">
        <WorkspaceIcon name="search" />
        <span class="sr-only">Search saved document shortcuts</span>
        <input v-model="query" type="search" placeholder="Search documents…" />
      </label>
      <div class="editor-layout-switch" role="group" aria-label="Document layout">
        <button type="button" :aria-pressed="layout === 'list'" @click="layout = 'list'">List</button>
        <button type="button" :aria-pressed="layout === 'grid'" @click="layout = 'grid'">Grid</button>
      </div>
    </div>
    <p v-if="storageError" class="text-status-warning-text" role="status">Document shortcuts could not be saved.</p>

    <section v-for="group in groups" :key="group.name" class="editor-home-group" :aria-label="group.name">
      <h2>{{ group.name }}</h2>
      <EditorDocumentList
        v-if="group.entries.length"
        :entries="group.entries"
        :layout="layout"
        @open="(path) => enterMarkdown('open', path)"
        @locate="(path) => enterMarkdown('open', undefined, path)"
        @pin="togglePin"
        @remove="remove"
      />
      <p v-else class="editor-home-empty">{{ query.trim() ? "No matching documents." : group.empty }}</p>
    </section>
  </main>
</template>
