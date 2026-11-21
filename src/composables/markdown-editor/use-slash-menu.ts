/** Tracks slash commands from direct input and preserves surrounding Markdown content. */
import { computed, ref, type ShallowRef } from "vue";
import type { Editor } from "@tiptap/vue-3";
import type { Transaction } from "@tiptap/pm/state";

import {
  blockActions,
  commitBlocks,
  makeBlock,
  replaceBlocks,
  type BlockAction,
} from "../../utils/markdown-editor/visual-blocks";

export function useSlashMenu(editor: ShallowRef<Editor | undefined>) {
  const slash = ref<{ from: number; to: number; query: string }>();
  const activeOption = ref(0);
  const choices = computed(() =>
    blockActions.filter((item) =>
      item.label.toLowerCase().includes(slash.value?.query.toLowerCase() ?? ""),
    ),
  );

  // Start only from a direct text input event so paste, load, and undo cannot open the menu.
  function start(from: number) {
    slash.value = { from, to: from + 1, query: "" };
    activeOption.value = 0;
  }

  // Follow the live trigger through edits and dismiss it when Space is inserted or the caret leaves.
  function update(transaction: Transaction) {
    const range = slash.value;
    if (!range) {
      return;
    }
    const mapped = transaction.mapping.mapResult(range.from, 1);
    const { $head, empty } = transaction.selection;
    const text = transaction.doc.textBetween(
      mapped.pos,
      Math.max(mapped.pos, $head.pos),
      "\n",
      "\n",
    );
    if (
      mapped.deleted ||
      !empty ||
      $head.pos < mapped.pos + 1 ||
      mapped.pos < $head.start() ||
      !text.startsWith("/") ||
      /[ \n]/.test(text)
    ) {
      dismiss();
      return;
    }
    const query = text.slice(1);
    if (query !== range.query) {
      activeOption.value = 0;
    }
    slash.value = { from: mapped.pos, to: $head.pos, query };
  }

  // Replace an empty text block in place or insert at the nearest legal ancestor boundary.
  function run(item: BlockAction): string | undefined {
    const current = editor.value;
    const range = slash.value;
    if (!current?.isEditable || !range) {
      return;
    }
    const { tr } = current.state;
    tr.delete(range.from, range.to);
    const $head = tr.doc.resolve(range.from);
    const block = makeBlock(current, item);
    if (!block) {
      return "This block is unavailable here.";
    }
    for (let depth = $head.depth; depth > 0; depth -= 1) {
      const to = $head.after(depth);
      const from =
        depth === $head.depth &&
        !$head.parent.content.size &&
        $head.parent.type.name !== "codeBlock"
          ? $head.before(depth)
          : to;
      if (replaceBlocks(tr, from, to, block) || (from !== to && replaceBlocks(tr, to, to, block))) {
        dismiss();
        return commitBlocks(current, tr, (tr.doc.nodeAt(from) === block ? from : to) + 1);
      }
    }
    return "This block cannot be inserted here.";
  }

  // Keep the literal input when the user cancels or leaves the editor.
  function dismiss() {
    slash.value = undefined;
  }

  return { slash, activeOption, choices, start, update, run, dismiss };
}
