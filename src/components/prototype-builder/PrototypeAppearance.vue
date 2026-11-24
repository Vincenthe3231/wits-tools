<!-- Edits the authored project theme or the selected component's supported appearance. -->
<script setup lang="ts">
import { computed } from "vue";

import type { CanvasNode, ComponentDefinition } from "../../api/prototype-builder";
import { roles, styleFields, validStyle, type FontChoice, type Style, type Theme } from "../../utils/prototype-builder/appearance";

const props = defineProps<{ theme: Theme; node?: CanvasNode; definition?: ComponentDefinition; disabled?: boolean }>();
const emit = defineEmits<{ theme: [value: Theme]; style: [value: Style] }>();

// Show layout values only on primitives whose layout can respond to them.
const fields = computed(() => {
  const kind = props.definition?.root.kind ?? props.node?.block.kind;
  const layout = kind === "stack" || kind === "row" || kind === "card" || kind === "signIn";
  return (Object.keys(styleFields) as (keyof Style)[]).filter((key) => key !== "gap" || layout);
});
const roleLabels: Record<typeof roles[number], string> = {
  screen: "Screen", surface: "Component surface", text: "Text", muted: "Muted text",
  border: "Border", accent: "Accent", onAccent: "Text on accent",
};

function numeric(key: keyof Style) {
  return key !== "fill" && key !== "color" && key !== "border";
}

function updateStyle(key: keyof Style, event: Event) {
  const input = event.target as HTMLInputElement;
  const style = { ...props.node?.style };
  if (!input.value.trim()) delete style[key];
  else Object.assign(style, { [key]: numeric(key) ? input.valueAsNumber : input.value.trim() });
  if (!input.validity.valid || !validStyle(style)) {
    input.setCustomValidity("Enter a valid value or clear this override.");
    input.reportValidity();
    return;
  }
  emit("style", style);
}
</script>

<template>
  <!-- Keep color inputs scoped to the document and optional overrides easy to clear. -->
  <fieldset v-if="!node" class="appearance-fields" :disabled="disabled">
    <legend>Project theme</legend>
    <label v-for="role in roles" :key="role" class="theme-color">
      {{ roleLabels[role] }}
      <input type="color" :value="theme[role]" @change="emit('theme', { ...theme, [role]: ($event.target as HTMLInputElement).value })" />
    </label>
    <label class="style-field">
      Font
      <select :value="theme.font" @change="emit('theme', { ...theme, font: ($event.target as HTMLSelectElement).value as FontChoice })">
        <option value="system">System sans serif</option>
        <option value="serif">Georgia</option>
        <option value="mono">Consolas</option>
      </select>
    </label>
  </fieldset>
  <fieldset v-else class="appearance-fields" :disabled="disabled">
    <legend>Component appearance</legend>
    <p class="style-hint">Clear a value to restore its default. Colors accept #RRGGBB or a theme token.</p>
    <label v-for="key in fields" :key="key" class="style-field">
      {{ styleFields[key] }}
      <input
        :type="numeric(key) ? 'number' : 'text'"
        :value="node.style?.[key] ?? ''"
        :min="key === 'weight' ? 100 : key === 'fontSize' ? 1 : 0"
        :max="key === 'weight' ? 900 : 512"
        placeholder="Default"
        @input="($event.target as HTMLInputElement).setCustomValidity('')"
        @change="updateStyle(key, $event)"
      />
    </label>
  </fieldset>
</template>

<style scoped>
/* Use existing tool field tokens for style controls, independently of authored values. */
.appearance-fields,
.style-field {
  display: grid;
  min-width: 0;
  gap: 0.5rem;
}

.appearance-fields legend {
  font-weight: 650;
}

.theme-color {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
}

.style-field :is(input, select),
.theme-color input {
  min-height: 44px;
  border: 1px solid var(--text-muted);
  border-radius: var(--tool-field-radius);
  background: var(--surface-editor);
}

.style-field :is(input, select) {
  min-width: 0;
  width: 100%;
  padding-inline: 0.75rem;
}

.theme-color input {
  width: 3rem;
  padding: 0.4rem;
}

.style-hint {
  color: var(--text-muted);
  font-size: 0.875rem;
}
</style>
