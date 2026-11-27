<!-- Provides the retained rich-text Visual document and its native file actions. -->
<script setup lang="ts">
import { computed, nextTick, onMounted, onScopeDispose, ref, shallowRef, watch } from "vue";
import { Editor, EditorContent } from "@tiptap/vue-3";
import type { JSONContent } from "@tiptap/core";
import { addColumnAfter, addRowAfter, deleteColumn, deleteRow, deleteTable } from "@tiptap/pm/tables";
import { DropdownMenuContent, DropdownMenuItem, DropdownMenuPortal, DropdownMenuRoot, DropdownMenuTrigger } from "reka-ui";

import { visualEditor } from "../../api/visual-editor";
import { clipboardImages } from "../../utils/markdown-editor/image-attachments";
import { embeddedImage, emptyVisualContent, starterTable, visualExtensions } from "../../utils/visual-editor/visual-document";
import { useVisualSession, type VisualCloseChoice } from "../../composables/visual-editor/use-visual-session";
import { visualNotice } from "../../const/visual-editor/errors";
import { notify } from "../../composables/use-notifications";

const props = defineProps<{ active: boolean; decide: (name: string) => Promise<VisualCloseChoice> }>();
const emit = defineEmits<{ file: [path: string, excerpt: string]; missing: [path: string] }>();
const editor = shallowRef<Editor>();
const imagePicker = ref<HTMLInputElement>();
const imageBusy = ref(false);
const picking = ref(false);
const linkDialog = ref<HTMLDialogElement>();
const linkTarget = ref("");
const linkError = ref("");
const transaction = ref(0);
const session = useVisualSession(
  () => editor.value, replaceContent, props.decide,
  (path, excerpt) => emit("file", path, excerpt),
  (path) => emit("missing", path),
);
const { pendingDraft, dirty, busy, name, externalChange, recoveryError, native } = session;
const working = computed(() => busy.value || imageBusy.value || picking.value);
const locked = computed(() => working.value || Boolean(pendingDraft.value));

// Replace the instance on open so a document can never undo into a different file.
function replaceContent(content: JSONContent) {
  const next = new Editor({
    content,
    extensions: visualExtensions(),
    editable: !locked.value,
    enableContentCheck: true,
    editorProps: {
      attributes: { "aria-label": "Visual document content", role: "textbox", "aria-multiline": "true" },
      handlePaste: (_view, event) => {
        const images = clipboardImages(event.clipboardData);
        if (!images.length || locked.value) return false;
        void insertImages(images, "pasteImage");
        return true;
      },
    },
    onUpdate: session.edited,
    onTransaction: () => { transaction.value += 1; },
  });
  editor.value?.destroy();
  editor.value = next;
}

// Import the whole selection before editing so a rejected image never leaves a partial paste.
async function insertImages(files: File[], operation: "pasteImage" | "insertImage") {
  if (!files.length || locked.value || !editor.value) return;
  const target = editor.value;
  imageBusy.value = true;
  try {
    const nodes: JSONContent[] = [];
    for (const file of files) nodes.push(await embeddedImage(file, operation));
    target.chain().focus().insertContent(nodes).run();
  } catch (cause) {
    notify(visualNotice(cause, operation));
  } finally {
    imageBusy.value = false;
  }
}

function pickImages(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  void insertImages(Array.from(input.files ?? []), "insertImage");
  input.value = "";
}

// Use text labels and native selections so available formatting is discoverable from the keyboard.
function openLink() {
  linkTarget.value = editor.value?.getAttributes("link").href ?? "";
  linkError.value = "";
  linkDialog.value?.showModal();
}

function applyLink() {
  const href = linkTarget.value.trim();
  if (href && !/^(https?:\/\/|mailto:)/i.test(href)) {
    linkError.value = "Enter an HTTPS, HTTP, or mailto link.";
    return;
  }
  const chain = editor.value?.chain().focus().extendMarkRange("link");
  if (href) chain?.setLink({ href }).run();
  else chain?.unsetLink().run();
  linkDialog.value?.close();
}

const tableCommands = [
  { label: "Add row", run: addRowAfter },
  { label: "Add column", run: addColumnAfter },
  { label: "Delete row", run: deleteRow },
  { label: "Delete column", run: deleteColumn },
  { label: "Delete table", run: deleteTable },
];
const selectionInTable = computed(() => {
  void transaction.value;
  return editor.value?.isActive("table") ?? false;
});

function editTable(command: typeof addRowAfter) {
  const current = editor.value;
  if (current) command(current.state, current.view.dispatch);
  current?.commands.focus();
}

// Keep picker scope Visual here. Editor home can dispatch either local file format.
async function openFile() {
  if (locked.value || !native) return;
  picking.value = true;
  try {
    const path = await visualEditor.pick(true);
    if (path) await session.open(path);
  } catch (cause) {
    notify(visualNotice(cause, "open"));
  } finally {
    picking.value = false;
  }
}

function shortcut(event: KeyboardEvent) {
  if (!props.active || !(event.ctrlKey || event.metaKey) || event.altKey || event.isComposing) return;
  const key = event.key.toLowerCase();
  if (!["s", "o", "n"].includes(key)) return;
  event.preventDefault();
  if (locked.value || linkDialog.value?.open) return;
  if (key === "s" && native) void session.save(event.shiftKey);
  else if (key === "o") void openFile();
  else if (key === "n") void session.open();
}

// Keep startup recovery ahead of the first open request and retain history across tool navigation.
let startup = Promise.resolve<unknown>(undefined);
onMounted(() => {
  replaceContent(emptyVisualContent());
  startup = Promise.resolve(session.initialize());
  window.addEventListener("keydown", shortcut);
});
watch(locked, (value) => editor.value?.setEditable(!value, false));
watch(() => props.active, (active) => {
  if (!active) session.flush();
});
onScopeDispose(() => {
  window.removeEventListener("keydown", shortcut);
  editor.value?.destroy();
});

async function enter(path?: string, onOpened?: (path: string) => void) {
  await startup;
  const opened = await session.open(path, onOpened);
  if (opened && !pendingDraft.value) await nextTick(() => editor.value?.commands.focus());
  return opened;
}

defineExpose({
  enter,
  navigationBlocked: working,
  requestClose: () => imageBusy.value || picking.value ? Promise.resolve(false) : session.requestClose(),
  finishClose: session.finishClose,
});
</script>

<template>
  <main class="visual-document">
    <!-- Keep file commands separated from formatting so they remain visible at narrow widths. -->
    <header class="visual-file-toolbar">
      <div class="visual-file-name">
        <strong :title="name">{{ name }}</strong>
        <small role="status">{{ working ? "Working…" : dirty ? "Unsaved changes" : "Visual document" }}</small>
      </div>
      <div class="visual-actions" role="group" aria-label="File actions">
        <button type="button" :disabled="locked" @click="session.open()">New</button>
        <button type="button" :disabled="locked || !native" @click="openFile">Open</button>
        <button type="button" class="primary-button" :disabled="locked || !native" @click="session.save()">Save</button>
        <button type="button" :disabled="locked || !native" @click="session.save(true)">Save As</button>
      </div>
    </header>
    <div v-if="pendingDraft || externalChange || recoveryError" class="visual-notices">
      <div v-if="pendingDraft" class="visual-actions">
        <p>Restore unsaved Visual document?</p>
        <button type="button" :disabled="working" @click="session.resolveDraft(false)">Discard draft</button>
        <button type="button" class="primary-button" :disabled="working" @click="session.resolveDraft(true)">Restore</button>
      </div>
      <div v-if="externalChange" class="visual-actions">
        <p>The original file changed. Save a copy to keep your edits.</p>
        <button type="button" :disabled="locked" @click="session.save(true)">Save As</button>
      </div>
      <p v-if="recoveryError" class="text-status-warning-text" role="status">{{ recoveryError }}</p>
    </div>

    <!-- Use the same Tiptap editing commands in the toolbar and document keyboard shortcuts. -->
    <div v-if="editor" class="visual-format-toolbar" role="group" aria-label="Formatting">
      <DropdownMenuRoot>
        <DropdownMenuTrigger :disabled="locked">Style</DropdownMenuTrigger>
        <DropdownMenuPortal>
          <DropdownMenuContent class="editor-menu" align="start">
            <DropdownMenuItem class="editor-menu-item" @select="editor.chain().focus().setParagraph().run()">Paragraph</DropdownMenuItem>
            <DropdownMenuItem v-for="level in ([1, 2, 3] as const)" :key="level" class="editor-menu-item" @select="editor.chain().focus().toggleHeading({ level }).run()">Heading {{ level }}</DropdownMenuItem>
            <DropdownMenuItem class="editor-menu-item" @select="editor.chain().focus().toggleBlockquote().run()">Quote</DropdownMenuItem>
            <DropdownMenuItem class="editor-menu-item" @select="editor.chain().focus().toggleCodeBlock().run()">Code block</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenuPortal>
      </DropdownMenuRoot>
      <button type="button" :disabled="locked" :aria-pressed="(transaction, editor.isActive('bold'))" @click="editor.chain().focus().toggleBold().run()">Bold</button>
      <button type="button" :disabled="locked" :aria-pressed="(transaction, editor.isActive('italic'))" @click="editor.chain().focus().toggleItalic().run()">Italic</button>
      <button type="button" :disabled="locked" :aria-pressed="(transaction, editor.isActive('code'))" @click="editor.chain().focus().toggleCode().run()">Code</button>
      <button type="button" :disabled="locked" @click="openLink">Link</button>
      <DropdownMenuRoot>
        <DropdownMenuTrigger :disabled="locked">List</DropdownMenuTrigger>
        <DropdownMenuPortal>
          <DropdownMenuContent class="editor-menu" align="start">
            <DropdownMenuItem class="editor-menu-item" @select="editor.chain().focus().toggleBulletList().run()">Bullet list</DropdownMenuItem>
            <DropdownMenuItem class="editor-menu-item" @select="editor.chain().focus().toggleOrderedList().run()">Numbered list</DropdownMenuItem>
            <DropdownMenuItem class="editor-menu-item" @select="editor.chain().focus().toggleTaskList().run()">Checklist</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenuPortal>
      </DropdownMenuRoot>
      <button type="button" :disabled="locked" @click="imagePicker?.click()">Image</button>
      <button type="button" :disabled="locked || selectionInTable" @click="editor.chain().focus().insertContent(starterTable()).run()">Insert table</button>
      <DropdownMenuRoot v-if="selectionInTable">
        <DropdownMenuTrigger :disabled="locked">Table actions</DropdownMenuTrigger>
        <DropdownMenuPortal>
          <DropdownMenuContent class="editor-menu" align="start">
            <DropdownMenuItem v-for="command in tableCommands" :key="command.label" class="editor-menu-item" @select="editTable(command.run)">
              {{ command.label }}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenuPortal>
      </DropdownMenuRoot>
      <input ref="imagePicker" type="file" hidden multiple accept="image/png,image/jpeg,image/gif,image/webp" @change="pickImages" />
    </div>
    <div class="visual-document-scroll">
      <EditorContent v-if="editor" :editor="editor" class="visual-document-body" />
    </div>

    <dialog ref="linkDialog" aria-labelledby="visual-link-title">
      <form class="dialog-content" @submit.prevent="applyLink">
        <h2 id="visual-link-title">Edit link</h2>
        <label for="visual-link-target">URL</label>
        <input id="visual-link-target" v-model="linkTarget" :aria-invalid="Boolean(linkError)" aria-describedby="visual-link-error" />
        <p v-if="linkError" id="visual-link-error" role="alert">{{ linkError }}</p>
        <div class="dialog-actions">
          <button type="button" @click="linkDialog?.close()">Cancel</button>
          <button type="submit" class="primary-button">Apply</button>
        </div>
      </form>
    </dialog>
  </main>
</template>

<style scoped src="../../styles/editor/visual-document.css"></style>
