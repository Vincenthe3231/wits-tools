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
import EditorCaret from "./EditorCaret.vue";

const props = defineProps<{ modelValue: string; locked: boolean }>();
const emit = defineEmits<{ "update:modelValue": [markdown: string] }>();
const container = ref<HTMLElement>();
const surface = ref<HTMLElement>();
let view: EditorView | undefined;
let composing = false;

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

onScopeDispose(() => view?.destroy());
defineExpose({ focus: () => view?.focus() });
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
