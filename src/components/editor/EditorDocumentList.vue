<!-- Presents saved shortcuts as a Recent table or a gallery of document previews. -->
<script setup lang="ts">
import type { EditorShortcut } from "../../composables/editor/use-editor-library";
import { documentIcon, documentTitle } from "../../utils/editor/document-labels";
import WorkspaceIcon from "../ui/WorkspaceIcon.vue";
import EditorDocumentActions from "./EditorDocumentActions.vue";

defineProps<{ entries: EditorShortcut[]; layout: "list" | "grid" }>();
const emit = defineEmits<{ open: [path: string]; locate: [path: string]; pin: [path: string]; remove: [path: string] }>();

// Format actual file and access dates independently, including older saved shortcuts.
const shortDate = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const exactDate = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const date = (value: number) => new Date(value);
const folderLabel = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

function open(entry: EditorShortcut) {
  if (entry.missing) emit("locate", entry.path);
  else emit("open", entry.path);
}
</script>

<template>
  <!-- Keep desktop metadata in real table columns with a compact row at small widths. -->
  <table v-if="layout === 'list'" class="editor-documents" aria-label="Recent documents">
    <thead>
      <tr>
        <th scope="col">Name</th>
        <th scope="col" class="editor-location-column">Location</th>
        <th scope="col" class="editor-created-column">Created</th>
        <th scope="col" class="editor-opened-column" aria-sort="descending">Last opened</th>
        <th scope="col" class="editor-actions-column"><span class="sr-only">Actions</span></th>
      </tr>
    </thead>
    <tbody>
      <tr v-for="entry in entries" :key="entry.path">
        <td>
          <button type="button" class="editor-document-name" :title="entry.folder + entry.name" :aria-label="entry.name" @click="open(entry)">
            <span class="editor-type-icon" :class="{ 'editor-type-visual': documentIcon(entry.name) === 'visual-document' }">
              <WorkspaceIcon :name="documentIcon(entry.name)" />
            </span>
            <span class="editor-name-label">
              <strong>{{ documentTitle(entry.name) }}</strong>
              <small v-if="entry.missing" class="text-status-warning-text">File moved or unavailable</small>
              <small v-else class="editor-compact-location">{{ folderLabel(entry.folder) }}</small>
            </span>
            <WorkspaceIcon v-if="entry.pinned" class="editor-pin-mark" name="pin" />
          </button>
        </td>
        <td class="editor-location-column">
          <span class="editor-folder" :title="entry.folder"><WorkspaceIcon name="folder" /><span>{{ folderLabel(entry.folder) }}</span></span>
        </td>
        <td class="editor-created-column">
          <time v-if="entry.created !== undefined" :datetime="date(entry.created).toISOString()" :title="exactDate.format(entry.created)">{{ shortDate.format(entry.created) }}</time>
          <span v-else aria-label="Creation date unavailable">—</span>
        </td>
        <td class="editor-opened-column">
          <time :datetime="date(entry.opened).toISOString()" :title="exactDate.format(entry.opened)">{{ shortDate.format(entry.opened) }}</time>
        </td>
        <td class="editor-actions-column">
          <EditorDocumentActions :entry="entry" @locate="emit('locate', $event)" @pin="emit('pin', $event)" @remove="emit('remove', $event)" />
        </td>
      </tr>
    </tbody>
  </table>

  <!-- Grid gives saved content a preview while using the same open and shortcut actions. -->
  <ul v-else class="editor-document-grid" aria-label="Recent documents">
    <li v-for="entry in entries" :key="entry.path">
      <button type="button" class="editor-grid-document" :title="entry.folder + entry.name" :aria-label="entry.name" @click="open(entry)">
        <span class="editor-thumbnail" aria-hidden="true">
          <span v-if="entry.excerpt.length && !entry.missing" class="editor-preview-paper">
            <span v-for="(line, index) in entry.excerpt" :key="index">{{ line }}</span>
          </span>
          <WorkspaceIcon v-else :name="documentIcon(entry.name)" />
        </span>
        <span class="editor-grid-caption">
          <strong>{{ documentTitle(entry.name) }}</strong>
          <small>{{ entry.missing ? "File moved or unavailable" : folderLabel(entry.folder) }}</small>
        </span>
      </button>
      <EditorDocumentActions :entry="entry" @locate="emit('locate', $event)" @pin="emit('pin', $event)" @remove="emit('remove', $event)" />
    </li>
  </ul>
</template>
