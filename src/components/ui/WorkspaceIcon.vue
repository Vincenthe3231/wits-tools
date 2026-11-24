<!-- Draws a consistent set of decorative line icons for the workspace. -->
<script setup lang="ts">
// Icons inherit the label and color of the control or surface around them.
defineProps<{
  name: "brand" | "home" | "document" | "search" | "arrow" | "sun" | "theme" | "sidebar" | "globe" | "trash";
}>();
</script>

<template>
  <!-- The neighboring text or control name supplies the accessible label. -->
  <svg
    aria-hidden="true"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.65"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    <path v-if="name === 'brand'" d="m3 7 9-4 9 4-9 4-9-4Zm0 5 9 4 9-4M3 17l9 4 9-4" />
    <path
      v-else-if="name === 'home'"
      d="M3.5 4h7v7h-7zM13.5 4h7v7h-7zM3.5 14h7v6.5h-7zM13.5 14h7v6.5h-7z"
    />
    <path v-else-if="name === 'document'" d="M6 3.5h8l4 4v13H6zM14 3.5v4h4M9 12h6m-6 4h6" />
    <path
      v-else-if="name === 'search'"
      d="M20 20l-4.25-4.25M17 10.5a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0Z"
    />
    <path v-else-if="name === 'arrow'" d="M5 12h14m-6-6 6 6-6 6" />
    <path v-else-if="name === 'trash'" d="M4 7h16m-14 0 1 13h10l1-13M9 7V4h6v3m-5 3v7m4-7v7" />
    <!-- Use the shared outline style for the Crawler shortcut. -->
    <path
      v-else-if="name === 'globe'"
      d="M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM3 12h18m-9-9a13 13 0 0 1 0 18 13 13 0 0 1 0-18Z"
    />
    <path
      v-else-if="name === 'sun'"
      d="M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0ZM12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5"
    />
    <path v-else-if="name === 'sidebar'" d="M5 5h14a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Zm5 0v14" />
    <path v-else d="M12 3a9 9 0 1 0 9 9 7 7 0 0 1-9-9Z" />
  </svg>
</template>
