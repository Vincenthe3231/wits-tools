<!-- Adapts the shadcn-vue Card slot to WITS workspace surfaces. -->
<script setup lang="ts">
// Use the outer surface by default and opt into inset children.
withDefaults(defineProps<{
  as?: "div" | "section" | "article" | "li";
  variant?: "outer" | "child";
}>(), { as: "div", variant: "outer" });
</script>

<template>
  <component
    :is="as"
    data-slot="card"
    :data-card-variant="variant"
    class="tool-card"
  >
    <!-- Keep the decorative motion layer outside layout and accessibility. -->
    <span v-if="variant === 'outer'" class="tool-card-motion" aria-hidden="true" />
    <slot />
  </component>
</template>
