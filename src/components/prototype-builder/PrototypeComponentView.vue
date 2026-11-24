<!-- Renders the validated definition using WITS-owned layout and text elements. -->
<script setup lang="ts">
import { computed } from "vue";

import type { ComponentElement } from "../../api/prototype-builder";
import { styleCss, type Style, type Theme } from "../../utils/prototype-builder/appearance";

const props = defineProps<{ element: ComponentElement; values: Record<string, string>; theme: Theme; override?: Style }>();
// Resolve named fields as plain text and apply overrides at the instance root.
const label = computed(() => "text" in props.element
  ? "field" in props.element.text ? props.values[props.element.text.field] ?? "" : props.element.text.literal
  : "");
const appearance = computed(() => styleCss({ ...props.element.style, ...props.override }, props.theme));
</script>

<template>
  <div v-if="'children' in element" class="composition" :class="element.kind" :style="appearance">
    <PrototypeComponentView v-for="(child, index) in element.children" :key="index" :element="child" :values="values" :theme="theme" />
  </div>
  <button v-else-if="element.kind === 'button'" class="composition-button" :style="appearance" type="button" disabled>{{ label }}</button>
  <p v-else class="composition-text" :style="appearance">{{ label }}</p>
</template>

<style scoped>
/* Keep the tree bounded by the selected node width and wrap authored content. */
.composition {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 0.5rem;
  min-width: 0;
  overflow-wrap: anywhere;
}

.composition.row {
  flex-flow: row wrap;
  align-items: center;
}

.composition-button:disabled {
  min-height: 44px;
  max-width: 100%;
  background: var(--accent-strong);
  color: var(--text-on-accent);
  opacity: 1;
}

.composition-text {
  min-width: 0;
  white-space: pre-wrap;
}
</style>
