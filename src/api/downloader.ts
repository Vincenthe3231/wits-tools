/** Defines the public Downloader IPC contract. Replay credentials stay in Rust. */
import { invoke } from "@tauri-apps/api/core";

// Keep actions and snapshots typed at the native boundary.
export type DownloadOptions = { folder: string | null; filename: string | null; workers: number };
export type DownloadStatus = "queued" | "downloading" | "completed" | "failed" | "cancelled" | "interrupted";
export type DownloadMode = "single" | "parallel" | "browser";
export type DownloadJob = {
  id: string;
  origin: string;
  name: string;
  captured: boolean;
  status: DownloadStatus;
  mode: DownloadMode | null;
  bytes: number;
  total: number | null;
  speed: number | null;
  eta: number | null;
  path: string | null;
  error: string | null;
  retryAvailable: boolean;
};

export type DownloadSnapshot = {
  jobs: DownloadJob[];
  candidate: { id: string; name: string; origin: string; size: number | null } | null;
  captureOpen: boolean;
  historyError: boolean;
};

// Capture commands pass an opaque ID back to the window that owns the download.
export const downloader = {
  status: () => invoke<DownloadSnapshot>("downloader_status"),
  enqueue: (url: string, options: DownloadOptions) =>
    invoke<string>("downloader_enqueue", { url, options }),
  retry: (id: string, options: DownloadOptions) =>
    invoke<string>("downloader_retry", { id, options }),
  cancel: (id: string) => invoke<void>("downloader_cancel", { id }),
  reveal: (id: string) => invoke<void>("downloader_reveal", { id }),
  removeHistory: (id: string) => invoke<void>("downloader_remove_history", { id }),
  openCapture: (url: string) => invoke<void>("downloader_open_capture", { url }),
  confirmCapture: (id: string, options: DownloadOptions, useBrowser: boolean) =>
    invoke<"singleWorker" | "unsupportedRanges" | "unverifiedRequest" | null>(
      "downloader_confirm_capture", { id, options, useBrowser },
    ),
  shutdown: () => invoke<void>("downloader_shutdown"),
};
