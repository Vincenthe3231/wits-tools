/** Provides pointer and keyboard controls for editing and moving Visual blocks. */
import type { Editor } from "@tiptap/vue-3";
import type { ResolvedPos } from "@tiptap/pm/model";

import {
  activeBlock,
  blockActions,
  commitBlocks,
  makeBlock,
  moveBlock,
  replaceBlocks,
  type Block,
  type BlockAction,
} from "./visual-blocks";

// Resolve all schema-admissible insertion boundaries, including nested containers.
export function blockDestinations(editor: Editor, block: Block) {
  const destinations: { pos: number; label: string }[] = [];
  const end = block.pos + block.node.nodeSize;

  function visit($parent: ResolvedPos) {
    const parent = $parent.parent;
    let pos = $parent.start();
    for (let index = 0; index <= parent.childCount; index += 1) {
      if ((pos < block.pos || pos > end) && parent.canReplaceWith(index, index, block.node.type)) {
        const next = parent.maybeChild(index);
        const container = $parent.depth ? parent.type.name : "document";
        destinations.push({
          pos,
          label: next
            ? `Before ${next.textContent.slice(0, 36) || next.type.name} in ${container}`
            : `End of ${container}`,
        });
      }
      const child = parent.maybeChild(index);
      if (child?.childCount && !child.isTextblock && (pos < block.pos || pos >= end)) {
        visit(editor.state.doc.resolve(pos + 1));
      }
      pos += child?.nodeSize ?? 0;
    }
  }

  visit(editor.state.doc.resolve(0));
  return destinations;
}

// Apply menu edits using the same schema and Markdown guard as dragging.
export function editBlock(
  editor: Editor,
  block: Block,
  action: "above" | "below" | "duplicate" | "delete" | BlockAction,
) {
  const { tr } = editor.state;
  const end = block.pos + block.node.nodeSize;
  let from = block.pos;
  let to = end;
  let node;

  if (action === "delete") {
    // Remove an empty wrapper or leave a writable paragraph in an empty document.
    let $from = tr.doc.resolve(from);
    while (
      $from.depth &&
      $from.parent.childCount === 1 &&
      ["bulletList", "orderedList", "taskList", "blockquote"].includes($from.parent.type.name)
    ) {
      from = $from.before();
      to = $from.after();
      $from = tr.doc.resolve(from);
    }
    if (from === 0 && to === tr.doc.content.size) {
      node = editor.schema.nodes.paragraph.create();
    }
  } else if (typeof action === "string") {
    const type = block.node.type.name;
    node =
      action === "duplicate"
        ? block.node
        : ["listItem", "taskItem"].includes(type)
          ? block.node.type.createAndFill()
          : makeBlock(editor, blockActions[0]);
    from = to = action === "above" ? from : end;
  } else {
    if (!block.node.isTextblock) {
      return "Turn Into is available for text blocks.";
    }
    node = makeBlock(editor, action, block.node.content);
    if (!node) {
      return "That block type cannot preserve the current content.";
    }
  }

  if (!replaceBlocks(tr, from, to, node ?? undefined)) {
    return "That action is unavailable in this container.";
  }
  return commitBlocks(editor, tr, from + Number(Boolean(node)));
}

// Use the same move operation for neighboring blocks and the destination picker.
export function moveAdjacent(editor: Editor, block: Block, direction: -1 | 1) {
  if (direction === -1 && block.index > 0) {
    return moveBlock(editor, block, block.pos - block.parent.child(block.index - 1).nodeSize);
  }
  if (direction === 1 && block.index + 1 < block.parent.childCount) {
    return moveBlock(
      editor,
      block,
      block.pos + block.node.nodeSize + block.parent.child(block.index + 1).nodeSize,
    );
  }
  return "There is no block in that direction.";
}

// Translate a pointer position into the nearest movable block.
export function blockAtPoint(editor: Editor, x: number, y: number) {
  const hit = editor.view.posAtCoords({ left: x, top: y });
  if (!hit) {
    return undefined;
  }
  // Outer list gutters and quote edges provide a target for moving the whole container.
  const $pos = editor.state.doc.resolve(hit.pos);
  for (let depth = 1; depth <= $pos.depth; depth += 1) {
    const node = $pos.node(depth);
    const pos = $pos.before(depth);
    const dom = editor.view.nodeDOM(pos);
    if (
      dom instanceof HTMLElement &&
      ["bulletList", "orderedList", "taskList", "blockquote"].includes(node.type.name) &&
      x < dom.getBoundingClientRect().left + 12
    ) {
      return { pos, node, parent: $pos.node(depth - 1), index: $pos.index(depth - 1) };
    }
  }
  return activeBlock($pos);
}
