/** Defines the desktop boundary for crawler jobs and media capture. */
import { invoke, isTauri } from "@tauri-apps/api/core";

export type CrawlMode = "auto" | "always" | "never";
export type MediaKind = "direct" | "hls" | "dash";

export interface CrawlConfig {
  startUrls: string[];
  maxPages: number;
  maxDepth: number;
  mode: CrawlMode;
  concurrency: number;
  requestDelayMillis: number;
  downloadAssets: boolean;
  // A blank download path keeps assets in the application's managed storage.
  assetFolder: string | null;
  allowedDomains: string[];
  excludePatterns: string[];
  respectRobotsTxt: boolean;
  proxyUrl: string | null;
}

export interface CrawlJob {
  id: string;
  status: "running" | "stopping" | "stopped" | "failed" | "completed";
  config: Omit<CrawlConfig, "mode"> & { usePlaywright: CrawlMode };
  created_at: number;
  finished_at: number | null;
  pages_crawled: number;
  assets_downloaded: number;
}

export interface CrawlResults {
  pages: { url: string; title: string; status_code: number; depth: number }[];
  assets: { source_url: string; local_path: string; mime_type: string; size_bytes: number }[];
  pages_total: number;
  assets_total: number;
}

export interface MediaSource {
  id: string;
  url: string;
  kind: MediaKind;
  segments: number;
}

export interface DownloadRequest {
  url: string;
  folder: string | null;
  filename: string | null;
  format: string;
  proxy: string | null;
  overwrite: boolean;
  workers: number;
}

export interface DownloadJob {
  id: string;
  status: "queued" | "downloading" | "completed" | "cancelled" | "failed";
  progress: { bytes: number; totalBytes: number | null; segments: number; totalSegments: number | null };
  durationSeconds: number | null;
  fileSize: number | null;
  etaSeconds: number | null;
  output: string | null;
  error: { code: string } | null;
}

export interface CaptureSnapshot {
  status: "closed" | "loading" | "ready" | "failed";
  sources: MediaSource[];
}

// Keep all native names and argument shapes out of the view components.
export const crawler = {
  start: (config: CrawlConfig) => invoke<string>("crawler_start", { config }),
  jobs: () => invoke<CrawlJob[]>("crawler_jobs"),
  results: (id: string, pages: number, assets: number) => invoke<CrawlResults>("crawler_results", { id, pages, assets }),
  stop: (id: string) => invoke<void>("crawler_stop", { id }),
  remove: (id: string) => invoke<void>("crawler_delete", { id }),
  export: (id: string) => invoke<boolean>("crawler_export", { id }),
  pickFolder: () => invoke<string | null>("crawler_pick_folder"),
  download: (request: DownloadRequest, source: string | null) =>
    invoke<string>("crawler_download", { request, source }),
  mediaInfo: (request: DownloadRequest, source: string | null) =>
    invoke<{ formats: { id: string; bandwidth: number }[] }>("crawler_media_info", { request, source }),
  downloads: () => invoke<DownloadJob[]>("crawler_downloads"),
  cancelDownload: (id: string) => invoke<void>("crawler_cancel_download", { id }),
  // The embedded browser owns its own session and candidate list.
  openCaptureBrowser: (url: string, proxy: string | null) =>
    invoke<void>("crawler_open_capture_browser", { url, proxy }),
  captureBrowserSources: () => invoke<CaptureSnapshot>("crawler_capture_browser_sources"),
  shutdown: () => isTauri() ? invoke<void>("crawler_shutdown") : Promise.resolve(),
};
