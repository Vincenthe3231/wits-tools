<!-- Keeps one accessible search control for the sidebar and its collapsed fallback. -->
<script setup lang="ts">
import WorkspaceIcon from "../ui/WorkspaceIcon.vue";

const query = defineModel<string>({ required: true });
</script>

<template>
  <label class="editor-library-search">
    <WorkspaceIcon name="search" />
    <span class="sr-only">Search saved documents</span>
    <input v-model="query" type="search" placeholder="Search" />
  </label>
</template>
