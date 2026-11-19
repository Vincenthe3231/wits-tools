<script setup lang="ts">
defineProps<{
  caret?: { transform: string; height: string; transition?: string };
  trail?: { id: number; transform: string; width: string };
}>();

const emit = defineEmits<{ trailFinished: [id: number] }>();
</script>

<template>
  <span
    v-if="trail"
    :key="trail.id"
    class="caret-trail"
    :style="{ transform: trail.transform, width: trail.width }"
    aria-hidden="true"
    @animationend="emit('trailFinished', trail.id)"
  />
  <span v-if="caret" class="animated-caret" :style="caret" aria-hidden="true" />
</template>

<style scoped>
.caret-trail {
  position: absolute;
  inset: 0 auto auto 0;
  height: 3px;
  z-index: 1;
  pointer-events: none;
  transform-origin: left center;
  background: linear-gradient(to right, var(--accent-strong), transparent);
  animation: fade-caret-trail 170ms ease-out both;
}

@keyframes fade-caret-trail {
  from {
    opacity: 0.55;
  }

  to {
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce), (forced-colors: active) {
  .caret-trail {
    display: none;
  }
}
</style>
