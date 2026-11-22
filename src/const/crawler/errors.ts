/** Maps native error codes to descriptions that are safe to show in the workspace. */
const messages: Record<string, string> = {
  invalidConfig: "Check the limits, domain names, exclusion patterns, and whether the download path is a full folder path.",
  invalidUrl: "Enter a valid HTTP or HTTPS URL.",
  invalidProxy: "Enter a supported public DNS address, a DNS-over-HTTPS endpoint, or a valid proxy URL.",
  notFound: "This job or source is no longer available.",
  busy: "Wait for the current operation to finish and try again.",
  storage: "WITS Tools could not access crawler history.",
  browserUnavailable: "Obscura was not found. Check that the WITS Tools browser resources are installed.",
  browserFailed: "The Obscura crawl session ended unexpectedly. Try the crawl again.",
  browserNavigationFailed: "Obscura could not load the page. Check the page URL and proxy settings.",
  network: "Check your connection and proxy settings, then try again.",
  unsupportedMedia: "Enter a supported direct file, HLS playlist, or DASH manifest.",
  invalidManifest: "The playlist or manifest could not be read with these settings.",
  outputConflict: "Choose a different filename or allow replacement.",
  file: "Check that the destination exists and that you can write to it.",
  cancelled: "The operation was cancelled.",
  captureRouteChanged: "Close the capture browser and choose Capture again to use the new network settings.",
  captureUnavailable: "WITS Tools could not open the private WebView2 capture browser.",
};

// Native diagnostics can mention local URLs or credentials, so show only known descriptions.
export function crawlerError(cause: unknown) {
  const code = cause && typeof cause === "object" && "code" in cause ? cause.code : undefined;
  return typeof code === "string" && Object.prototype.hasOwnProperty.call(messages, code)
    ? messages[code]
    : "WITS Tools could not complete this action.";
}
