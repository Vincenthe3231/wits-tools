<!-- Shows persistent dashboard and tool navigation. -->
<script setup lang="ts">
import { tools } from "../../const/dashboard/tools";
import WorkspaceIcon from "./WorkspaceIcon.vue";

// List only live tools so every shortcut leads to a working page.
</script>

<template>
  <aside class="workspace-sidebar">
    <!-- Brand and dashboard links provide a stable way back from any tool. -->
    <RouterLink class="workspace-brand" to="/" aria-label="WITS Tools home">
      <span class="brand-mark"><WorkspaceIcon name="brand" /></span>
      <span>WITS Tools</span>
    </RouterLink>

    <!-- Keep the dashboard above the live tool shortcuts. -->
    <nav class="workspace-navigation" aria-label="Workspace">
      <RouterLink class="workspace-link" to="/">
        <WorkspaceIcon name="home" />
        Dashboard
      </RouterLink>

      <div class="workspace-shortcuts">
        <RouterLink
          v-for="tool in tools"
          :key="tool.routeName"
          class="workspace-link"
          :to="{ name: tool.routeName }"
        >
          <WorkspaceIcon :name="tool.icon" />
          {{ tool.name }}
        </RouterLink>
      </div>
    </nav>
  </aside>
</template>
