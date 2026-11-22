<!-- Hosts the crawler and media views inside the existing dashboard shell. -->
<script setup lang="ts">
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from "reka-ui";
import { ref } from "vue";

import { useActiveToolCard } from "../../composables/use-active-tool-card";
import CrawlView from "../../components/crawler/CrawlView.vue";
import MediaDownloadsView from "../../components/crawler/MediaDownloadsView.vue";
import { useCrawlerJobs } from "../../composables/crawler/use-crawler-jobs";
import "../../styles/crawler/workspace.css";

// Keep jobs and selected results stable as the user moves between views.
defineOptions({ name: "CrawlerPage" });
const workspace = useCrawlerJobs();

// Clear the decorative selection when changing workflows.
const cardWorkspace = ref<HTMLElement>();
const clearActiveCard = useActiveToolCard(cardWorkspace);
const { native, refreshError } = workspace;
</script>

<template>
  <div ref="cardWorkspace" class="crawler-card-workspace tool-workspace">
    <TabsRoot
      class="crawler-workspace"
      default-value="crawl"
      :unmount-on-hide="false"
      @update:model-value="clearActiveCard"
    >
      <!-- Reuse keyboard navigation and accessible selection from the shared primitives. -->
      <TabsList class="crawler-tabs" aria-label="Crawler views">
        <TabsTrigger value="crawl">Crawl</TabsTrigger>
        <TabsTrigger value="media">Media Downloads</TabsTrigger>
      </TabsList>

      <p v-if="!native" class="crawler-notice">
        Open WITS Tools on the desktop to run crawls or downloads.
      </p>
      <p v-else-if="refreshError" class="crawler-notice crawler-error" role="status">
        {{ refreshError }}
      </p>

      <!-- Retain drafts and controls when switching between the two workflows. -->
      <TabsContent class="crawler-tab-content" value="crawl">
        <CrawlView :workspace="workspace" />
      </TabsContent>
      <TabsContent class="crawler-tab-content" value="media">
        <MediaDownloadsView :workspace="workspace" />
      </TabsContent>
    </TabsRoot>
  </div>
</template>
