/** Defines the commands and state shared by the workspace and browser toolbar. */
import { invoke, isTauri } from "@tauri-apps/api/core";

export interface BrowserTab {
  id: number;
  address: string;
  title: string;
  canBack: boolean;
  canForward: boolean;
}

export interface BrowserState {
  tabs: BrowserTab[];
  active: number | null;
  pending: boolean;
  downloads: number;
  notice: string | null;
}

export type BrowserAction =
  | { kind: "read" | "openTab" | "back" | "forward" | "reload" }
  | { kind: "select" | "closeTab"; id: number }
  | { kind: "bounds"; top: number }
  | { kind: "navigate"; address: string }
  | { kind: "focusPage"; backwards: boolean };

// Keep workspace actions separate from the surface exposed to the trusted toolbar.
export const browser = {
  open: () => invoke<void>("browser_open"),
  prepareClose: () => isTauri() ? invoke<boolean>("browser_prepare_close") : Promise.resolve(true),
  cancelClose: () => isTauri() ? invoke<void>("browser_cancel_close") : Promise.resolve(),
  finishClose: () => isTauri() ? invoke<void>("browser_finish_close") : Promise.resolve(),
  toolbar: (action: BrowserAction) => invoke<BrowserState>("browser_toolbar", { action }),
};
