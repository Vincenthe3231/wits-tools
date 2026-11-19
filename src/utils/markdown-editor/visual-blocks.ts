import type { ChainedCommands, Editor } from "@tiptap/core";
import { closeHistory } from "@tiptap/pm/history";
import { Fragment } from "@tiptap/pm/model";
import { TextSelection } from "@tiptap/pm/state";

export const blockActions = [
  { label: "Paragraph", apply: (chain: ChainedCommands) => chain.setParagraph() },
  { label: "Heading 1", apply: (chain: ChainedCommands) => chain.setHeading({ level: 1 }) },
  { label: "Heading 2", apply: (chain: ChainedCommands) => chain.setHeading({ level: 2 }) },
  { label: "Heading 3", apply: (chain: ChainedCommands) => chain.setHeading({ level: 3 }) },
  { label: "Bullet list", apply: (chain: ChainedCommands) => chain.toggleBulletList() },
  { label: "Numbered list", apply: (chain: ChainedCommands) => chain.toggleOrderedList() },
  { label: "Task list", apply: (chain: ChainedCommands) => chain.toggleTaskList() },
  { label: "Quote", apply: (chain: ChainedCommands) => chain.toggleBlockquote() },
  { label: "Code block", apply: (chain: ChainedCommands) => chain.setCodeBlock() },
];

export function activeBlock(editor: Editor) {
  return editor.state.selection.$head.index(0);
}

export function moveBlock(editor: Editor, direction: -1 | 1) {
  const { doc, selection, tr } = editor.state;
  const index = activeBlock(editor);
  const target = index + direction;
  if (target < 0 || target >= doc.childCount) {
    return;
  }

  const startIndex = Math.min(index, target);
  let start = 0;
  for (let i = 0; i < startIndex; i += 1) {
    start += doc.child(i).nodeSize;
  }

  const block = doc.child(index);
  const neighbor = doc.child(target);
  const oldStart = direction === -1 ? start + neighbor.nodeSize : start;
  const newStart = direction === -1 ? start : start + neighbor.nodeSize;
  const reordered = direction === -1 ? [block, neighbor] : [neighbor, block];

  tr.replaceWith(start, start + block.nodeSize + neighbor.nodeSize, Fragment.fromArray(reordered));
  const head = newStart + Math.max(0, Math.min(block.nodeSize, selection.head - oldStart));
  tr.setSelection(TextSelection.near(tr.doc.resolve(head))).scrollIntoView();
  editor.view.dispatch(closeHistory(tr));
  editor.view.focus();
}
