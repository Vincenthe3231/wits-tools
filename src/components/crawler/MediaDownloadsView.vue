<!-- Captures media in WebView2 and tracks native downloads. -->
<script setup lang="ts">
import { ref } from "vue";

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
  <form class="crawler-panel crawler-stack" @submit.prevent="submit">
    <div class="crawler-heading">
      <h1>Media Downloads</h1>
      <button class="primary-button" type="submit" :disabled="!native || !url.trim() || busy || !workersValid">
        {{ mode === "direct" ? "Download" : "Capture" }}
      </button>
    </div>
    <fieldset class="crawler-actions">
      <legend class="sr-only">URL type</legend>
      <label><input v-model="mode" type="radio" value="page" /> Page URL</label>
      <label><input v-model="mode" type="radio" value="direct" /> Direct media URL</label>
    </fieldset>
    <label class="crawler-field">
      <span>{{ mode === "page" ? "Page URL" : "Media file, HLS playlist, or DASH manifest URL" }}</span>
      <input v-model="url" type="url" placeholder="https://example.com" required />
    </label>
    <details class="crawler-stack">
      <summary>Download settings</summary>
      <div class="form-grid crawler-advanced">
        <!-- Use the same full-width control as Page URL and put the picker beside its label. -->
        <div class="crawler-field form-grid-full">
          <div class="crawler-field-heading">
            <label for="media-save-folder">Save to folder</label>
            <button
              class="crawler-folder-button"
              type="button"
              :disabled="!native || busy"
              @click="action('Could not open folder picker', chooseFolder)"
            >
              Choose folder
            </button>
          </div>
          <input id="media-save-folder" v-model="folder" placeholder="Windows Downloads" />
        </div>
        <label class="crawler-field">
          <span>Filename</span>
          <input v-model="filename" :placeholder="mode === 'page' ? 'From page title or URL' : 'From media URL'" />
        </label>
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
      </div>
      <label class="crawler-check"><input v-model="overwrite" type="checkbox" /> Replace an existing file after a successful download</label>
    </details>
  </form>

  <!-- Show navigation feedback and streams from the current capture window. -->
  <section v-if="browserCapture.status !== 'closed'" class="crawler-panel crawler-stack" aria-labelledby="browser-sources">
    <h2 id="browser-sources">Captured streams</h2>
    <p v-if="browserCapture.status === 'loading'" class="crawler-meta" role="status">Opening the page…</p>
    <p v-else-if="browserCapture.status === 'failed'" class="crawler-error" role="status">The capture browser could not load the page. Check the address or network settings and try again.</p>
    <p v-else-if="!browserCapture.sources.length" class="crawler-meta" role="status">Play the media in the capture browser to detect streams.</p>
    <ul v-if="browserCapture.sources.length" class="crawler-stack">
      <li v-for="source in browserCapture.sources" :key="source.id" class="crawler-heading">
        <div class="crawler-stack crawler-compact">
          <p class="crawler-url" :title="source.url">{{ source.url }}</p>
          <p class="crawler-meta">{{ source.kind.toUpperCase() }} · {{ source.durationSeconds == null ? "Duration unavailable" : "Duration " + mediaTime(source.durationSeconds) }}
            · Captured {{ new Date(source.capturedAt * 1000).toLocaleString() }}
          </p>
        </div>
        <div class="crawler-actions">
          <button type="button" :disabled="busy || !workersValid" @click="download(source)">Download</button>
        </div>
      </li>
    </ul>
  </section>

  <!-- Show transfer state, progress, and recovery details even after a notification closes. -->
  <section class="crawler-stack" aria-labelledby="media-history">
    <h2 id="media-history">Downloads</h2>
    <p v-if="!downloads.length" class="crawler-notice">No downloads yet.</p>
    <article v-for="job in downloads" :key="job.id" class="crawler-panel crawler-stack">
      <div class="crawler-heading">
        <p class="crawler-meta">
          <span class="crawler-status" :data-status="job.status">{{ job.status }}</span>
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
    </article>
  </section>
</template>
