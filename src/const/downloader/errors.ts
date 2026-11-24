/** Maps native download errors to messages with the relevant recovery action. */

// Keep low-level network or filesystem diagnostics out of the download history.
const messages: Record<string, string> = {
  invalidUrl: "Enter an HTTP or HTTPS URL.",
  invalidWorkers: "Enter a worker count from 1 to 16.",
  accessDenied: "The site denied access. Get a fresh link or sign in on the download page.",
  rateLimited: "The site is limiting requests. Wait before trying again.",
  page: "This link opened a page. Use Download page to find the file.",
  network: "The transfer was interrupted. Check your connection and try again.",
  encoding: "The server returned an unsupported file encoding.",
  rangeChanged: "The file changed during transfer. Try again with a fresh link.",
  conflict: "That filename is taken. Change the filename or choose another folder.",
  file: "Check that the destination exists and is writable.",
  cancelled: "The download was cancelled.",
  capture: "Open the download page again and click its download control.",
  busy: "Wait for the current action to finish.",
  missingSource: "Enter the file URL again or capture a fresh link.",
  storage: "WITS Tools could not read or save the download history.",
};

export function downloadError(error: unknown) {
  return typeof error === "string"
    ? messages[error] ?? "The download action could not be completed."
    : "The download action could not be completed.";
}
