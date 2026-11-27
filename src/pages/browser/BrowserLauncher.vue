<!-- Opens the private browser while keeping the workspace available. -->
<script setup lang="ts">
import { onMounted, ref } from "vue";

import { browser } from "../../api/browser";
import { notify } from "../../composables/use-notifications";
import Card from "../../components/ui/Card.vue";
import CardContent from "../../components/ui/CardContent.vue";

const opening = ref(false);

// Reuse this launch action for navigation and for reopening after a browser close.
async function openBrowser() {
  if (opening.value) return;
  opening.value = true;
  try {
    await browser.open();
  } catch (error) {
    notify({ id: "browser-open", severity: "error", title: "Could not open the browser", description: String(error) });
  } finally {
    opening.value = false;
  }
}

onMounted(() => { void openBrowser(); });
</script>

<template>
  <Card>
    <CardContent class="flex flex-col items-start gap-3 p-5">
      <p class="text-sm text-[var(--text-muted)]">Browsing data clears when you close the private browser window.</p>
      <button
        type="button"
        class="min-h-11 rounded-full bg-[var(--accent-strong)] px-4 text-sm font-semibold text-[var(--text-on-accent)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--accent-strong)]"
        :disabled="opening"
        @click="openBrowser"
      >{{ opening ? "Opening…" : "Open Browser" }}</button>
    </CardContent>
  </Card>
</template>
