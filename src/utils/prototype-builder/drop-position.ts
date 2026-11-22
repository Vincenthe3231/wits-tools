/** Finds the insertion slot and indicator position for a vertical canvas. */

// Each block contributes a midpoint for choosing its preceding or following slot.
type BlockBounds = { top: number; bottom: number };

export function findDropPosition(blocks: readonly BlockBounds[], pointerY: number, emptyTop: number) {
  const first = blocks[0];
  if (!first) {
    return { index: 0, top: emptyTop };
  }

  // In the gap between two blocks, show the line between their actual edges.
  const index = blocks.findIndex(({ top, bottom }) => pointerY < (top + bottom) / 2);
  if (index === 0) {
    return { index, top: first.top };
  }
  if (index === -1) {
    return { index: blocks.length, top: blocks[blocks.length - 1].bottom };
  }
  return { index, top: (blocks[index - 1].bottom + blocks[index].top) / 2 };
}
