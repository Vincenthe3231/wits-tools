/** Defines the prototype data passed between Vue and the desktop commands. */
import { invoke } from "@tauri-apps/api/core";
import type { Style, Theme } from "../utils/prototype-builder/appearance";

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
  | SignInBlock
  | ImportedBlock;

// Imported definitions describe plain text and a small set of owned layout primitives.
export type ComponentText = { literal: string } | { field: string };
export type ComponentElement =
  | { kind: "stack" | "row"; children: ComponentElement[]; style?: Style }
  | { kind: "text" | "button"; text: ComponentText; style?: Style };
export type ComponentDefinition = {
  version: 1; id: string; revision: number; name: string; defaultWidth: number;
  fields: Record<string, { label: string; default: string }>;
  root: ComponentElement;
};
export type ImportedBlock = {
  id: string; kind: "imported"; definitionId: string; revision: number; values: Record<string, string>;
};
export type ComponentDiagnostic = { code: string; path: string; message: string; line?: number; column?: number };
export type ComponentReview = { name: string; diagnostics: ComponentDiagnostic[]; definition?: ComponentDefinition };

// Store authored copy only. Preview input values belong to the rendered form.
export type SignInBlock = {
  id: string;
  kind: "signIn";
  title: string;
  emailLabel: string;
  passwordLabel: string;
  buttonLabel: string;
};

export type BlockKind = Exclude<PrototypeBlock["kind"], "imported">;
export type LinearDocument = { version: 2; blocks: Exclude<PrototypeBlock, ImportedBlock>[] };

// Geometry uses document pixels. Node order defines stacking from back to front.
export type ScreenSize = { width: number; height: number };
export type CanvasNode = { block: PrototypeBlock; x: number; y: number; width: number; height?: number; style?: Style };
export type NodeGeometry = Partial<Pick<CanvasNode, "x" | "y" | "width" | "height">>;
type CanvasDocument = { screen: ScreenSize; nodes: CanvasNode[] };
export type PrototypeDocument = CanvasDocument & (
  | { version: 3; theme?: never; definitions?: never }
  | { version: 4; theme: Theme; definitions: ComponentDefinition[] }
);

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
  chooseComponent: () => invoke<ComponentReview | null>("prototype_component_choose"),
  installComponent: (definition: ComponentDefinition) =>
    invoke<ComponentReview>("prototype_component_install", { definition }),
  components: () => invoke<ComponentDefinition[]>("prototype_component_list"),
  removeComponent: (id: string, revision: number) => invoke<void>("prototype_component_remove", { id, revision }),
};
