/** Creates prototype blocks and transforms documents without changing saved snapshots. */
import type {
  BlockKind,
  PrototypeBlock,
  PrototypeDocument,
  SavedVariant,
  Starter,
  Variant,
} from "../../api/prototype-builder";

// Convert the published starter and legacy saves through the same path.
export function fromStarter(starter: Starter): PrototypeDocument {
  const ids = new Set(starter.cards.map((card) => card.id));
  let headingId = "starter-heading";
  while (ids.has(headingId)) {
    headingId += "-title";
  }
  return {
    version: 2,
    blocks: [
      { id: headingId, kind: "heading", text: starter.title },
      ...starter.cards.map((card): PrototypeBlock => ({ ...card, kind: "card" })),
    ],
  };
}

// Give edits an independent document even when reopening a loaded version 2 variant.
export function openVariant(variant: SavedVariant): Variant {
  const { id, name, sourceUrl } = variant;
  const document = "document" in variant
    ? { ...variant.document, blocks: variant.document.blocks.map((block) => ({ ...block })) }
    : fromStarter(variant.starter);
  return { id, name, sourceUrl, document };
}

// Create a ready-to-edit block at the selected insertion boundary.
export function createBlock(kind: BlockKind, id: string): PrototypeBlock {
  switch (kind) {
    case "heading": return { id, kind, text: "Heading" };
    case "text": return { id, kind, text: "Text" };
    case "card": return { id, kind, title: "Card title", body: "Card content" };
    case "button": return { id, kind, label: "Button" };
    // Give the complete form usable copy as soon as it lands on the canvas.
    case "signIn": return {
      id,
      kind,
      title: "Welcome back",
      emailLabel: "Email address",
      passwordLabel: "Password",
      buttonLabel: "Sign in",
    };
  }
}

// Reorder using the final index so drag and the position menu share one operation.
export function reorder(blocks: PrototypeBlock[], id: string, destination: number) {
  const position = blocks.findIndex((block) => block.id === id);
  const block = blocks[position];
  if (!block || !Number.isInteger(destination)
    || destination < 0 || destination >= blocks.length || position === destination) {
    return blocks;
  }
  const ordered = [...blocks];
  ordered.splice(position, 1);
  ordered.splice(destination, 0, block);
  return ordered;
}

// Insert only into a valid slot and preserve the incoming block's identity.
export function insert(blocks: PrototypeBlock[], block: PrototypeBlock, index: number) {
  if (!Number.isInteger(index) || index < 0 || index > blocks.length
    || blocks.some((entry) => entry.id === block.id)) {
    return blocks;
  }
  return [...blocks.slice(0, index), block, ...blocks.slice(index)];
}

// Update properties only on the matching block type.
export function update(blocks: PrototypeBlock[], block: PrototypeBlock) {
  if (!blocks.some((entry) => entry.id === block.id && entry.kind === block.kind)) {
    return blocks;
  }
  return blocks.map((entry) => entry.id === block.id ? block : entry);
}

// Permit removing the last block to return to a valid blank document.
export function remove(blocks: PrototypeBlock[], id: string) {
  const remaining = blocks.filter((block) => block.id !== id);
  return remaining.length === blocks.length ? blocks : remaining;
}
