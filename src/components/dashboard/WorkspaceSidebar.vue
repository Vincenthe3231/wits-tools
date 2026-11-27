<!-- Shows persistent dashboard and tool navigation. -->
<script setup lang="ts">
import { tools } from "../../const/dashboard/tools";
import WorkspaceIcon from "../ui/WorkspaceIcon.vue";

// List only live tools so every shortcut leads to a working page.
defineProps<{ editor?: boolean }>();
</script>

<template>
  <aside class="workspace-sidebar">
    <!-- Brand and dashboard links provide a stable way back from any tool. -->
    <RouterLink class="workspace-brand" :to="{ name: editor ? 'editor' : 'dashboard' }" :aria-label="editor ? 'Editor home' : 'WITS Tools home'">
      <span class="brand-mark"><WorkspaceIcon name="brand" /></span>
      <span>{{ editor ? "Editor" : "WITS Tools" }}</span>
    </RouterLink>

    <!-- Allow the active tool to use the sidebar track for its own navigation. -->
    <slot v-if="editor" />
    <nav v-else class="workspace-navigation" aria-label="Workspace">
      <RouterLink class="workspace-link" to="/">
        <WorkspaceIcon name="home" />
        Dashboard
      </RouterLink>

      <div class="workspace-shortcuts">
        <RouterLink
          v-for="tool in tools"
          :key="tool.routeName"
          class="workspace-link"
          :class="{ 'router-link-active': tool.routeName === 'editor' && ['markdown-editor', 'visual-editor'].includes(String($route.name)) }"
          :to="{ name: tool.routeName }"
        >
          <WorkspaceIcon :name="tool.icon" />
          {{ tool.name }}
        </RouterLink>
      </div>
    </nav>
  </aside>
</template>
