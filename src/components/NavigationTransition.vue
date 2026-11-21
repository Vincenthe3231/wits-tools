<!-- Shares navigation entrance motion while callers retain control of visibility and state. -->
<script setup lang="ts">
// Each surface uses the shared motion preset that matches how far it moves.
defineProps<{
  kind: "page" | "panel" | "sidebar" | "menu";
  direction?: "left" | "right" | "up" | "down";
}>();
</script>

<template>
  <!-- A single slotted root receives the Vue transition classes. -->
  <Transition
    :name="`navigation-${kind}`"
    :enter-from-class="`navigation-${kind}-enter-from${direction ? ' navigation-from-' + direction : ''}`"
  >
    <slot />
  </Transition>
</template>
