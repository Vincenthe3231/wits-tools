<!-- Owns the interactive Vault and the installer’s Vault choices. -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { onBeforeRouteLeave } from "vue-router";

import { cancelVaultTransfer, vaultAction, type VaultAction, type VaultReply } from "../../api/vault";
import { notify } from "../../composables/use-notifications";

const props = defineProps<{ installer?: boolean }>();

// Only an unlocked native session supplies the file list.
const data = ref<VaultReply>();
const pending = ref<VaultAction["kind"]>();
const error = ref("");
const message = ref("");
const remaining = ref<string[]>([]);
const password = ref("");
const confirmation = ref("");
const creating = ref(false);
const wizard = ref(false);
const deleting = ref(false);
const remove = ref(false);
const confirmMove = ref<HTMLDialogElement>();
const moveFolder = ref(false);
const restored = ref("");
const locked = computed(() => data.value?.entries == null);
const showWizard = computed(() => props.installer || wizard.value);
let timer: ReturnType<typeof setInterval> | undefined;

// Clearing form fields before awaiting IPC limits how long passwords live in Vue state.
async function run(action: VaultAction, success?: string) {
  if (pending.value) return;
  pending.value = action.kind;
  error.value = "";
  if (action.kind !== "status") {
    message.value = "";
    remaining.value = [];
  }
  try {
    const response = await vaultAction(action);
    if (data.value?.path !== response.path) {
      restored.value = "";
      deleting.value = false;
    }
    data.value = response;
    if (response.remaining.length) {
      remaining.value = response.remaining;
      message.value = "The encrypted copy was saved. Some originals remain.";
    } else if (response.output) {
      message.value = `Saved to ${response.output}`;
      if (action.kind === "export" && action.id === null) restored.value = response.output;
    } else if (success && (
      action.kind === "lock" || action.kind === "delete" || action.kind === "locate" || action.kind === "open" || action.kind === "create"
    )) {
      message.value = success;
    }
    return response;
  } catch (cause) {
    error.value = typeof cause === "string" ? cause : "Vault could not complete the action.";
    if (action.kind !== "status") notify({ severity: "error", title: "Vault action failed", description: error.value });
  } finally {
    pending.value = undefined;
  }
}

// Publish all restored files before asking whether to delete the encrypted Vault.
async function credentials() {
  if (creating.value && password.value !== confirmation.value) {
    error.value = "The passwords do not match.";
    return;
  }
  const wasCreating = creating.value;
  const action = { kind: wasCreating ? "create" : "open", password: password.value } as const;
  password.value = "";
  confirmation.value = "";
  const response = await run(action);
  if (response?.entries) creating.value = false;
}

function startImport(folder: boolean) {
  if (!remove.value) {
    void run({ kind: "import", folder, remove: false });
    return;
  }
  moveFolder.value = folder;
  confirmMove.value?.showModal();
}

function approveMove() {
  confirmMove.value?.close();
  void run({ kind: "import", folder: moveFolder.value, remove: true });
}

async function deleteVault() {
  if (!data.value?.path) return;
  const response = await run({ kind: "delete", confirmed: data.value.path }, "Vault data was removed.");
  if (response) {
    deleting.value = false;
    restored.value = "";
  }
}

// A departure may not strand an active transfer or unlocked native session.
onBeforeRouteLeave(async () => {
  if (pending.value) return false;
  return (await run({ kind: "lock" })) !== undefined;
});
onMounted(() => {
  void run({ kind: "status" });
  timer = setInterval(() => {
    if (data.value?.entries) void run({ kind: "status" });
  }, 5000);
});
onUnmounted(() => clearInterval(timer));
</script>

<template>
  <section class="vault-page">
    <!-- Keep location and recovery instructions visible even before unlock. -->
    <header>
      <h1>{{ showWizard ? "Prepare to uninstall" : "Vault" }}</h1>
      <p v-if="data?.path" class="vault-path">{{ data.path }}</p>
      <p v-else-if="showWizard">Locate your Vault to decrypt or delete it. You can also continue and keep any encrypted Vault folders.</p>
      <p v-else>Choose where to store your encrypted Vault, or locate an existing one.</p>
    </header>

    <p v-if="error" class="text-status-error-text" role="alert">{{ error }}</p>
    <p v-if="message" role="status">{{ message }}</p>
    <ul v-if="remaining.length">
      <li v-for="path in remaining" :key="path" class="vault-path">{{ path }}</li>
    </ul>
    <div v-if="pending && pending !== 'status'" class="vault-actions" role="status">
      <span>{{ pending === "import" || pending === "export" ? "Transferring files…" : "Working…" }}</span>
      <button v-if="pending === 'import' || pending === 'export'" type="button" @click="cancelVaultTransfer">
        Cancel transfer
      </button>
    </div>

    <!-- Prompt for credentials only for create, unlock, and full decryption. -->
    <fieldset :disabled="Boolean(pending && pending !== 'status')">
      <div v-if="showWizard" class="vault-card">
        <p>Keep the encrypted folder to reopen it in the latest WITS Tools with your password.</p>
        <div class="vault-actions">
          <button v-if="installer" type="button" class="primary" @click="run({ kind: 'finishUninstall', proceed: true })">
            {{ data?.path ? "Keep Vault encrypted and uninstall" : "Continue uninstall" }}
          </button>
          <button type="button" @click="run({ kind: 'locate' })">Locate Vault</button>
          <button v-if="installer" type="button" @click="run({ kind: 'finishUninstall', proceed: false })">Cancel uninstall</button>
          <button v-else type="button" @click="wizard = false">Back to Vault</button>
        </div>
        <template v-if="data?.path">
          <p>To get ordinary files back, unlock Vault and choose a destination for all files.</p>
          <button v-if="!locked && !restored" type="button" @click="run({ kind: 'export', id: null })">Decrypt all files</button>
          <p v-if="restored" class="vault-path">Restored and verified at {{ restored }}. You can now remove the encrypted Vault.</p>
          <button type="button" @click="deleting = true">Permanently delete Vault…</button>
        </template>
      </div>

      <div v-else class="vault-actions">
        <button v-if="!showWizard" type="button" @click="run({ kind: 'locate' })">Open existing Vault</button>
        <button v-if="locked" type="button" @click="creating = !creating">{{ creating ? "Cancel setup" : "Create Vault" }}</button>
        <button v-else type="button" @click="run({ kind: 'lock' })">Lock</button>
        <button v-if="data?.path" type="button" @click="wizard = true">Prepare to uninstall</button>
      </div>

      <form v-if="locked && (creating || data?.path)" class="vault-card" @submit.prevent="credentials">
        <p v-if="creating">Your password and the complete Vault folder are needed to recover your files. Keep a backup of the locked folder.</p>
        <label>Password
          <input v-model="password" type="password" :autocomplete="creating ? 'new-password' : 'current-password'" required />
        </label>
        <label v-if="creating">Confirm password
          <input v-model="confirmation" type="password" autocomplete="new-password" required minlength="12" />
        </label>
        <button class="primary" type="submit">{{ creating ? "Choose storage folder" : "Unlock Vault" }}</button>
      </form>

      <!-- Copy is the default. A move needs deliberate selection and confirmation. -->
      <div v-if="!locked && !showWizard" class="vault-card">
        <label class="vault-check"><input v-model="remove" type="checkbox" /> Move into Vault by removing originals after saving</label>
        <div class="vault-actions">
          <button class="primary" type="button" @click="startImport(false)">{{ remove ? "Move file…" : "Add file copy…" }}</button>
          <button type="button" @click="startImport(true)">{{ remove ? "Move folder…" : "Add folder copy…" }}</button>
        </div>
        <p v-if="!data?.entries?.length">Vault is empty.</p>
        <ul v-else class="vault-list">
          <li v-for="entry in data.entries" :key="entry.id">
            <span class="vault-path">{{ entry.name }}{{ entry.folder ? "/" : "" }}</span>
            <button type="button" :aria-label="`Export copy of ${entry.name}`" @click="run({ kind: 'export', id: entry.id })">
              Export copy
            </button>
          </li>
        </ul>
      </div>

      <!-- Place the destructive confirmation beside the selected location. -->
      <div v-if="deleting && data?.path" class="vault-card" role="alert">
        <strong>Permanently delete this Vault?</strong>
        <p class="vault-path">{{ data.path }}</p>
        <p>Deleting this folder removes the encrypted files. Retained backups are needed to recover them.</p>
        <div class="vault-actions">
          <button class="danger" type="button" @click="deleteVault">Confirm deletion</button>
          <button type="button" @click="deleting = false">Cancel</button>
        </div>
      </div>
    </fieldset>

    <dialog ref="confirmMove" class="vault-card">
      <form method="dialog">
        <p>The originals will be removed after the encrypted copy is saved. This uses ordinary deletion.</p>
        <div class="vault-actions">
          <button type="submit" autofocus>Cancel</button>
          <button class="primary" type="button" @click="approveMove">Choose item to move</button>
        </div>
      </form>
    </dialog>
  </section>
</template>

<style scoped>
.vault-page {
  display: grid;
  align-content: start;
  gap: 1rem;
  padding: clamp(1rem, 4vw, 2rem);
  height: 100%;
  overflow: auto;
}

h1 {
  font-size: 1.5rem;
  font-weight: 650;
}

.vault-card {
  display: grid;
  gap: 0.75rem;
  background: var(--surface-toolbar);
  color: var(--text-primary);
  border-radius: 0.75rem;
  padding: 1rem;
}

fieldset {
  display: grid;
  gap: 1rem;
  min-inline-size: 0;
}

dialog:not([open]) {
  display: none;
}

dialog::backdrop {
  background: rgb(0 0 0 / 45%);
}

.vault-actions, .vault-check, .vault-list li {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.vault-list li {
  justify-content: space-between;
  padding-block: 0.35rem;
}

.vault-path {
  overflow-wrap: anywhere;
}

label:not(.vault-check) {
  display: grid;
  gap: 0.25rem;
}

button, input[type="password"] {
  min-height: 44px;
  padding-inline: 1rem;
  border: 1px solid var(--text-muted);
  border-radius: 1.5rem;
}

button {
  justify-self: start;
}

button:focus-visible, input:focus-visible {
  outline: 2px solid var(--accent-strong);
  outline-offset: 3px;
}

button.primary {
  background: var(--accent-strong);
  color: var(--text-on-accent);
}

button.danger {
  color: var(--status-error-text);
  border-color: var(--status-error-indicator);
}
</style>
