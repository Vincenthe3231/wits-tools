<!-- Renders the same document shortcuts and actions in list and grid layouts. -->
<script setup lang="ts">
import { DropdownMenuContent, DropdownMenuItem, DropdownMenuPortal, DropdownMenuRoot, DropdownMenuTrigger } from "reka-ui";

import type { EditorShortcut } from "../../composables/editor/use-editor-library";
import WorkspaceIcon from "../ui/WorkspaceIcon.vue";

defineProps<{ entries: EditorShortcut[]; layout: "list" | "grid" }>();
const emit = defineEmits<{ open: [path: string]; locate: [path: string]; pin: [path: string]; remove: [path: string] }>();
</script>

<template>
  <!-- Keep the document link and its actions separate for pointer and keyboard use. -->
  <ul class="editor-shortcuts" :class="`editor-shortcuts-${layout}`">
    <li v-for="entry in entries" :key="entry.path">
      <button
        class="editor-shortcut"
        type="button"
        :title="entry.folder + entry.name"
        @click="entry.missing ? emit('locate', entry.path) : emit('open', entry.path)"
      >
        <div v-if="layout === 'grid'" class="editor-thumbnail" aria-hidden="true">
          <div v-if="entry.excerpt.length && !entry.missing">
            <p v-for="(line, index) in entry.excerpt" :key="index">{{ line }}</p>
          </div>
          <WorkspaceIcon v-else name="document" />
        </div>
        <WorkspaceIcon v-else name="document" />
        <span class="editor-shortcut-label">
          <strong>{{ entry.name }}</strong>
          <small>{{ entry.missing ? "File moved or unavailable" : entry.folder }}</small>
        </span>
      </button>
      <DropdownMenuRoot>
        <DropdownMenuTrigger class="editor-shortcut-menu" :aria-label="`Actions for ${entry.name}`">
          <span aria-hidden="true">⋯</span>
        </DropdownMenuTrigger>
        <DropdownMenuPortal>
          <DropdownMenuContent class="editor-menu" align="end" :side-offset="5">
            <DropdownMenuItem class="editor-menu-item" @select="emit('pin', entry.path)">
              {{ entry.pinned ? "Unpin" : "Pin" }}
            </DropdownMenuItem>
            <DropdownMenuItem v-if="entry.missing" class="editor-menu-item" @select="emit('locate', entry.path)">
              Locate file
            </DropdownMenuItem>
            <DropdownMenuItem class="editor-menu-item" @select="emit('remove', entry.path)">
              Remove shortcut
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenuPortal>
      </DropdownMenuRoot>
    </li>
  </ul>
</template>
