<!-- Adapts the shadcn-vue Badge primitive to the WITS status palette. -->
<script setup lang="ts">
import { Primitive } from "reka-ui";

// Default metadata to the neutral appearance.
withDefaults(defineProps<{
  variant?: "neutral" | "info" | "warning" | "success" | "error";
}>(), { variant: "neutral" });
</script>

<template>
  <Primitive
    as="span"
    class="tool-badge"
    data-slot="badge"
    :data-variant="variant"
  >
    <slot />
  </Primitive>
</template>
