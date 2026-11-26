// Sends typed Vault requests to the local Rust process.
import { invoke } from "@tauri-apps/api/core";

export type VaultAction =
  | { kind: "status" | "locate" | "lock" }
  | { kind: "open" | "create"; password: string }
  | { kind: "import"; folder: boolean; remove: boolean }
  | { kind: "export"; id: string | null }
  | { kind: "delete"; confirmed: string }
  | { kind: "finishUninstall"; proceed: boolean };

export interface VaultReply {
  path: string | null;
  entries: { id: string; name: string; folder: boolean }[] | null;
  output: string | null;
  remaining: string[];
  uninstall: boolean;
}

// The backend owns the keys and can cancel a transfer without waiting for its command to return.
export function vaultAction(action: VaultAction) {
  return invoke<VaultReply>("vault_action", { action });
}

export function cancelVaultTransfer() {
  return invoke<void>("vault_cancel");
}
