/** Polls persisted crawl and media status without overlapping refreshes. */
import { isTauri } from "@tauri-apps/api/core";
import { onActivated, onDeactivated, onScopeDispose, ref, shallowRef } from "vue";

import { crawler } from "../../api/crawler";
import type { CrawlJob, CrawlResults, DownloadJob, MediaSource } from "../../api/crawler";
import { crawlerError } from "../../const/crawler/errors";
import { notify } from "../use-notifications";

export function useCrawlerJobs() {
  const jobs = shallowRef<CrawlJob[]>([]);
  const downloads = shallowRef<DownloadJob[]>([]);
  const captured = shallowRef<MediaSource[]>([]);
  // Embedded capture can remain open while the user visits other workspace views.
  const browserSources = shallowRef<MediaSource[]>([]);
  const results = shallowRef<CrawlResults>();
  const selected = ref<string>();
  const pageOffset = ref(0);
  const assetOffset = ref(0);
  const captureActive = ref(false);
  const proxy = ref("");
  const refreshError = ref("");
  const native = isTauri();
  let timer: ReturnType<typeof setInterval> | undefined;
  let currentRefresh: Promise<void> | undefined;
  let revision = 0;

  // Avoid applying a response after the user has switched jobs or changed page.
  async function refreshResults() {
    const ticket = ++revision;
    const id = selected.value;
    if (!id) return;
    const next = await crawler.results(id, pageOffset.value, assetOffset.value);
    if (ticket === revision && id === selected.value) results.value = next;
  }

  // Use a single refresh across the two views and announce each completed transfer once.
  function refresh() {
    if (!native) return Promise.resolve();
    if (currentRefresh) return currentRefresh;
    currentRefresh = (async () => {
      const [nextJobs, nextDownloads, nextCaptured, nextBrowserSources] = await Promise.all([
        crawler.jobs(), crawler.downloads(), captureActive.value ? crawler.captured() : Promise.resolve([]),
        crawler.captureBrowserSources(),
      ]);
      for (const job of nextJobs) {
        const previous = jobs.value.find((entry) => entry.id === job.id);
        if (previous && previous.status !== job.status && job.status === "completed") {
          notify({ severity: "success", title: "Crawl complete", id: job.id });
        }
        if (previous && previous.status !== job.status && job.status === "failed") {
          notify({ severity: "error", title: "Crawl failed", id: job.id });
        }
      }
      for (const job of nextDownloads) {
        const previous = downloads.value.find((entry) => entry.id === job.id);
        if (previous && previous.status !== job.status && job.status === "completed") {
          notify({ severity: "success", title: "Download complete", id: job.id });
        }
        if (previous && previous.status !== job.status && job.status === "failed") {
          notify({ severity: "error", title: "Download failed", description: crawlerError(job.error), id: job.id });
        }
      }
      jobs.value = nextJobs;
      downloads.value = nextDownloads;
      browserSources.value = nextBrowserSources;
      if (captureActive.value) captured.value = nextCaptured;
      if (selected.value && !nextJobs.some((job) => job.id === selected.value)) await selectJob(undefined);
      await refreshResults();
      refreshError.value = "";
    })().catch((error: unknown) => { refreshError.value = crawlerError(error); })
      .finally(() => { currentRefresh = undefined; });
    return currentRefresh;
  }

  async function selectJob(id: string | undefined) {
    selected.value = id;
    pageOffset.value = 0;
    assetOffset.value = 0;
    results.value = undefined;
    await refreshResults();
  }

  function stopPolling() {
    if (timer !== undefined) clearInterval(timer);
    timer = undefined;
  }

  // Poll only while the page is visible. Native jobs continue while it is cached.
  onActivated(() => {
    void refresh();
    if (native) timer = setInterval(() => void refresh(), 1200);
  });
  onDeactivated(stopPolling);
  onScopeDispose(stopPolling);

  return { native, jobs, downloads, captured, browserSources, results, selected, pageOffset, assetOffset,
    captureActive, proxy, refreshError, refresh, refreshResults, selectJob };
}
