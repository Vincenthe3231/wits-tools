<!-- Renders the spring-driven caret head and its decorative smear in either editor. -->
<script setup lang="ts">
// The motion composable supplies transforms so neither layer participates in layout.
defineProps<{
  caret?: { transform: string; height: string };
  trail?: { transform: string; opacity: number };
}>();
</script>

<template>
  <!-- Paint the trailing shadow behind the solid insertion point. -->
  <span v-if="trail" class="caret-trail" :style="trail" aria-hidden="true" />
  <span v-if="caret" class="animated-caret" :style="caret" aria-hidden="true" />
</template>

<style scoped>
/* Stretch a one-pixel gradient with a transform to avoid reflow on every frame. */
.caret-trail {
  position: absolute;
  inset: 0 auto auto 0;
  width: 1px;
  height: 6px;
  z-index: 1;
  pointer-events: none;
  transform-origin: 0 3px;
  background: linear-gradient(to right, var(--accent-strong), transparent);
}

/* Keep the real insertion marker visible independently of the fading tail. */
.animated-caret {
  position: absolute;
  inset: 0 auto auto 0;
  width: 4px;
  z-index: 1;
  border-radius: 2px;
  pointer-events: none;
  background: var(--accent-strong);
}

/* Use the native cursor whenever motion or custom colors are disabled. */
@media (prefers-reduced-motion: reduce), (forced-colors: active) {
  .caret-trail,
  .animated-caret {
    display: none;
  }
}
</style>
