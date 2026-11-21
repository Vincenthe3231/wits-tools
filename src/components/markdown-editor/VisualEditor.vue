<!-- Hosts Tiptap, Visual editing controls, and transient editor interactions. -->
<script setup lang="ts">
import { nextTick, onMounted, onScopeDispose, ref, shallowRef, watch } from "vue";
import { Editor, EditorContent } from "@tiptap/vue-3";

import {
  checkedVisualContent,
  markdownExtensions,
  type VisualIssue,
} from "../../utils/markdown-editor/markdown-format";
import { useAnimatedCaret } from "../../composables/markdown-editor/use-animated-caret";
import { useVisualImages } from "../../composables/markdown-editor/use-visual-images";
import { useSlashMenu } from "../../composables/markdown-editor/use-slash-menu";
import type { BlockAction } from "../../utils/markdown-editor/visual-blocks";
import VisualBlockControls from "./VisualBlockControls.vue";
import NavigationTransition from "../NavigationTransition.vue";
import EditorCaret from "./EditorCaret.vue";

const props = defineProps<{ locked: boolean }>();
const emit = defineEmits<{ edited: [] }>();
const surface = ref<HTMLElement>();
const editor = shallowRef<Editor>();

// Reuse the current Visual selection for the picker and clipboard commands.
const images = useVisualImages(editor, () => props.locked);
const { busy: importingImage } = images;
const imagePicker = ref<HTMLInputElement>();

// Reset the input so selecting the same file twice still triggers insertion.
function pickImages(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  void images.insert(Array.from(input.files ?? []));
  input.value = "";
}
// Keep menu state outside the saved document and reuse the editor's current selection.
const commands = useSlashMenu(editor);
const { slash, activeOption, choices } = commands;
const controls = ref<InstanceType<typeof VisualBlockControls>>();
const popup = ref<HTMLElement>();
const popupPosition = ref({ left: "0px", top: "0px", maxHeight: "0px" });
const blockError = ref("");
const linkDialog = ref<HTMLDialogElement>();
const linkValue = ref("");
const linkError = ref("");
const linkButton = ref<HTMLButtonElement>();
const composing = ref(false);
let linkApplied = false;

const { caret, trail, refresh, hide } = useAnimatedCaret(surface, () => {
  const current = editor.value;
  if (
    !current?.isFocused ||
    composing.value ||
    current.view.composing ||
    !current.state.selection.empty
  ) {
    return null;
  }
  return current.view.coordsAtPos(current.state.selection.head);
});

// Anchor the menu to the insertion point and reserve space for the visible results.
function positionSlash() {
  const current = editor.value;
  const bounds = surface.value?.getBoundingClientRect();
  if (!slash.value || !current || !bounds) {
    return;
  }
  const cursor = current.view.coordsAtPos(current.state.selection.head);
  const availableBelow = bounds.bottom - cursor.bottom - 8;
  const availableAbove = cursor.top - bounds.top - 8;
  const above = availableBelow < 220 && availableAbove > availableBelow;
  const maxHeight = Math.max(0, Math.min(290, above ? availableAbove : availableBelow));
  const height = Math.min(maxHeight, popup.value?.scrollHeight ?? maxHeight);
  const width = popup.value?.offsetWidth ?? 240;
  popupPosition.value = {
    left: Math.max(8, Math.min(cursor.left - bounds.left, bounds.width - width - 8)) + "px",
    top:
      Math.max(0, above ? cursor.top - bounds.top - height - 4 : cursor.bottom - bounds.top + 4) +
      "px",
    maxHeight: maxHeight + "px",
  };
}

// Report guarded command failures without changing the user's typed text.
function runSlash(item: BlockAction) {
  blockError.value = commands.run(item) ?? "";
}

function slashKey(event: KeyboardEvent) {
  if (!slash.value || event.isComposing || composing.value) {
    return false;
  }

  const items = choices.value;
  if (event.key === "Escape") {
    commands.dismiss();
  } else if (items.length && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
    const step = event.key === "ArrowDown" ? 1 : -1;
    activeOption.value = (activeOption.value + step + items.length) % items.length;
    void nextTick(() =>
      document
        .getElementById(`slash-option-${activeOption.value}`)
        ?.scrollIntoView({ block: "nearest" }),
    );
  } else if (items.length && event.key === "Enter") {
    runSlash(items[activeOption.value]);
  } else {
    return false;
  }

  event.preventDefault();
  return true;
}

function prepare(markdown: string): VisualIssue | undefined {
  const next = new Editor({
    extensions: markdownExtensions(),
    editable: !props.locked,
    editorProps: {
      attributes: {
        "aria-label": "Visual Markdown editor",
        role: "textbox",
        "aria-multiline": "true",
      },
      // Dispatch the literal slash before tracking it so existing text never triggers a menu.
      handleTextInput(view, from, to, text) {
        if (text !== "/" || composing.value || view.composing || props.locked) {
          return false;
        }
        view.dispatch(view.state.tr.insertText(text, from, to));
        commands.start(from);
        return true;
      },
      handleKeyDown: (_view, event) => slashKey(event),
      // Intercept bitmap clipboard events before ProseMirror performs its default conversion.
      handlePaste: (_view, event) => images.paste(event),
      handleDOMEvents: {
        copy: (view, event) => view === editor.value?.view && images.copy(event),
        compositionstart: () => {
          composing.value = true;
          hide();
          return false;
        },
        compositionend: () => {
          composing.value = false;
          refresh();
          return false;
        },
      },
    },
    onUpdate: () => emit("edited"),
    onTransaction: ({ editor: current, transaction }) => {
      if (current === editor.value) {
        commands.update(transaction);
        refresh();
      }
    },
    onFocus: () => refresh(),
    onBlur: () => {
      commands.dismiss();
      hide();
    },
  });

  const result = checkedVisualContent(next, markdown);
  if ("reason" in result) {
    next.destroy();
    return result;
  }

  if (result.content.content?.length) {
    next.commands.setContent(result.content, { emitUpdate: false });
  }
  editor.value?.destroy();
  editor.value = next;
  commands.dismiss();
  return undefined;
}

function openLink() {
  linkApplied = false;
  linkValue.value = editor.value?.getAttributes("link").href ?? "";
  linkError.value = "";
  linkDialog.value?.showModal();
}

function applyLink() {
  const url = linkValue.value.trim();
  if (url && !/^https?:\/\/\S+$|^mailto:[^\s]+$/i.test(url)) {
    linkError.value = "Enter an https, http, or mailto link.";
    return;
  }

  linkApplied = true;
  linkDialog.value?.close();
  const chain = editor.value?.chain().focus().extendMarkRange("link");
  if (url) {
    chain?.setLink({ href: url }).run();
  } else {
    chain?.unsetLink().run();
  }
}

function restoreLinkFocus() {
  if (!linkApplied) {
    linkButton.value?.focus();
  }
}

// Keep menu geometry synchronized after results change size.
watch(slash, () => void nextTick(positionSlash), { flush: "post" });

// A click outside the popup dismisses it even when the editor keeps focus.
function dismissSlashOutside(event: PointerEvent) {
  if (slash.value && event.target instanceof Node && !popup.value?.contains(event.target)) {
    commands.dismiss();
  }
}

// Update the anchor when the window changes size without requiring a keystroke.
onMounted(() => {
  document.addEventListener("pointerdown", dismissSlashOutside, true);
  window.addEventListener("resize", positionSlash);
});

onScopeDispose(() => {
  document.removeEventListener("pointerdown", dismissSlashOutside, true);
  window.removeEventListener("resize", positionSlash);
});

watch(
  () => props.locked,
  (locked) => editor.value?.setEditable(!locked, false),
);
watch([slash, activeOption], () => {
  const dom = editor.value?.view.dom;
  if (slash.value && choices.value.length) {
    dom?.setAttribute("aria-controls", "slash-commands");
    dom?.setAttribute("aria-activedescendant", `slash-option-${activeOption.value}`);
  } else {
    dom?.removeAttribute("aria-controls");
    dom?.removeAttribute("aria-activedescendant");
  }
});

onScopeDispose(() => editor.value?.destroy());
// Clear transient UI on route suspension while preserving the Tiptap document and history.
function dismissInteractions() {
  commands.dismiss();
  controls.value?.dismiss();
  linkDialog.value?.close();
  hide();
}

defineExpose({
  dismissInteractions,
  prepare,
  serialize: () => editor.value?.getMarkdown(),
  focus: () => editor.value?.commands.focus(),
});
</script>

<template>
  <div class="visual-editor">
    <div v-if="editor" class="format-bar" aria-label="Formatting and block actions" role="group">
      <button
        type="button"
        :disabled="locked"
        :aria-pressed="editor.isActive('bold')"
        @click="editor.chain().focus().toggleBold().run()"
      >
        Bold
      </button>
      <button
        type="button"
        :disabled="locked"
        :aria-pressed="editor.isActive('italic')"
        @click="editor.chain().focus().toggleItalic().run()"
      >
        Italic
      </button>
      <button
        type="button"
        :disabled="locked"
        :aria-pressed="editor.isActive('code')"
        @click="editor.chain().focus().toggleCode().run()"
      >
        Code
      </button>
      <button ref="linkButton" type="button" :disabled="locked" @click="openLink">Link</button>
      <!-- Keep the native file picker keyboard-accessible through the toolbar. -->
      <input
        ref="imagePicker"
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        multiple
        hidden
        @change="pickImages"
      />
      <button
        type="button"
        :disabled="locked || importingImage"
        @click="imagePicker?.click()"
      >
        {{ importingImage ? "Inserting…" : "Insert image" }}
      </button>
      <!-- Offer the same block actions from the keyboard-accessible toolbar. -->
      <button
        type="button"
        :disabled="locked"
        @click="controls?.openForSelection($event.currentTarget as HTMLElement)"
      >
        Block actions
      </button>
    </div>

    <div ref="surface" class="visual-surface" :class="{ 'has-animated-caret': caret }">
      <div class="visual-scroll" @scroll="positionSlash">
        <EditorContent :editor="editor" class="visual-content" />
      </div>
      <EditorCaret :caret="caret" :trail="trail" />

      <!-- Keep block controls outside ProseMirror so they never become document content. -->
      <VisualBlockControls
        v-if="editor && surface"
        ref="controls"
        :editor="editor"
        :surface="surface"
        :locked="locked"
        @error="blockError = $event"
      />
      <p v-if="blockError" class="block-error" role="status">{{ blockError }}</p>

      <!-- Animate the command menu while the editor keeps keyboard focus. -->
      <NavigationTransition kind="menu">
        <div v-if="slash" ref="popup" class="slash-menu" :style="popupPosition" @mousedown.prevent>
          <div class="slash-title">Insert block</div>
          <div v-if="!choices.length" role="status">No matching blocks</div>
          <div v-else id="slash-commands" role="listbox" aria-label="Insert block">
            <div
              v-for="(item, index) in choices"
              :id="`slash-option-${index}`"
              :key="item.label"
              role="option"
              :aria-selected="index === activeOption"
              @mousemove="activeOption = index"
              @click="runSlash(item)"
            >
              {{ item.label }}
            </div>
          </div>
        </div>
      </NavigationTransition>
    </div>

    <dialog ref="linkDialog" aria-labelledby="link-title" @close="restoreLinkFocus">
      <form class="dialog-content" @submit.prevent="applyLink">
        <h2 id="link-title">Edit link</h2>
        <label for="link-url">URL</label>
        <input
          id="link-url"
          v-model="linkValue"
          autofocus
          :aria-describedby="linkError ? 'link-error' : undefined"
        />
        <p v-if="linkError" id="link-error" role="alert">{{ linkError }}</p>
        <div class="dialog-actions">
          <button type="button" @click="linkDialog?.close()">Cancel</button>
          <button class="primary-button" type="submit">Apply</button>
        </div>
      </form>
    </dialog>
  </div>
</template>

<style scoped>
.visual-editor {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  height: 100%;
  min-height: 0;
}

.format-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 0.25rem;
  padding: 0.4rem 0.75rem;
  border-bottom: 1px solid var(--surface-selected);
}

.block-menu {
  position: relative;
}

.block-menu summary {
  padding: 0.35rem 0.6rem;
  cursor: pointer;
}

.block-menu-items,
.slash-menu {
  position: absolute;
  z-index: 5;
  padding: 0.45rem;
  border: 1px solid var(--text-muted);
  border-radius: 0.5rem;
  background: var(--surface-toolbar);
  box-shadow: 0 0.5rem 1.5rem #0002;
}

.block-menu-items {
  display: grid;
  min-width: 8rem;
  right: 0;
}

.visual-surface {
  position: relative;
  min-height: 0;
  overflow: hidden;
}

.visual-scroll {
  height: 100%;
  overflow: auto;
}

/* Give the editable root the full pane, including space below a short document. */
.visual-content {
  display: flex;
  min-height: 100%;
  flex-direction: column;
}

.visual-content :deep(.tiptap) {
  flex: 1;
  min-width: 0;
  padding: clamp(1rem, 3vw, 2rem);
  /* Reserve a handle lane beside the text and native list markers. */
  padding-inline-start: clamp(2.75rem, 5vw, 4rem);
  outline: none;
  line-height: 1.7;
  overflow-wrap: anywhere;
  caret-color: var(--accent-strong);
}

/* Mark keyboard focus at the pane edge without framing the document. */
.visual-surface:has(.tiptap:focus-visible)::after {
  position: absolute;
  inset: 0 0 auto;
  height: 2px;
  pointer-events: none;
  background: var(--accent-strong);
  content: "";
}

.visual-content :deep(.tiptap > *) {
  margin-block: 0 0.7em;
}

.visual-content :deep(:is(h1, h2, h3, h4, h5, h6)) {
  font-weight: 650;
  line-height: 1.3;
}

.visual-content :deep(h1) {
  font-size: 2rem;
}
.visual-content :deep(h2) {
  font-size: 1.5rem;
}
.visual-content :deep(h3) {
  font-size: 1.2rem;
}

.visual-content :deep(:is(ul, ol)) {
  padding-inline-start: 1.5rem;
}

.visual-content :deep(ul) {
  list-style: disc;
}
.visual-content :deep(ol) {
  list-style: decimal;
}
.visual-content :deep(ul[data-type="taskList"]) {
  list-style: none;
}
/* Align the checkbox to the first line and keep wrapped or nested text in its own column. */
.visual-content :deep(li[data-type="taskItem"]) {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  align-items: start;
  column-gap: 0.5rem;
  list-style: none;
}
.visual-content :deep(li[data-type="taskItem"] > label) {
  display: grid;
  height: 1lh;
  place-items: center;
}
.visual-content :deep(li[data-type="taskItem"] > div) {
  min-width: 0;
}

.visual-content :deep(blockquote) {
  padding-inline-start: 0.75rem;
  border-inline-start: 3px solid var(--accent-strong);
}

.visual-content :deep(pre) {
  overflow: auto;
  padding: 0.75rem;
  border-radius: 0.35rem;
  background: var(--surface-toolbar);
}

.visual-content :deep(code) {
  font-family: var(--font-code);
}
.visual-content :deep(a) {
  color: var(--accent-strong);
  text-decoration: underline;
}

.slash-menu {
  overflow-y: auto;
  scrollbar-width: thin;
  width: min(15rem, calc(100% - 1.5rem));
}

.slash-title {
  color: var(--text-muted);
  font-size: 0.8rem;
  padding: 0.35rem 0.5rem;
}

.slash-menu [role="listbox"] {
  min-width: 0;
}

/* Use the scrollbar thumb without the platform's arrow buttons. */
.slash-menu::-webkit-scrollbar-button {
  display: none;
}

/* Report an unavailable structural edit without covering the writing area. */
.block-error {
  position: absolute;
  right: 0.75rem;
  bottom: 0.75rem;
  z-index: 2;
  padding: 0.5rem;
  background: var(--surface-toolbar);
  color: var(--text-primary);
}

.slash-menu [role="option"] {
  padding: 0.35rem 0.5rem;
  border-inline-start: 3px solid transparent;
  cursor: pointer;
}

.slash-menu [aria-selected="true"] {
  border-color: var(--accent-strong);
  background: var(--surface-selected);
  color: var(--text-primary);
}
/* Fit image blocks to the writing area and identify selected images without shifting layout. */
.visual-content :deep(img) {
  max-inline-size: 100%;
  block-size: auto;
  vertical-align: middle;
}

.visual-content :deep(img.ProseMirror-selectednode) {
  outline: 2px solid var(--accent-strong);
  outline-offset: 2px;
}

</style>
