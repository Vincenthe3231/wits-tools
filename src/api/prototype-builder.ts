/** Defines the prototype data passed between Vue and the desktop commands. */
import { invoke } from "@tauri-apps/api/core";

// Each snapshot owns its dashboard and optionally carries an existing save ID.
export type Starter = {
  version: 1;
  title: string;
  cards: { id: string; title: string; body: string }[];
};

export type ImportedStarter = { sourceUrl: string; starter: Starter };
export type Variant = ImportedStarter & { id: string | null; name: string };

// Keep command names and payloads at the IPC boundary.
export const prototypeBuilder = {
  import: (source: string) => invoke<ImportedStarter>("prototype_import", { source }),
  save: (variant: Variant) => invoke<string>("prototype_save", { variant }),
  list: () => invoke<Variant[]>("prototype_list"),
};
