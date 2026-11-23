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
  | { id: string; kind: "button"; label: string }
  | SignInBlock;

// Store authored copy only. Preview input values belong to the rendered form.
export type SignInBlock = {
  id: string;
  kind: "signIn";
  title: string;
  emailLabel: string;
  passwordLabel: string;
  buttonLabel: string;
};

export type BlockKind = PrototypeBlock["kind"];
export type LinearDocument = { version: 2; blocks: PrototypeBlock[] };

// Geometry uses document pixels. Node order defines stacking from back to front.
export type ScreenSize = { width: number; height: number };
export type CanvasNode = { block: PrototypeBlock; x: number; y: number; width: number; height?: number };
export type NodeGeometry = Partial<Omit<CanvasNode, "block">>;
export type PrototypeDocument = { version: 3; screen: ScreenSize; nodes: CanvasNode[] };

// A blank project has no upstream source, while older saves still carry a starter.
export type Variant = {
  id: string | null;
  name: string;
  sourceUrl: string | null;
  document: PrototypeDocument;
};

export type SavedVariant =
  | (Omit<Variant, "document"> & { id: string; document: PrototypeDocument | LinearDocument })
  | (ImportedStarter & { id: string; name: string });

// Keep command names and payloads at the IPC boundary.
export const prototypeBuilder = {
  import: (source: string) => invoke<ImportedStarter>("prototype_import", { source }),
  save: (variant: Variant) => invoke<string>("prototype_save", { variant }),
  list: () => invoke<SavedVariant[]>("prototype_list"),
};
