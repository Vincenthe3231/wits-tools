/** Builds and validates structural edits before applying them to the Visual editor. */
import type { Editor } from "@tiptap/core";
import { closeHistory } from "@tiptap/pm/history";
import { Fragment, Slice, type Node, type ResolvedPos } from "@tiptap/pm/model";
import { Selection, type Transaction } from "@tiptap/pm/state";
import { ReplaceStep } from "@tiptap/pm/transform";

// Use a common catalog so slash insertion and block conversion offer the same types.
export const blockActions = [
  { label: "Paragraph", type: "paragraph" },
  { label: "Heading 1", type: "heading", level: 1 },
  { label: "Heading 2", type: "heading", level: 2 },
  { label: "Heading 3", type: "heading", level: 3 },
  { label: "Bullet list", type: "bulletList" },
  { label: "Numbered list", type: "orderedList" },
  { label: "Task list", type: "taskList" },
  { label: "Quote", type: "blockquote" },
  { label: "Code block", type: "codeBlock" },
] as const;

export type BlockAction = (typeof blockActions)[number];
export type Block = { pos: number; node: Node; parent: Node; index: number };

const UNSAFE_EDIT = "This change would lose structure when saved as Markdown.";

// Find the nearest meaningful block. A list item's first text block represents the item.
export function activeBlock($pos: ResolvedPos): Block | undefined {
  let depth = $pos.depth;
  if (
    depth > 1 &&
    $pos.index(depth - 1) === 0 &&
    ["listItem", "taskItem"].includes($pos.node(depth - 1).type.name)
  ) {
    depth -= 1;
  }
  if (depth === 0) {
    const node = $pos.nodeAfter ?? $pos.nodeBefore;
    return node?.isBlock
      ? {
          pos: $pos.nodeAfter ? $pos.pos : $pos.pos - node.nodeSize,
          node,
          parent: $pos.parent,
          index: $pos.index() - ($pos.nodeAfter ? 0 : 1),
        }
      : undefined;
  }

  return {
    pos: $pos.before(depth),
    node: $pos.node(depth),
    parent: $pos.node(depth - 1),
    index: $pos.index(depth - 1),
  };
}

// Build a schema-valid node and keep inline marks when the destination accepts them.
export function makeBlock(
  editor: Editor,
  action: BlockAction,
  content = Fragment.empty,
): Node | null {
  const { nodes } = editor.schema;
  const paragraph = nodes.paragraph.create(null, content);
  if (action.type === "blockquote") {
    return nodes.blockquote.create(null, paragraph);
  }
  if (action.type === "bulletList" || action.type === "orderedList" || action.type === "taskList") {
    const item = action.type === "taskList" ? nodes.taskItem : nodes.listItem;
    return nodes[action.type].create(null, item.create(null, paragraph));
  }
  return nodes[action.type].createAndFill(
    "level" in action ? { level: action.level } : null,
    content,
  );
}

// Use an exact replacement so the fitter cannot silently reshape the requested edit.
export function replaceBlocks(tr: Transaction, from: number, to: number, node?: Node) {
  return !tr.maybeStep(
    new ReplaceStep(from, to, node ? new Slice(Fragment.from(node), 0, 0) : Slice.empty),
  ).failed;
}

// Check the saved representation before adding anything to the undo history.
export function commitBlocks(editor: Editor, tr: Transaction, focus: number): string | undefined {
  try {
    tr.doc.check();
    const markdown = editor.markdown;
    if (!markdown) {
      return UNSAFE_EDIT;
    }
    const exported = markdown.serialize(tr.doc.toJSON());
    const parsed = editor.schema.nodeFromJSON(markdown.parse(exported));
    const reopened = parsed.childCount ? parsed : editor.schema.topNodeType.createAndFill();
    if (!reopened?.eq(tr.doc)) {
      return UNSAFE_EDIT;
    }
    tr.setSelection(Selection.near(tr.doc.resolve(Math.min(focus, tr.doc.content.size))));
    editor.view.dispatch(closeHistory(tr).scrollIntoView());
    editor.view.focus();
  } catch {
    return UNSAFE_EDIT;
  }
}

// Move a subtree as a unit. Remove empty list or quote wrappers when moving their only child.
export function moveBlock(editor: Editor, source: Block, destination: number): string | undefined {
  const { doc, tr } = editor.state;
  const end = source.pos + source.node.nodeSize;
  if (destination >= source.pos && destination <= end) {
    return "Choose a different destination.";
  }

  const $destination = doc.resolve(destination);
  if (
    !$destination.parent.canReplaceWith(
      $destination.index(),
      $destination.index(),
      source.node.type,
    )
  ) {
    return "This container cannot hold that block.";
  }

  // Insert first so the source container can retain required children during a sibling move.
  if (!replaceBlocks(tr, destination, destination, source.node)) {
    return "This container cannot hold that block.";
  }
  let from = tr.mapping.map(source.pos, 1);
  let to = tr.mapping.map(end, -1);
  let $from = tr.doc.resolve(from);
  while (
    $from.depth > 0 &&
    $from.parent.childCount === 1 &&
    ["bulletList", "orderedList", "taskList", "blockquote"].includes($from.parent.type.name)
  ) {
    from = $from.before();
    to = $from.after();
    $from = tr.doc.resolve(from);
  }
  const inserted = tr.mapping.map(destination, -1);
  if (!replaceBlocks(tr, from, to)) {
    return "Moving this block would leave its original container invalid.";
  }
  return commitBlocks(editor, tr, tr.mapping.slice(1).map(inserted, -1) + 1);
}
