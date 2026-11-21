/** Maps native error codes to descriptions that are safe to show in the workspace. */
const messages: Record<string, string> = {
  invalidConfig: "Check the limits, domain names, and exclusion patterns.",
  invalidUrl: "Enter a valid HTTP or HTTPS URL.",
  invalidProxy: "Enter a valid HTTP, HTTPS, or SOCKS5 proxy URL.",
  notFound: "This job or source is no longer available.",
  busy: "Wait for the current operation to finish and try again.",
  storage: "WITS Tools could not access crawler history.",
  browserUnavailable: "Obscura was not found. Check that the WITS Tools browser resources are installed.",
  browserFailed: "Obscura could not open the page. Check the URL and proxy, or try selected-tab capture for media.",
  network: "Check your connection and proxy settings, then try again.",
  unsupportedMedia: "Enter a supported direct file, HLS playlist, or DASH manifest.",
  invalidManifest: "The playlist or manifest could not be read with these settings.",
  outputConflict: "Choose a different filename or allow replacement.",
  file: "Check that the destination exists and that you can write to it.",
  cancelled: "The operation was cancelled.",
  captureUnavailable: "Enable selected-tab capture, or generate a manual pairing code.",
};

// Native diagnostics can mention local URLs or credentials, so show only known descriptions.
export function crawlerError(cause: unknown) {
  const code = cause && typeof cause === "object" && "code" in cause ? cause.code : undefined;
  return typeof code === "string" && Object.prototype.hasOwnProperty.call(messages, code)
    ? messages[code]
    : "WITS Tools could not complete this action.";
}
