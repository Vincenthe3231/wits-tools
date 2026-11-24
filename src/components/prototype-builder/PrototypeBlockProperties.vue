<!-- Edits the supported properties of the selected prototype block. -->
<script setup lang="ts">
import type { PrototypeBlock } from "../../api/prototype-builder";

// Emit a new block so updates flow through the draft's normal mutation path.
const props = defineProps<{ block: PrototypeBlock; disabled: boolean }>();
const emit = defineEmits<{
  update: [block: PrototypeBlock];
  remove: [id: string];
}>();

// Restrict each input to the properties its block actually owns.
function changeText(event: Event) {
  const block = props.block;
  if (block.kind === "heading" || block.kind === "text") {
    emit("update", { ...block, text: (event.target as HTMLInputElement).value });
  }
}

function changeCard(field: "title" | "body", event: Event) {
  const block = props.block;
  if (block.kind === "card") {
    emit("update", { ...block, [field]: (event.target as HTMLInputElement).value });
  }
}

function changeLabel(event: Event) {
  const block = props.block;
  if (block.kind === "button") {
    emit("update", { ...block, label: (event.target as HTMLInputElement).value });
  }
}

// Each field updates the existing block while preserving its ID and other copy.
const signInFields = [
  { field: "title", label: "Card title" },
  { field: "emailLabel", label: "Email field label" },
  { field: "passwordLabel", label: "Password field label" },
  { field: "buttonLabel", label: "Submit button label" },
] as const;

function changeSignIn(field: typeof signInFields[number]["field"], event: Event) {
  const block = props.block;
  if (block.kind === "signIn") {
    emit("update", { ...block, [field]: (event.target as HTMLInputElement).value });
  }
}
</script>

<template>
  <!-- Keep each control visibly labeled and leave block removal in the inspector. -->
  <fieldset class="properties-fields" :disabled="disabled">
    <div v-if="block.kind === 'heading'">
      <label for="prototype-heading-text">Heading text</label>
      <input id="prototype-heading-text" :value="block.text" @input="changeText" />
    </div>
    <div v-else-if="block.kind === 'text'">
      <label for="prototype-body-text">Text content</label>
      <textarea id="prototype-body-text" :value="block.text" rows="4" @input="changeText" />
    </div>
    <template v-else-if="block.kind === 'card'">
      <div>
        <label for="prototype-card-title">Card title</label>
        <input id="prototype-card-title" :value="block.title" @input="changeCard('title', $event)" />
      </div>
      <div>
        <label for="prototype-card-body">Card body</label>
        <textarea id="prototype-card-body" :value="block.body" rows="4" @input="changeCard('body', $event)" />
      </div>
    </template>
    <!-- Keep authored labels in Properties and sample input in Preview. -->
    <template v-else-if="block.kind === 'signIn'">
      <div v-for="{ field, label } in signInFields" :key="field">
        <label :for="'prototype-sign-in-' + field">{{ label }}</label>
        <input :id="'prototype-sign-in-' + field" :value="block[field]" @input="changeSignIn(field, $event)" />
      </div>
    </template>
    <div v-else-if="block.kind === 'button'">
      <label for="prototype-button-label">Button label</label>
      <input id="prototype-button-label" :value="block.label" @input="changeLabel" />
      <p class="properties-note">Visual placeholder. This button has no action in preview.</p>
    </div>
    <button class="remove-block" type="button" @click="emit('remove', block.id)">Remove block</button>
  </fieldset>
</template>

<style scoped>
/* Keep property fields wide enough for their labels at narrow workspace sizes. */
.properties-fields,
.properties-fields > div {
  display: grid;
  min-width: 0;
  gap: 0.5rem;
}

.properties-fields {
  gap: 0.75rem;
}

.properties-fields :is(input, textarea) {
  width: 100%;
  padding: 0.55rem 0.8rem;
  border: 1px solid var(--text-muted);
  border-radius: var(--tool-field-radius);
  background: var(--surface-editor);
}

.properties-fields input {
  min-height: 44px;
}

.properties-fields textarea:focus-visible {
  outline: 2px solid var(--accent-strong);
  outline-offset: 2px;
}

.properties-note {
  color: var(--text-muted);
  font-size: 0.875rem;
}

.remove-block {
  min-height: 44px;
  justify-self: start;
  border-color: var(--status-error-indicator);
  border-radius: 1.5rem;
  color: var(--status-error-text);
}
</style>
