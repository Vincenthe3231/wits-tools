<!-- Captures media in WebView2 and tracks native downloads. -->
<script setup lang="ts">
import { ref } from "vue";

import Badge from "../ui/Badge.vue";
import Card from "../ui/Card.vue";
import CardHeader from "../ui/CardHeader.vue";
import ToolFieldCard from "../ui/ToolFieldCard.vue";
import CardContent from "../ui/CardContent.vue";
import AppInputSelect from "../AppInputSelect.vue";
import AppSlider from "../AppSlider.vue";
import { crawler } from "../../api/crawler";
import type { MediaSource } from "../../api/crawler";
import type { useCrawlerJobs } from "../../composables/crawler/use-crawler-jobs";
import { notify } from "../../composables/use-notifications";
import { crawlerError } from "../../const/crawler/errors";
import { DNS_PRESETS } from "../../const/crawler/network-presets";
import { bytes, downloadActive, downloadPercent, mediaTime } from "../../utils/crawler/display";

const props = defineProps<{ workspace: ReturnType<typeof useCrawlerJobs> }>();
const { native, downloads, browserCapture, refresh } = props.workspace;

// Retain each workflow's input while switching modes and keep secrets out of displayed results.
const proxy = ref("");
const mode = ref<"page" | "direct">("page");
const url = ref("");
const folder = ref("");
const filename = ref("");
const format = ref("best");
const overwrite = ref(false);
const workers = ref(4);
const workersValid = ref(true);
const busy = ref(false);

async function action(title: string, run: () => Promise<unknown>) {
  if (busy.value) return;
  busy.value = true;
  try {
    await run();
  } catch (error) {
    notify({ severity: "error", title, description: crawlerError(error) });
  } finally {
    busy.value = false;
  }
}

async function download(source?: MediaSource) {
  if (!workersValid.value) return;
  await action("Could not start download", async () => {
    await crawler.download(request(source), source?.id ?? null);
    await refresh();
  });
}

function request(source?: MediaSource) {
  return { url: source ? "" : url.value.trim(), folder: folder.value || null,
    filename: filename.value.trim() || null, format: format.value.trim() || "best",
    proxy: proxy.value.trim() || null, overwrite: overwrite.value, workers: workers.value };
}

// Start the private capture session or download a known media URL.
async function submit() {
  if (!workersValid.value) return;
  if (mode.value === "direct") return download();
  await action("Could not open capture browser", async () => {
    await crawler.openCaptureBrowser(url.value.trim(), proxy.value.trim() || null);
    await refresh();
  });
}

async function chooseFolder() {
  const selected = await crawler.pickFolder();
  if (selected) folder.value = selected;
}

</script>

<template>
  <!-- Offer both a known media URL and browser-based discovery from a page URL. -->
  <form @submit.prevent="submit">
    <Card>
      <CardHeader class="crawler-heading">
        <div>
          <h1>Media Downloads</h1>
          <p class="tool-card-description">Capture streams from a page or download from a media URL.</p>
        </div>
        <button class="primary-button" type="submit" :disabled="!native || !url.trim() || busy || !workersValid">
          {{ mode === "direct" ? "Download" : "Capture" }}
        </button>
      </CardHeader>
      <CardContent>
        <ToolFieldCard
          title="Source"
          description="Choose a source type and enter its address."
        >
          <fieldset class="crawler-actions">
            <legend class="sr-only">URL type</legend>
            <label><input v-model="mode" type="radio" value="page" /> Page URL</label>
            <label><input v-model="mode" type="radio" value="direct" /> Direct media URL</label>
          </fieldset>
          <label class="crawler-field">
            <span>{{ mode === "page" ? "Page URL" : "Media file, HLS playlist, or DASH manifest URL" }}</span>
            <input v-model="url" type="url" placeholder="https://example.com" required />
          </label>
        </ToolFieldCard>
        <details class="crawler-stack">
          <summary>Download settings</summary>
          <div class="crawler-advanced tool-field-controls">
            <ToolFieldCard
              title="Destination"
              description="Set the save location and filename."
            >
              <div class="crawler-field form-grid-full">
                <label for="media-save-folder">Save to folder</label>
                <div class="crawler-inline-control">
                  <input id="media-save-folder" v-model="folder" placeholder="Windows Downloads" />
                  <button
                    class="crawler-folder-button"
                    type="button"
                    :disabled="!native || busy"
                    @click="action('Could not open folder picker', chooseFolder)"
                  >
                    Choose folder
                  </button>
                </div>
              </div>
              <label class="crawler-field">
                <span>Filename</span>
                <input v-model="filename" :placeholder="mode === 'page' ? 'From page title or URL' : 'From media URL'" />
              </label>
            </ToolFieldCard>
            <ToolFieldCard
              title="Format"
              description="Choose a preset or enter a format ID."
            >
              <!-- Keep common format presets and allow an explicit format ID. -->
              <AppInputSelect
                id="media-format"
                v-model="format"
                class="crawler-field"
                label="Format"
                placeholder="best"
                help="Enter best, worst, or a format ID."
                :options="[
                  { value: 'best', label: 'Best' },
                  { value: 'worst', label: 'Worst' },
                ]"
              />
            </ToolFieldCard>
            <ToolFieldCard
              title="Connection"
              description="Set a proxy or DNS resolver for this source."
            >
              <AppInputSelect
                id="media-proxy"
                v-model="proxy"
                class="crawler-field"
                label="Proxy or DNS"
                type="password"
                autocomplete="off"
                placeholder="Proxy URL or DNS endpoint"
                help="Reopen the capture browser after changing this setting."
                :options="DNS_PRESETS"
              />
            </ToolFieldCard>
            <ToolFieldCard
              title="Download options"
              description="Set parallel workers and whether files may be replaced."
            >
              <div class="form-grid form-grid-full form-grid-sliders">
                <AppSlider
                  id="media-workers"
                  v-model="workers"
                  v-model:valid="workersValid"
                  class="crawler-field"
                  label="Download workers"
                  :min="1"
                  :max="16"
                  help="Parallel segments or byte ranges per file, when supported."
                />
              </div>
              <label class="crawler-check"><input v-model="overwrite" type="checkbox" /> Replace an existing file after a successful download</label>
            </ToolFieldCard>
          </div>
        </details>
      </CardContent>
    </Card>
  </form>

  <!-- Show navigation feedback and streams from the current capture window. -->
  <Card as="section" v-if="browserCapture.status !== 'closed'" aria-labelledby="browser-sources">
    <CardHeader>
      <h2 id="browser-sources">Captured streams</h2>
      <p class="tool-card-description">Download a stream detected in the capture browser.</p>
    </CardHeader>
    <CardContent>
      <p v-if="browserCapture.status === 'loading'" class="crawler-meta" role="status">Opening the page…</p>
      <p v-else-if="browserCapture.status === 'failed'" class="crawler-error" role="status">The capture browser could not load the page. Check the address or network settings and try again.</p>
      <p v-else-if="!browserCapture.sources.length" class="crawler-meta" role="status">Play the media in the capture browser to detect streams.</p>
      <ul v-if="browserCapture.sources.length" class="tool-card-children">
        <Card as="li" v-for="source in browserCapture.sources" :key="source.id" variant="child" class="tool-card-compact">
          <div class="crawler-heading">
            <div class="crawler-stack crawler-compact">
              <p class="crawler-url" :title="source.url">{{ source.url }}</p>
              <p class="crawler-meta"><Badge>{{ source.kind.toUpperCase() }}</Badge> {{ source.durationSeconds == null ? "Duration unavailable" : "Duration " + mediaTime(source.durationSeconds) }}
                · Captured {{ new Date(source.capturedAt * 1000).toLocaleString() }}
              </p>
            </div>
            <div class="crawler-actions">
              <button type="button" :disabled="busy || !workersValid" @click="download(source)">Download</button>
            </div>
          </div>
        </Card>
      </ul>
    </CardContent>
  </Card>

  <!-- Show transfer state, progress, and recovery details even after a notification closes. -->
  <Card as="section" aria-labelledby="media-history">
    <CardHeader>
      <div>
        <h2 id="media-history">Downloads</h2>
        <p class="tool-card-description">Track progress and review completed downloads.</p>
      </div>
    </CardHeader>
    <CardContent>
      <p v-if="!downloads.length" class="crawler-notice">No downloads yet.</p>
      <Card as="article" v-for="job in downloads" :key="job.id" variant="child" class="tool-card-compact">
        <div class="crawler-heading">
          <p class="crawler-meta">
            <Badge class="crawler-status" :variant="job.status === 'completed' ? 'success' : job.status === 'failed' ? 'error' : job.status === 'downloading' ? 'info' : 'neutral'">{{ job.status }}</Badge>
            {{ bytes(job.fileSize ?? job.progress.bytes) }}<span v-if="job.fileSize == null && job.progress.totalBytes != null"> of {{ bytes(job.progress.totalBytes) }}</span>
            <span v-if="job.progress.totalSegments"> · {{ job.progress.segments }} of {{ job.progress.totalSegments }} segments</span>
          </p>
          <button v-if="downloadActive(job)" type="button" :disabled="busy"
            @click="action('Could not cancel download', () => crawler.cancelDownload(job.id))">Cancel download</button>
        </div>
        <progress v-if="downloadActive(job)" :value="downloadPercent(job)" max="100" aria-label="Download progress" />
        <p class="crawler-meta">
          {{ job.durationSeconds == null ? "Duration unavailable" : "Duration " + mediaTime(job.durationSeconds) }}
          <span v-if="downloadActive(job)">
            · {{ job.etaSeconds != null ? "ETA " + mediaTime(job.etaSeconds) :
              job.progress.totalBytes || job.progress.totalSegments || job.status === "queued" ? "Estimating…" : "ETA unavailable" }}
          </span>
        </p>
        <p v-if="job.output" class="crawler-meta crawler-url" :title="job.output">{{ job.output }}</p>
        <p v-if="job.error" class="crawler-error">{{ crawlerError(job.error) }}</p>
      </Card>
    </CardContent>
  </Card>
</template>
