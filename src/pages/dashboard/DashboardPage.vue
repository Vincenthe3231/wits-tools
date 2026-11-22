<!-- Presents the searchable live tool catalog in a left-aligned dashboard. -->
<script setup lang="ts">
import { computed } from "vue";

import WorkspaceIcon from "../../components/dashboard/WorkspaceIcon.vue";
import { searchTools } from "../../utils/dashboard/tool-search";

// The toolbar supplies the catalog filter, leaving the tool links available.
const props = defineProps<{ query: string }>();
const matches = computed(() => searchTools(props.query));
</script>

<template>
  <main class="tools-dashboard">
    <!-- The broad opening band introduces the workspace and its primary action. -->
    <section class="dashboard-intro" aria-labelledby="dashboard-title">
      <div>
        <h1 id="dashboard-title">Your toolkit</h1>
        <p>Edit Markdown, test an API, or crawl a website.</p>
        <RouterLink class="dashboard-primary" :to="{ name: 'markdown-editor' }">
          Open Markdown Editor
          <WorkspaceIcon name="arrow" />
        </RouterLink>
      </div>
      <span class="dashboard-emblem" aria-hidden="true"><WorkspaceIcon name="brand" /></span>
    </section>

    <!-- The shorter second band leads into a left-aligned list of live tools. -->
    <section class="dashboard-catalog" aria-labelledby="catalog-title">
      <h2 id="catalog-title">All tools</h2>
      <ul v-if="matches.length" class="dashboard-tool-list">
        <li v-for="tool in matches" :key="tool.routeName">
          <RouterLink class="dashboard-tool" :to="{ name: tool.routeName }">
            <span class="dashboard-tool-icon"><WorkspaceIcon :name="tool.icon" /></span>
            <span>
              <strong>{{ tool.name }}</strong>
              <span class="dashboard-tool-description">{{ tool.description }}</span>
            </span>
            <WorkspaceIcon class="dashboard-tool-arrow" name="arrow" />
          </RouterLink>
        </li>
      </ul>
      <p v-else class="workspace-no-results" role="status">No tools match your search.</p>
    </section>
  </main>
</template>
