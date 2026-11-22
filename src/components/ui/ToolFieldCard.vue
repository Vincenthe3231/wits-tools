<!-- Groups field guidance and controls in an inset child card. -->
<script setup lang="ts">
import Card from "./Card.vue";

// A single field can use the child title as its accessible label.
defineProps<{ title: string; description: string; labelFor?: string }>();
</script>

<template>
  <Card variant="child" class="tool-field-card">
    <!-- Keep explanation alongside the controls at wide sizes. -->
    <div class="tool-field-intro">
      <label v-if="labelFor" :for="labelFor">{{ title }}</label>
      <h3 v-else>{{ title }}</h3>
      <p>{{ description }}</p>
    </div>

    <!-- Slot the original controls so form behavior stays with the tool. -->
    <div class="tool-field-controls">
      <slot />
    </div>
  </Card>
</template>
