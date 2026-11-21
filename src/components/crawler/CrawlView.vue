<!-- Starts crawl jobs and provides separate, paginated views of pages and assets. -->
<script setup lang="ts">
import { ref } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";

import { crawler } from "../../api/crawler";
import type { CrawlJob, CrawlMode } from "../../api/crawler";
import type { useCrawlerJobs } from "../../composables/crawler/use-crawler-jobs";
import { notify } from "../../composables/use-notifications";
import { crawlerError } from "../../const/crawler/errors";
import { bytes, crawlActive, items } from "../../utils/crawler/display";

const props = defineProps<{ workspace: ReturnType<typeof useCrawlerJobs> }>();
const { native, jobs, results, selected, pageOffset, assetOffset, proxy, refresh, refreshResults, selectJob } = props.workspace;

// Start with the source crawler defaults and leave the proxy unset until the user provides one.
const seedUrls = ref("");
const mode = ref<CrawlMode>("auto");
const maxPages = ref(100);
const maxDepth = ref(5);
const concurrency = ref(2);
const delay = ref(500);
const domains = ref("");
const exclusions = ref("");
const respectRobots = ref(true);
const assets = ref(true);
// Keep the chosen destination while the user adjusts the crawl settings.
const assetFolder = ref("");
const starting = ref(false);
const busy = ref(false);
const deleteDialog = ref<HTMLDialogElement>();
const pendingDelete = ref<CrawlJob>();
const pageSize = 50;

// Run one history action at a time to keep repeated clicks from racing native operations.
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

// Closing the native picker preserves the existing destination.
async function chooseFolder() {
  const folder = await crawler.pickFolder();
  if (folder) assetFolder.value = folder;
}

async function start() {
  if (starting.value || busy.value) return;
  starting.value = true;
  try {
    await crawler.start({
      startUrls: items(seedUrls.value), mode: mode.value, maxPages: maxPages.value,
      maxDepth: maxDepth.value, concurrency: concurrency.value, requestDelayMillis: delay.value,
      proxyUrl: proxy.value.trim() || null, allowedDomains: items(domains.value),
      excludePatterns: items(exclusions.value), respectRobotsTxt: respectRobots.value, downloadAssets: assets.value,
      assetFolder: assets.value ? assetFolder.value.trim() || null : null,
    });
    await refresh();
  } catch (error) {
    notify({ severity: "error", title: "Could not start crawl", description: crawlerError(error) });
  } finally {
    starting.value = false;
  }
}

function confirmDelete(job: CrawlJob) {
  pendingDelete.value = job;
  deleteDialog.value?.showModal();
}

// Keep the confirmation scoped to the displayed job even if polling refreshes the list.
function finishDelete() {
  const job = pendingDelete.value;
  const confirmed = deleteDialog.value?.returnValue === "delete";
  pendingDelete.value = undefined;
  if (!confirmed || !job) return;
  void action("Could not delete crawl", async () => {
    await crawler.remove(job.id);
    if (selected.value === job.id) await selectJob(undefined);
    await refresh();
  });
}

function paginate(kind: "pages" | "assets", step: number) {
  const offset = kind === "pages" ? pageOffset : assetOffset;
  offset.value = Math.max(0, offset.value + step * pageSize);
  void action("Could not load results", refreshResults);
}
</script>

<template>
  <!-- Put the requested URLs and primary action first, with advanced options one step away. -->
  <form class="crawler-panel crawler-stack" @submit.prevent="start">
    <div class="crawler-heading">
      <h1>New crawl</h1>
      <button class="primary-button" type="submit" :disabled="!native || !seedUrls.trim() || starting || busy">
        {{ starting ? "Starting…" : "Start crawl" }}
      </button>
    </div>
    <label class="crawler-field">
      <span>Start URLs</span>
      <textarea v-model="seedUrls" rows="2" required placeholder="https://example.com" />
      <small>Enter one URL per line.</small>
    </label>
    <div class="crawler-grid">
      <label class="crawler-field">
        <span>Mode</span>
        <select v-model="mode">
          <option value="auto">Auto</option>
          <option value="always">Browser</option>
          <option value="never">Static</option>
        </select>
      </label>
      <label class="crawler-field">
        <span>Max pages</span>
        <input v-model.number="maxPages" type="number" min="1" max="4294967295" required />
      </label>
      <label class="crawler-field">
        <span>Max depth</span>
        <input v-model.number="maxDepth" type="number" min="0" max="4294967295" required />
      </label>
    </div>
    <!-- Keep the asset location visible alongside the basic crawl settings. -->
    <div class="crawler-grid">
      <label class="crawler-field">
        <span>Download path</span>
        <input
          v-model="assetFolder"
          placeholder="WITS Tools app data"
          :disabled="!assets || starting || busy"
        />
      </label>
      <button
        class="crawler-align-end"
        type="button"
        :disabled="!native || !assets || starting || busy"
        @click="action('Could not open folder picker', chooseFolder)"
      >
        Choose folder
      </button>
    </div>
    <details class="crawler-stack">
      <summary>Advanced settings</summary>
      <div class="crawler-grid crawler-advanced">
        <label class="crawler-field">
          <span>Concurrency</span>
          <input v-model.number="concurrency" type="number" min="1" max="20" required />
        </label>
        <label class="crawler-field">
          <span>Request delay (ms)</span>
          <input v-model.number="delay" type="number" min="0" required />
        </label>
        <label class="crawler-field">
          <span>Proxy URL</span>
          <input v-model="proxy" type="password" autocomplete="off" placeholder="Optional" />
        </label>
        <label class="crawler-field">
          <span>Allowed domains</span>
          <textarea v-model="domains" rows="2" placeholder="One domain per line" />
        </label>
        <label class="crawler-field">
          <span>Exclude patterns</span>
          <textarea v-model="exclusions" rows="2" placeholder="One regular expression per line" />
        </label>
      </div>
      <div class="crawler-actions">
        <label><input v-model="respectRobots" type="checkbox" /> Respect robots.txt</label>
        <label><input v-model="assets" type="checkbox" /> Download assets</label>
      </div>
    </details>
  </form>

  <!-- Keep result tables next to the selected job while the list updates. -->
  <section class="crawler-stack" aria-labelledby="crawler-history">
    <div class="crawler-heading">
      <h2 id="crawler-history">Crawl jobs</h2>
      <button type="button" :disabled="!native" @click="refresh">Refresh</button>
    </div>
    <p v-if="!jobs.length" class="crawler-notice">No crawl jobs yet.</p>
    <article v-for="job in jobs" :key="job.id" class="crawler-panel crawler-stack">
      <div class="crawler-heading">
        <div class="crawler-stack crawler-compact">
          <p class="crawler-url" :title="job.config.startUrls.join(', ')">{{ job.config.startUrls.join(", ") }}</p>
          <p class="crawler-meta">
            <span class="crawler-status" :data-status="job.status">{{ job.status }}</span>
            {{ job.pages_crawled }} pages · {{ job.assets_downloaded }} assets · {{ new Date(job.created_at).toLocaleString() }}
          </p>
        </div>
        <div class="crawler-actions">
          <button type="button" :aria-pressed="selected === job.id" :disabled="busy"
            @click="action('Could not load results', () => selectJob(selected === job.id ? undefined : job.id))">
            {{ selected === job.id ? "Hide results" : "Results" }}
          </button>
          <button v-if="crawlActive(job)" type="button" :disabled="busy || job.status === 'stopping'"
            @click="action('Could not stop crawl', async () => { await crawler.stop(job.id); await refresh(); })">
            {{ job.status === "stopping" ? "Stopping…" : "Stop" }}
          </button>
          <button type="button" :disabled="busy"
            @click="action('Could not export crawl', async () => { if (await crawler.export(job.id)) notify({ severity: 'success', title: 'Crawl exported' }); })">
            Export JSON
          </button>
          <button v-if="!crawlActive(job)" type="button" :disabled="busy" @click="confirmDelete(job)">Delete</button>
        </div>
      </div>
      <progress v-if="crawlActive(job)" :value="job.pages_crawled" :max="job.config.maxPages" aria-label="Pages crawled" />

      <div v-if="selected === job.id && results" class="crawler-stack">
        <section aria-label="Crawled pages" class="crawler-stack crawler-compact">
          <div class="crawler-heading">
            <h3>Pages</h3>
            <div class="crawler-actions">
              <button type="button" :disabled="busy || pageOffset === 0" @click="paginate('pages', -1)">Previous pages</button>
              <button type="button" :disabled="busy || pageOffset + pageSize >= results.pages_total" @click="paginate('pages', 1)">Next pages</button>
            </div>
          </div>
          <p v-if="!results.pages.length" class="crawler-meta">No pages collected.</p>
          <div v-else class="crawler-table">
            <table>
              <thead><tr><th scope="col">URL</th><th scope="col">Title</th><th scope="col">HTTP</th><th scope="col">Depth</th></tr></thead>
              <tbody>
                <tr v-for="page in results.pages" :key="page.url">
                  <td><a :href="page.url" @click.prevent="action('Could not open page', () => openUrl(page.url))">{{ page.url }}</a></td>
                  <td>{{ page.title || "Untitled" }}</td><td>{{ page.status_code }}</td><td>{{ page.depth }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>
        <section aria-label="Downloaded assets" class="crawler-stack crawler-compact">
          <div class="crawler-heading">
            <h3>Assets</h3>
            <div class="crawler-actions">
              <button type="button" :disabled="busy || assetOffset === 0" @click="paginate('assets', -1)">Previous assets</button>
              <button type="button" :disabled="busy || assetOffset + pageSize >= results.assets_total" @click="paginate('assets', 1)">Next assets</button>
            </div>
          </div>
          <p v-if="!results.assets.length" class="crawler-meta">No assets downloaded.</p>
          <div v-else class="crawler-table">
            <table>
              <thead><tr><th scope="col">URL</th><th scope="col">Type</th><th scope="col">Size</th></tr></thead>
              <tbody>
                <tr v-for="asset in results.assets" :key="asset.local_path">
                  <td :title="asset.local_path">{{ asset.source_url }}</td><td>{{ asset.mime_type }}</td><td>{{ bytes(asset.size_bytes) }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </article>
  </section>

  <dialog ref="deleteDialog" aria-labelledby="delete-crawl-title" @close="finishDelete">
    <form class="dialog-content" method="dialog">
      <h2 id="delete-crawl-title">Delete crawl results?</h2>
      <!-- Files in a user-selected destination belong to the user. -->
      <p v-if="pendingDelete?.config.assetFolder">This removes the job. Files in your chosen folder are kept.</p>
      <p v-else>This removes the job and its downloaded assets.</p>
      <div class="dialog-actions">
        <button value="cancel" autofocus>Cancel</button>
        <button value="delete" class="primary-button">Delete crawl</button>
      </div>
    </form>
  </dialog>
</template>
