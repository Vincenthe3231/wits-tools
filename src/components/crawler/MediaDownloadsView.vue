<!-- Discovers media, enables selected-tab fallback, and tracks native downloads. -->
<script setup lang="ts">
import { ref, shallowRef } from "vue";

import { crawler } from "../../api/crawler";
import type { MediaSource, Pairing } from "../../api/crawler";
import type { useCrawlerJobs } from "../../composables/crawler/use-crawler-jobs";
import { notify } from "../../composables/use-notifications";
import { crawlerError } from "../../const/crawler/errors";
import { bytes, downloadActive, downloadPercent } from "../../utils/crawler/display";

const props = defineProps<{ workspace: ReturnType<typeof useCrawlerJobs> }>();
const { native, downloads, captured, captureActive, proxy, refresh } = props.workspace;

// Retain each workflow's input while switching modes and keep secrets out of displayed results.
const mode = ref<"page" | "direct">("page");
const url = ref("");
const folder = ref("");
const filename = ref("");
const format = ref("best");
const formats = shallowRef<{ id: string; bandwidth: number }[]>([]);
const overwrite = ref(false);
const discovering = ref(false);
const searched = ref(false);
const sources = shallowRef<MediaSource[]>([]);
const pairing = shallowRef<Pairing>();
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

async function download(source?: MediaSource, fromExtension = false) {
  await action("Could not start download", async () => {
    await crawler.download(request(source), source?.id ?? null, fromExtension);
    await refresh();
  });
}

// Inspect formats with the same selected media context used for a subsequent transfer.
function request(source?: MediaSource) {
  return { url: source ? "" : url.value.trim(), folder: folder.value || null,
    filename: filename.value.trim() || null, format: format.value.trim() || "best",
    proxy: proxy.value.trim() || null, overwrite: overwrite.value };
}

async function inspect(source?: MediaSource, fromExtension = false) {
  await action("Could not inspect formats", async () => {
    formats.value = (await crawler.mediaInfo(request(source), source?.id ?? null, fromExtension)).formats;
    if (!formats.value.length) notify({ severity: "info", title: "No selectable variants" });
  });
}

// Keep discovery independently cancellable while transfers continue in the background.
async function submit() {
  if (mode.value === "direct") return download();
  if (discovering.value) return;
  discovering.value = true;
  searched.value = false;
  sources.value = [];
  try {
    sources.value = await crawler.discover(url.value.trim(), proxy.value.trim() || null);
    searched.value = true;
  } catch (error) {
    if (!(error && typeof error === "object" && "code" in error && error.code === "cancelled")) {
      notify({ severity: "error", title: "Could not detect media", description: crawlerError(error) });
    }
  } finally {
    discovering.value = false;
  }
}

async function chooseFolder() {
  const selected = await crawler.pickFolder();
  if (selected) folder.value = selected;
}

async function enableCapture() {
  await crawler.startCapture();
  captureActive.value = true;
}

async function pair() {
  pairing.value = await crawler.pairCapture();
  captureActive.value = true;
}

async function stopCapture() {
  await crawler.stopCapture();
  captureActive.value = false;
  captured.value = [];
  pairing.value = undefined;
}
</script>

<template>
  <!-- Offer both a known media URL and browser-based discovery from a page URL. -->
  <form class="crawler-panel crawler-stack" @submit.prevent="submit">
    <div class="crawler-heading">
      <h1>Media Downloads</h1>
      <button class="primary-button" type="submit" :disabled="!native || !url.trim() || discovering || busy">
        {{ mode === "direct" ? "Download" : discovering ? "Detecting…" : "Detect media" }}
      </button>
    </div>
    <fieldset class="crawler-actions">
      <legend class="sr-only">URL type</legend>
      <label><input v-model="mode" type="radio" value="page" :disabled="discovering" /> Page URL</label>
      <label><input v-model="mode" type="radio" value="direct" :disabled="discovering" /> Direct media URL</label>
    </fieldset>
    <label class="crawler-field">
      <span>{{ mode === "page" ? "Page URL" : "Media file, HLS playlist, or DASH manifest URL" }}</span>
      <input v-model="url" type="url" placeholder="https://example.com" required :disabled="discovering" />
    </label>
    <div v-if="discovering" class="crawler-heading">
      <p class="crawler-meta" role="status">Waiting for media requests…</p>
      <button type="button" @click="action('Could not cancel discovery', crawler.cancelDiscovery)">Cancel discovery</button>
    </div>
    <details class="crawler-stack">
      <summary>Download settings</summary>
      <div class="crawler-grid crawler-advanced">
        <label class="crawler-field">
          <span>Save to folder</span>
          <input v-model="folder" placeholder="Windows Downloads" />
        </label>
        <button class="crawler-align-end" type="button" :disabled="!native || busy"
          @click="action('Could not open folder picker', chooseFolder)">Choose folder</button>
        <label class="crawler-field">
          <span>Filename</span>
          <input v-model="filename" placeholder="From media URL" />
        </label>
        <label class="crawler-field">
          <span>Format</span>
          <input v-model="format" list="crawler-formats" placeholder="best" />
          <datalist id="crawler-formats">
            <option value="best" />
            <option value="worst" />
            <option v-for="variant in formats" :key="variant.id" :value="variant.id">{{ variant.bandwidth }} bps</option>
          </datalist>
          <small>Enter best, worst, or a format ID.</small>
        </label>
        <label class="crawler-field">
          <span>Proxy URL</span>
          <input v-model="proxy" type="password" autocomplete="off" placeholder="Optional" />
        </label>
      </div>
      <button v-if="mode === 'direct'" type="button" :disabled="!native || !url.trim() || busy" @click="inspect()">Inspect formats</button>
      <label class="crawler-check"><input v-model="overwrite" type="checkbox" /> Replace an existing file after a successful download</label>
    </details>
  </form>

  <!-- Each detected manifest is a download choice. Observed segments stay grouped with it. -->
  <section v-if="searched || sources.length" class="crawler-stack" aria-labelledby="detected-media">
    <h2 id="detected-media">Detected media</h2>
    <p v-if="!sources.length" class="crawler-notice" role="status">
      No media requests detected. Try a direct media URL or selected-tab capture.
    </p>
    <ul v-else class="crawler-stack">
      <li v-for="source in sources" :key="source.id" class="crawler-panel crawler-heading">
        <div class="crawler-stack crawler-compact">
          <p class="crawler-url" :title="source.url">{{ source.url }}</p>
          <p class="crawler-meta">{{ source.kind.toUpperCase() }}<span v-if="source.segments"> · {{ source.segments }} observed segments</span></p>
        </div>
        <div class="crawler-actions">
          <button v-if="source.kind !== 'direct'" type="button" :disabled="busy" @click="inspect(source)">Inspect formats</button>
          <button type="button" :disabled="busy" @click="download(source)">Download</button>
        </div>
      </li>
    </ul>
  </section>

  <!-- Use the optional extension when playback depends on an already-open browser tab. -->
  <details class="crawler-panel crawler-stack">
    <summary>Selected-tab capture</summary>
    <p class="crawler-meta">Use the WITS Tools Capture extension to detect media in a tab you already have open.</p>
    <div class="crawler-actions">
      <button type="button" :disabled="!native || busy"
        @click="action('Could not register capture bridge', async () => { await crawler.registerCapture(); notify({ severity: 'success', title: 'Capture bridge registered' }); })">
        Register browser bridge
      </button>
      <button type="button" :disabled="!native || busy || captureActive"
        @click="action('Could not enable capture', enableCapture)">Enable capture</button>
      <button type="button" :disabled="!native || busy" @click="action('Could not create pairing code', pair)">Generate pairing code</button>
      <button v-if="captureActive" type="button" :disabled="busy"
        @click="action('Could not stop capture', stopCapture)">Stop capture</button>
    </div>
    <label v-if="pairing" class="crawler-field">
      <span>Pairing code</span>
      <input :value="pairing.code" readonly @focus="($event.target as HTMLInputElement).select()" />
      <small>Paste the code into the extension within {{ pairing.expires_in_seconds / 60 }} minutes.</small>
    </label>
    <p v-if="captureActive && !captured.length" class="crawler-meta" role="status">
      Start capture in the extension and play the media in that tab.
    </p>
    <ul v-if="captured.length" class="crawler-stack">
      <li v-for="source in captured" :key="source.id" class="crawler-heading">
        <div class="crawler-stack crawler-compact">
          <p class="crawler-url" :title="source.url">{{ source.url }}</p>
          <p class="crawler-meta">{{ source.kind.toUpperCase() }}</p>
        </div>
        <div class="crawler-actions">
          <button v-if="source.kind !== 'direct'" type="button" :disabled="busy" @click="inspect(source, true)">Inspect formats</button>
          <button type="button" :disabled="busy" @click="download(source, true)">Download</button>
        </div>
      </li>
    </ul>
  </details>

  <!-- Show transfer state, progress, and recovery details even after a notification closes. -->
  <section class="crawler-stack" aria-labelledby="media-history">
    <h2 id="media-history">Downloads</h2>
    <p v-if="!downloads.length" class="crawler-notice">No downloads yet.</p>
    <article v-for="job in downloads" :key="job.id" class="crawler-panel crawler-stack">
      <div class="crawler-heading">
        <p class="crawler-meta">
          <span class="crawler-status" :data-status="job.status">{{ job.status }}</span>
          {{ bytes(job.progress.bytes) }}<span v-if="job.progress.totalBytes"> of {{ bytes(job.progress.totalBytes) }}</span>
          <span v-if="job.progress.totalSegments"> · {{ job.progress.segments }} of {{ job.progress.totalSegments }} segments</span>
        </p>
        <button v-if="downloadActive(job)" type="button" :disabled="busy"
          @click="action('Could not cancel download', () => crawler.cancelDownload(job.id))">Cancel download</button>
      </div>
      <progress v-if="downloadActive(job)" :value="downloadPercent(job)" max="100" aria-label="Download progress" />
      <p v-if="job.output" class="crawler-meta crawler-url" :title="job.output">{{ job.output }}</p>
      <p v-if="job.error" class="crawler-error">{{ crawlerError(job.error) }}</p>
    </article>
  </section>
</template>
