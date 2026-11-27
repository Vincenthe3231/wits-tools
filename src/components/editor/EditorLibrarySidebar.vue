<!-- Adds workspace search and compact pinned shortcuts to the Editor navigation. -->
<script setup lang="ts">
import type { EditorShortcut } from "../../composables/editor/use-editor-library";
import { documentIcon, documentTitle } from "../../utils/editor/document-labels";
import WorkspaceIcon from "../ui/WorkspaceIcon.vue";
import EditorDocumentActions from "./EditorDocumentActions.vue";
import EditorLibrarySearch from "./EditorLibrarySearch.vue";

defineProps<{ pinned: EditorShortcut[] }>();
const query = defineModel<string>({ required: true });
const emit = defineEmits<{ open: [path: string]; locate: [path: string]; pin: [path: string]; remove: [path: string] }>();
</script>

<template>
  <!-- Reuse shell navigation styles for the active home location and the return link. -->
  <div class="editor-navigation">
    <EditorLibrarySearch v-model="query" />
    <nav aria-label="Editor">
      <RouterLink class="workspace-link" :to="{ name: 'editor' }"><WorkspaceIcon name="house" />Home</RouterLink>
      <RouterLink class="workspace-link editor-all-tools" :to="{ name: 'dashboard' }"><WorkspaceIcon name="home" />All tools</RouterLink>
    </nav>

    <!-- Keep pinned files beside Recent and share the same file actions. -->
    <section class="editor-pinned" aria-label="Pinned documents">
      <h2>Pinned</h2>
      <ul v-if="pinned.length">
        <li v-for="entry in pinned" :key="entry.path">
          <button
            type="button"
            class="editor-pinned-file"
            :title="entry.folder + entry.name"
            :aria-label="entry.name"
            @click="entry.missing ? emit('locate', entry.path) : emit('open', entry.path)"
          >
            <WorkspaceIcon :name="documentIcon(entry.name)" />
            <span>{{ documentTitle(entry.name) }}</span>
          </button>
          <EditorDocumentActions :entry="entry" @locate="emit('locate', $event)" @pin="emit('pin', $event)" @remove="emit('remove', $event)" />
        </li>
      </ul>
      <p v-else>Pin files from Recent.</p>
    </section>
  </div>
</template>
