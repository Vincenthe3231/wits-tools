/** Defines the prototype data passed between Vue and the desktop commands. */
import { invoke } from "@tauri-apps/api/core";

// GitHub still serves version 1 starters independently of local projects.
export type Starter = {
  version: 1;
  title: string;
  cards: { id: string; title: string; body: string }[];
};

export type ImportedStarter = { sourceUrl: string; starter: Starter };

// Blocks carry stable IDs and only the properties used by their renderer.
export type PrototypeBlock =
  | { id: string; kind: "heading"; text: string }
  | { id: string; kind: "text"; text: string }
  | { id: string; kind: "card"; title: string; body: string }
  | { id: string; kind: "button"; label: string };

export type BlockKind = PrototypeBlock["kind"];
export type PrototypeDocument = { version: 2; blocks: PrototypeBlock[] };

// A blank project has no upstream source, while older saves still carry a starter.
export type Variant = {
  id: string | null;
  name: string;
  sourceUrl: string | null;
  document: PrototypeDocument;
};

export type SavedVariant =
  | (Variant & { id: string })
  | (ImportedStarter & { id: string; name: string });

// Keep command names and payloads at the IPC boundary.
export const prototypeBuilder = {
  import: (source: string) => invoke<ImportedStarter>("prototype_import", { source }),
  save: (variant: Variant) => invoke<string>("prototype_save", { variant }),
  list: () => invoke<SavedVariant[]>("prototype_list"),
};
