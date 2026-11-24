/** Creates prototype blocks and transforms documents without changing saved snapshots. */
import type {
  BlockKind,
  PrototypeBlock,
  LinearDocument,
  SavedVariant,
  Starter,
  Variant,
} from "../../api/prototype-builder";

// Convert the published starter and legacy saves through the same path.
export function fromStarter(starter: Starter): LinearDocument {
  const ids = new Set(starter.cards.map((card) => card.id));
  let headingId = "starter-heading";
  while (ids.has(headingId)) {
    headingId += "-title";
  }
  return {
    version: 2,
    blocks: [
      { id: headingId, kind: "heading", text: starter.title },
      ...starter.cards.map((card): LinearDocument["blocks"][number] => ({ ...card, kind: "card" })),
    ],
  };
}

// Give edits an independent document even when reopening a loaded version 2 variant.
export function openVariant(variant: SavedVariant): Omit<Variant, "document"> & { document: Variant["document"] | LinearDocument } {
  const { id, name, sourceUrl } = variant;
  const document = "document" in variant
    ? variant.document.version !== 2
      ? { ...variant.document, screen: { ...variant.document.screen },
        nodes: variant.document.nodes.map((node) => ({ ...node, block: { ...node.block } })) }
      : { ...variant.document, blocks: variant.document.blocks.map((block) => ({ ...block })) }
    : fromStarter(variant.starter);
  return { id, name, sourceUrl, document };
}

// A palette entry and a saved instance resolve by their exact definition revision.
export function definitionKey(definition: { id: string; revision: number }) {
  return definition.id + "@" + definition.revision;
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
