<!-- Shares the shortcut menu across Recent, Grid, and the pinned sidebar. -->
<script setup lang="ts">
import { DropdownMenuContent, DropdownMenuItem, DropdownMenuPortal, DropdownMenuRoot, DropdownMenuTrigger } from "reka-ui";

import type { EditorShortcut } from "../../composables/editor/use-editor-library";
import WorkspaceIcon from "../ui/WorkspaceIcon.vue";

defineProps<{ entry: EditorShortcut }>();
const emit = defineEmits<{ locate: [path: string]; pin: [path: string]; remove: [path: string] }>();
</script>

<template>
  <DropdownMenuRoot>
    <DropdownMenuTrigger class="editor-shortcut-menu" :aria-label="`Actions for ${entry.name}`">
      <WorkspaceIcon name="more" />
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
</template>
