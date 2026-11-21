<!-- Hosts Tiptap, Visual editing controls, and transient editor interactions. -->
<script setup lang="ts">
import { computed, nextTick, onScopeDispose, ref, shallowRef, watch } from "vue";
import { Editor, EditorContent } from "@tiptap/vue-3";

import {
  checkedVisualContent,
  markdownExtensions,
} from "../../utils/markdown-editor/markdown-format";
import { useAnimatedCaret } from "../../composables/markdown-editor/use-animated-caret";
import { activeBlock, blockActions, moveBlock } from "../../utils/markdown-editor/visual-blocks";
import NavigationTransition from "../NavigationTransition.vue";
import EditorCaret from "./EditorCaret.vue";

const props = defineProps<{ locked: boolean }>();
const emit = defineEmits<{ edited: [] }>();
const surface = ref<HTMLElement>();
const editor = shallowRef<Editor>();
const slash = ref<{ from: number; to: number; query: string }>();
const activeOption = ref(0);
const linkDialog = ref<HTMLDialogElement>();
const linkValue = ref("");
const linkError = ref("");
const linkButton = ref<HTMLButtonElement>();
const blockMenu = ref<HTMLDetailsElement>();
const blockTrigger = ref<HTMLElement>();
const composing = ref(false);
let dismissedSlash: number | undefined;
let linkApplied = false;

const choices = computed(() =>
  blockActions.filter((item) => item.label.toLowerCase().includes(slash.value?.query ?? "")),
);
const blockIndex = computed(() => (editor.value ? activeBlock(editor.value) : 0));
const canMoveDown = computed(() =>
  editor.value ? blockIndex.value + 1 < editor.value.state.doc.childCount : false,
);

const { caret, trail, refresh, refreshMovement, hide, clearTrail } = useAnimatedCaret(
  surface,
  () => {
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
  },
);

function updateSlash(current: Editor) {
  const { $head, empty } = current.state.selection;
  const paragraph = $head.depth === 1 && $head.parent.type.name === "paragraph";
  const prefix =
    paragraph && $head.parentOffset <= 64 ? $head.parent.textBetween(0, $head.parentOffset) : "";
  const match = /^\/([a-z\d -]*)$/i.exec(prefix);
  if (!empty || !match) {
    slash.value = undefined;
    dismissedSlash = undefined;
    return;
  }

  const from = $head.start();
  if (dismissedSlash === from) {
    return;
  }
  const query = match[1].toLowerCase();
  if (slash.value?.query !== query) {
    activeOption.value = 0;
  }
  slash.value = { from, to: $head.pos, query };
}

function runSlash(item: (typeof blockActions)[number]) {
  const current = editor.value;
  const range = slash.value;
  if (!current || !range || props.locked) {
    return;
  }
  slash.value = undefined;
  item.apply(current.chain().focus().deleteRange({ from: range.from, to: range.to })).run();
}

function slashKey(event: KeyboardEvent) {
  if (!slash.value || event.isComposing || composing.value) {
    return false;
  }

  const items = choices.value;
  if (event.key === "Escape") {
    dismissedSlash = slash.value.from;
    slash.value = undefined;
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

function prepare(markdown: string): string | undefined {
  const next = new Editor({
    extensions: markdownExtensions(),
    editable: !props.locked,
    editorProps: {
      attributes: {
        "aria-label": "Visual Markdown editor",
        role: "textbox",
        "aria-multiline": "true",
      },
      handleKeyDown: (_view, event) => slashKey(event),
      handleDOMEvents: {
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
        updateSlash(current as Editor);
        if (transaction.selectionSet || transaction.docChanged) {
          refreshMovement();
        } else {
          refresh();
        }
      }
    },
    onFocus: () => refresh(),
    onBlur: () => {
      slash.value = undefined;
      hide();
    },
  });

  const result = checkedVisualContent(next, markdown);
  if ("reason" in result) {
    next.destroy();
    return result.reason;
  }

  if (result.content.content?.length) {
    next.commands.setContent(result.content, { emitUpdate: false });
  }
  editor.value?.destroy();
  editor.value = next;
  slash.value = undefined;
  dismissedSlash = undefined;
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

function closeBlockMenu(restoreFocus: boolean) {
  if (blockMenu.value?.open) {
    blockMenu.value.open = false;
  }
  if (restoreFocus) {
    blockTrigger.value?.focus();
  }
}

function moveActiveBlock(direction: -1 | 1) {
  const current = editor.value;
  if (!current || props.locked) {
    return;
  }
  closeBlockMenu(false);
  moveBlock(current, direction);
}

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
  slash.value = undefined;
  closeBlockMenu(false);
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
      <details ref="blockMenu" class="block-menu" @keydown.esc.stop.prevent="closeBlockMenu(true)">
        <summary ref="blockTrigger">Block actions</summary>
        <div class="block-menu-items">
          <button type="button" :disabled="locked || blockIndex === 0" @click="moveActiveBlock(-1)">
            Move Up
          </button>
          <button type="button" :disabled="locked || !canMoveDown" @click="moveActiveBlock(1)">
            Move Down
          </button>
        </div>
      </details>
    </div>

    <div ref="surface" class="visual-surface" :class="{ 'has-animated-caret': caret }">
      <div class="visual-scroll">
        <EditorContent :editor="editor" class="visual-content" />
      </div>
      <EditorCaret :caret="caret" :trail="trail" @trail-finished="clearTrail" />

      <!-- Animate the command menu while the editor keeps keyboard focus. -->
      <NavigationTransition kind="menu">

      </NavigationTransition>      <div v-if="slash" class="slash-menu" @mousedown.prevent>
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

.visual-content {
  width: min(100%, 74ch);
  margin-inline: auto;
  padding: clamp(1rem, 4vw, 3rem);
}

.visual-content :deep(.tiptap) {
  min-height: 14rem;
  padding: 0.5rem;
  line-height: 1.7;
  overflow-wrap: anywhere;
  caret-color: var(--accent-strong);
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
.visual-content :deep(li[data-type="taskItem"]) {
  display: flex;
  gap: 0.5rem;
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
  inset: auto 0.75rem 0.75rem auto;
  width: min(15rem, calc(100% - 1.5rem));
}

.slash-title {
  color: var(--text-muted);
  font-size: 0.8rem;
  padding: 0.35rem 0.5rem;
}

.slash-menu [role="listbox"] {
  max-height: min(14rem, 33vh);
  overflow-y: auto;
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
</style>
