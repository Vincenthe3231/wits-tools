/** Formats crawler lists and transfer progress for the desktop workspace. */
import type { CrawlJob, DownloadJob } from "../../api/crawler";

// Parse separate entries without stripping regular expression punctuation.
export function items(text: string) {
  return text.split(/\r?\n/).map((value) => value.trim()).filter(Boolean);
}

export function bytes(count: number) {
  if (count < 1024) return `${count} B`;
  const unit = Math.min(3, Math.floor(Math.log(count) / Math.log(1024)));
  return `${(count / 1024 ** unit).toFixed(1)} ${["B", "KiB", "MiB", "GiB"][unit]}`;
}

export function crawlActive(job: CrawlJob) {
  return job.status === "running" || job.status === "stopping";
}

export function downloadActive(job: DownloadJob) {
  return job.status === "queued" || job.status === "downloading";
}

// Prefer file bytes for direct transfers and completed segments for manifests.
export function downloadPercent({ progress }: DownloadJob) {
  if (progress.totalBytes) return Math.min(100, 100 * progress.bytes / progress.totalBytes);
  if (progress.totalSegments) return Math.min(100, 100 * progress.segments / progress.totalSegments);
  return undefined;
}
