<!-- Hosts CodeMirror and handles editing and clipboard actions in Markdown source. -->
<script setup lang="ts">
import { onMounted, onScopeDispose, ref, watch } from "vue";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { EditorState, Transaction } from "@codemirror/state";
import { EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";

import { sourceHighlighting } from "../../const/markdown-editor/source-highlighting";
import { useAnimatedCaret } from "../../composables/markdown-editor/use-animated-caret";
import {
  clipboardImages,
  imageMarkdown,
  stageImage,
} from "../../utils/markdown-editor/image-attachments";
import { editorNotice } from "../../const/markdown-editor/errors";
import { notify } from "../../composables/use-notifications";
import EditorCaret from "./EditorCaret.vue";

const props = defineProps<{ modelValue: string; locked: boolean }>();
const emit = defineEmits<{ "update:modelValue": [markdown: string] }>();
const container = ref<HTMLElement>();
const surface = ref<HTMLElement>();
let view: EditorView | undefined;
let composing = false;

// Keep the clipboard operation separate from ordinary CodeMirror text edits.
const pastingImage = ref(false);
let pasteRange: { from: number; to: number } | undefined;

// Claim a bitmap paste before CodeMirror can consume any text carried alongside it.
function pasteImage(event: ClipboardEvent, current: EditorView): boolean {
  const files = clipboardImages(event.clipboardData);
  if (!files.length) return false;

  event.preventDefault();
  if (!props.locked && !pastingImage.value) void insertImages(current, files);
  return true;
}

// Stage every image first so a failed import leaves the original selection and text intact.
async function insertImages(current: EditorView, files: File[]) {
  pasteRange = current.state.selection.main;
  pastingImage.value = true;

  try {
    const references: string[] = [];
    for (const file of files) {
      references.push(imageMarkdown({ attrs: await stageImage(file, "pasteImage") }));
    }

    if (view !== current || !pasteRange || props.locked) return;

    // Separate multiple images with blank lines and leave the caret after the pasted text.
    const { from, to } = pasteRange;
    const insert = references.join("\n\n");
    current.dispatch({
      changes: { from, to, insert },
      selection: { anchor: from + insert.length },
      scrollIntoView: true,
      userEvent: "input.paste",
    });
  } catch (error) {
    notify(editorNotice(error, "pasteImage"));
  } finally {
    pasteRange = undefined;
    pastingImage.value = false;
  }
}

const { caret, trail, refresh, hide } = useAnimatedCaret(
  surface,
  () => {
    if (!view?.hasFocus || composing || view.composing || !view.state.selection.main.empty) {
      return null;
    }

    return view.coordsAtPos(view.state.selection.main.head);
  },
);

onMounted(() => {
  // CodeMirror uses LF internally. Preserve CRLF when editing a Windows file.
  const lineEnding = props.modelValue.includes("\r\n") ? "\r\n" : "\n";
  const state = EditorState.create({
    doc: props.modelValue,
    extensions: [
      lineNumbers(),
      highlightActiveLine(),
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(sourceHighlighting),
      EditorView.lineWrapping,
      EditorView.contentAttributes.of({
        "aria-label": "Markdown source",
        role: "textbox",
        "aria-multiline": "true",
        spellcheck: "false",
      }),
      EditorView.domEventHandlers({
        paste: pasteImage,
        compositionstart() {
          composing = true;
          hide();
        },
        compositionend() {
          composing = false;
          refresh();
        },
      }),
      EditorView.updateListener.of((update) => {
        // Follow the original paste range if the user types while the image is being staged.
        if (pasteRange && update.docChanged) {
          const from = update.changes.mapPos(pasteRange.from, 1);
          const to = update.changes.mapPos(pasteRange.to, -1);
          pasteRange = { from, to: Math.max(from, to) };
        }

        if (update.docChanged) {
          emit("update:modelValue", update.state.doc.toString().replace(/\n/g, lineEnding));
        }
        refresh();
      }),
      EditorState.transactionFilter.of((transaction) => {
        if (
          props.locked &&
          transaction.docChanged &&
          transaction.annotation(Transaction.userEvent)
        ) {
          return [];
        }
        return transaction;
      }),
    ],
  });

  view = new EditorView({ state, parent: container.value });
});

watch(
  () => props.locked,
  (locked) => {
    view?.contentDOM.setAttribute("contenteditable", String(!locked));
  },
);

// Invalidate any outstanding paste before CodeMirror is destroyed.
onScopeDispose(() => {
  view?.destroy();
  view = undefined;
});
defineExpose({ focus: () => view?.focus(), pastingImage });
</script>

<template>
  <div ref="surface" class="source-editor" :class="{ 'has-animated-caret': caret }">
    <div ref="container" class="source-content" />
    <EditorCaret :caret="caret" :trail="trail" />
  </div>
</template>

<style scoped>
.source-editor,
.source-content {
  height: 100%;
  min-height: 0;
}

.source-editor {
  position: relative;
  overflow: hidden;
}

.source-content :deep(.cm-editor) {
  height: 100%;
}

.source-content :deep(.cm-scroller) {
  font: 0.9rem/1.65 var(--font-code);
}

.source-content :deep(.cm-content) {
  padding: 1rem 0;
  caret-color: var(--accent-strong);
}

.source-content :deep(.cm-line) {
  padding-inline: 0.75rem;
}

.source-content :deep(.cm-gutters) {
  background: var(--surface-toolbar);
  color: var(--text-muted);
  border: 0;
  padding-inline: 0.35rem;
}

.source-content :deep(.cm-activeLine),
.source-content :deep(.cm-activeLineGutter) {
  color: var(--text-primary);
  background: var(--surface-selected);
}

.source-content :deep(.cm-focused) {
  outline: 2px solid var(--accent-strong);
  outline-offset: -2px;
}
</style>
