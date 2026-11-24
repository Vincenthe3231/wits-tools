<!-- Presents direct downloads, private browser captures, and the native job history. -->
<script setup lang="ts">
import { isTauri } from "@tauri-apps/api/core";
import { nextTick, onActivated, onDeactivated, ref, shallowRef } from "vue";

import { crawler } from "../../api/crawler";
import { downloader, type DownloadJob, type DownloadSnapshot } from "../../api/downloader";
import AppSlider from "../../components/AppSlider.vue";
import WorkspaceIcon from "../../components/ui/WorkspaceIcon.vue";
import Badge from "../../components/ui/Badge.vue";
import Card from "../../components/ui/Card.vue";
import CardContent from "../../components/ui/CardContent.vue";
import CardHeader from "../../components/ui/CardHeader.vue";
import ToolFieldCard from "../../components/ui/ToolFieldCard.vue";
import { useActiveToolCard } from "../../composables/use-active-tool-card";
import { notify } from "../../composables/use-notifications";
import { downloadError } from "../../const/downloader/errors";
import { bytes, mediaTime } from "../../utils/crawler/display";
import "../../styles/crawler/workspace.css";

// Keep drafts in this tool session while native jobs continue across navigation.
defineOptions({ name: "DownloaderPage" });
const native = isTauri();
const workspace = ref<HTMLElement>();
useActiveToolCard(workspace);
const formInput = ref<HTMLInputElement>();
const historyTitle = ref<HTMLHeadingElement>();
const mode = ref<"direct" | "page">("direct");
const directUrl = ref("");
const pageUrl = ref("");
const filename = ref("");
const folder = ref("");
const workers = ref(4);
const workersValid = ref(true);
const busy = ref(false);
const refreshError = ref("");
const snapshot = shallowRef<DownloadSnapshot>({ jobs: [], candidate: null, captureOpen: false, historyError: false });
const directSources = new Map<string, string>();
let timer: ReturnType<typeof setInterval> | undefined;
let revision = 0;

// Ignore outdated polls that finish after a newer refresh or a view deactivation.
async function refresh() {
  const current = ++revision;
  try {
    const state = await downloader.status();
    if (current !== revision) return;
    snapshot.value = state;
    refreshError.value = "";
    for (const job of state.jobs) {
      if (job.status === "completed") directSources.delete(job.id);
    }
  } catch (error) {
    if (current === revision) refreshError.value = downloadError(error);
  }
}

onActivated(() => {
  if (!native) return;
  void refresh();
  timer = setInterval(() => void refresh(), 500);
});

onDeactivated(() => {
  ++revision;
  clearInterval(timer);
});

// Keep picker cancellation silent and report action errors through the shared host.
async function action(title: string, operation: () => Promise<unknown>) {
  if (busy.value) return;
  busy.value = true;
  try {
    await operation();
    await refresh();
  } catch (error) {
    notify({ severity: "error", title, description: downloadError(error) });
  } finally {
    busy.value = false;
  }
}

function options() {
  return { folder: folder.value.trim() || null, filename: filename.value.trim() || null, workers: workers.value };
}

function submit() {
  if (!workersValid.value) return;
  if (mode.value === "page") return action("Could not open download page", () => downloader.openCapture(pageUrl.value.trim()));
  return action("Could not queue download", async () => {
    const url = directUrl.value.trim();
    const id = await downloader.enqueue(url, options());
    directSources.set(id, url);
  });
}

function chooseFolder() {
  return action("Could not choose folder", async () => {
    const selected = await crawler.pickFolder();
    if (selected) folder.value = selected;
  });
}

// Use session links for direct retries. Older or captured jobs need a new source.
async function recover(job: DownloadJob) {
  if (job.error === "page" || job.captured) {
    mode.value = "page";
    if (job.error === "page") pageUrl.value = directSources.get(job.id) ?? "";
    if (pageUrl.value) {
      return action("Could not open download page", () => downloader.openCapture(pageUrl.value));
    }
  } else if (job.retryAvailable && workersValid.value) {
    return action("Could not retry download", async () => {
      const id = await downloader.retry(job.id, options());
      const url = directSources.get(job.id);
      if (url) directSources.set(id, url);
    });
  } else {
    mode.value = "direct";
    directUrl.value = "";
  }
  await nextTick();
  formInput.value?.focus();
}

const modes = { parallel: "Parallel", single: "Single stream", browser: "Browser" } as const;
// Explain the automatic fallback when the user selected Download.
const fallbackReasons = {
  singleWorker: "Parallel downloads need at least two workers.",
  unsupportedRanges: "The file was too small to split or the server did not pass the range check.",
  unverifiedRequest: "WITS Tools could not verify that this browser request can be replayed.",
};

function confirmCapture(id: string, useBrowser: boolean) {
  return action("Could not confirm download", async () => {
    const reason = await downloader.confirmCapture(id, options(), useBrowser);
    if (reason) notify({ severity: "info", title: "Saving with browser", description: fallbackReasons[reason] });
  });
}

// The byte count can reach the total before WebView2 finishes saving the file.
function finishing(job: DownloadJob) {
  return job.status === "downloading" && job.total !== null && job.total > 0 && job.bytes >= job.total;
}

// Return keyboard focus to the history when its remove button disappears.
function removeHistory(job: DownloadJob) {
  return action("Could not remove download record", async () => {
    await downloader.removeHistory(job.id);
    directSources.delete(job.id);
    historyTitle.value?.focus();
  });
}

const status = {
  queued: "Queued", downloading: "Downloading", completed: "Completed",
  failed: "Failed", cancelled: "Cancelled", interrupted: "Interrupted",
} as const;

function badge(job: DownloadJob) {
  if (job.status === "completed") return "success";
  if (job.status === "failed") return "error";
  if (job.status === "interrupted") return "warning";
  if (job.status === "downloading") return "info";
  return "neutral";
}
</script>

<template>
  <!-- Both source modes share settings while retaining their own URL drafts. -->
  <div ref="workspace" class="crawler-card-workspace crawler-workspace tool-workspace downloader-workspace">
    <p v-if="!native" class="crawler-notice">Open WITS Tools on the desktop to download files.</p>
    <p v-else-if="refreshError || snapshot.historyError" class="crawler-notice crawler-error" role="status">
      {{ refreshError || "WITS Tools could not save the download history." }}
    </p>
    <form autocomplete="off" @submit.prevent="submit">
      <Card>
        <CardHeader class="crawler-heading">
          <h1>Downloader</h1>
          <button
            class="primary-button"
            :disabled="!native || busy || !workersValid || !(mode === 'direct' ? directUrl.trim() : pageUrl.trim())"
          >
            {{ mode === "direct" ? "Download" : snapshot.captureOpen ? "Show capture browser" : "Open download page" }}
          </button>
        </CardHeader>

        <CardContent>
          <ToolFieldCard title="Source" description="Paste a file link or open the page that offers it.">
            <fieldset class="crawler-actions">
              <legend class="sr-only">URL type</legend>
              <label><input v-model="mode" type="radio" value="direct" /> Direct file URL</label>
              <label><input v-model="mode" type="radio" value="page" /> Download page</label>
            </fieldset>
            <label v-if="mode === 'direct'" class="crawler-field">
              <span>File URL</span>
              <input ref="formInput" v-model="directUrl" type="url" placeholder="https://example.com/file" required />
            </label>
            <label v-else class="crawler-field">
              <span>Page URL</span>
              <input ref="formInput" v-model="pageUrl" type="url" placeholder="https://example.com/download" required />
            </label>
          </ToolFieldCard>

          <!-- The destination stays editable while a captured download awaits confirmation. -->
          <details class="crawler-stack">
            <summary>Download settings</summary>
            <!-- Match the spacing of the Crawler download settings. -->
            <div class="crawler-advanced tool-field-controls">
              <ToolFieldCard title="Destination" description="Choose where to save the file.">
                <div class="crawler-field">
                  <label for="download-folder">Save to folder</label>
                  <div class="crawler-inline-control">
                    <input id="download-folder" v-model="folder" placeholder="Windows Downloads" />
                    <button type="button" :disabled="!native || busy" @click="chooseFolder">Choose folder</button>
                  </div>
                </div>
                <label class="crawler-field">
                  <span>Filename</span>
                  <input v-model="filename" placeholder="Use the suggested filename" />
                </label>
              </ToolFieldCard>
              <ToolFieldCard title="Acceleration" description="Set the maximum number of workers per file.">
                <AppSlider
                  id="download-workers"
                  v-model="workers"
                  v-model:valid="workersValid"
                  label="Download workers"
                  :min="1"
                  :max="16"
                  help="More workers can help when the server supports parallel file ranges."
                />
              </ToolFieldCard>
            </div>
          </details>
        </CardContent>
      </Card>
    </form>

    <!-- Announce capture state without announcing every progress update. -->
    <p v-if="snapshot.captureOpen && !snapshot.candidate" class="crawler-notice" role="status">
      Use the site’s download control in the capture browser. Keep the window open for browser transfers.
    </p>
    <Card v-if="snapshot.candidate">
      <CardHeader>
        <h2 role="status">Download detected</h2>
      </CardHeader>
      <CardContent>
        <div v-for="candidate in [snapshot.candidate]" :key="candidate.id" class="crawler-stack">
          <strong class="break-all">{{ candidate.name }}</strong>
          <span>{{ candidate.origin }}{{ candidate.size === null ? "" : " · " + bytes(candidate.size) }}</span>
          <div class="crawler-actions">
            <button
              class="primary-button"
              type="button"
              :disabled="busy || !workersValid"
              @click="confirmCapture(candidate.id, false)"
            >Download</button>
            <button
              type="button"
              :disabled="busy || !workersValid"
              @click="confirmCapture(candidate.id, true)"
            >Save with browser</button>
            <button
              type="button"
              :disabled="busy"
              @click="action('Could not dismiss download', () => downloader.cancel(candidate.id))"
            >Dismiss</button>
          </div>
        </div>
      </CardContent>
    </Card>

    <!-- Keep status and recovery visible at the job that owns them. -->
    <Card>
      <CardHeader><h2 ref="historyTitle" tabindex="-1">Downloads</h2></CardHeader>
      <CardContent>
        <p v-if="!snapshot.jobs.length">No downloads yet.</p>
        <ul v-else class="tool-card-children">
          <Card v-for="job in snapshot.jobs" :key="job.id" as="li" variant="child" class="tool-card-compact">
            <div class="crawler-actions">
              <strong class="break-all">{{ job.name }}</strong>
              <Badge :variant="badge(job)">{{ finishing(job) ? "Finishing" : status[job.status] }}</Badge>
              <Badge v-if="job.mode">{{ modes[job.mode] }}</Badge>
            </div>
            <p class="break-all">{{ job.origin }}</p>
            <!-- Show activity when WebView2 has bytes to report but no file size. -->
            <progress
              v-if="job.status === 'downloading' && job.total"
              class="download-progress"
              :aria-label="'Progress for ' + job.name"
              :value="job.bytes"
              :max="job.total"
            />
            <p v-else-if="job.status === 'downloading'" class="text-muted">
              Downloading · Total size unknown
            </p>
            <p v-if="job.mode">
              {{ bytes(job.bytes) }}{{ job.total === null ? "" : " / " + bytes(job.total) }}
              {{ job.speed === null ? "" : " · " + bytes(job.speed) + "/s" }}
              {{ job.eta === null ? "" : " · " + mediaTime(job.eta) + " remaining" }}
            </p>
            <p v-if="job.error && job.status === 'failed'" class="text-status-error-text">
              {{ downloadError(job.error) }}
            </p>
            <div class="crawler-actions">
              <button
                v-if="job.status === 'queued' || job.status === 'downloading'"
                type="button"
                :disabled="busy"
                @click="action('Could not cancel download', () => downloader.cancel(job.id))"
              >Cancel</button>
              <button
                v-else-if="job.status === 'completed'"
                type="button"
                :disabled="busy"
                @click="action('Could not reveal download', () => downloader.reveal(job.id))"
              >Reveal in folder</button>
              <button
                v-else
                type="button"
                :disabled="busy || (job.retryAvailable && job.error !== 'page' && !workersValid)"
                @click="recover(job)"
              >{{ job.error === "page" ? "Open download page" : job.captured ? "Capture again" : job.retryAvailable ? "Retry" : "Enter URL again" }}</button>
              <button
                v-if="job.status !== 'queued' && job.status !== 'downloading'"
                class="ml-auto inline-grid size-9 place-items-center rounded-full text-status-error-text hover:bg-status-error-surface"
                type="button"
                :disabled="busy"
                :aria-label="'Remove ' + job.name + ' from download history'"
                title="Remove from history"
                @click="removeHistory(job)"
              >
                <WorkspaceIcon name="trash" class="size-4" />
              </button>
            </div>
          </Card>
        </ul>
      </CardContent>
    </Card>
  </div>
</template>

<style scoped>
/* Match the gap between Crawler's outer cards, including when capture is visible. */
.downloader-workspace {
  display: grid;
  align-content: start;
  gap: 1rem;
}

.download-progress {
  width: 100%;
  accent-color: var(--accent-strong);
}
</style>
