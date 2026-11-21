/** Supplies field and transport messages for the API Tester request form. */
import type { ApiFailure } from "../../api/api-tester";

// Share validation copy between local input validation and typed native rejections.
export const apiErrors = {
  invalidMethod: "Choose an HTTP method.",
  invalidUrl: "Enter a full HTTP or HTTPS URL. Use the Headers tab for credentials.",
  invalidHeaderName: "Enter a valid HTTP header name.",
  invalidHeaderValue: "Remove control characters from the header value.",
  managedHeader: "The HTTP client sets this header automatically.",
  timeout: "The request exceeded the 30-second limit.",
  connectionFailed: "Check the address, server availability, and certificate if you’re using HTTPS.",
  responseFailed: "The connection ended or the response could not be read.",
  unexpected: "Try sending the request again.",
} satisfies Record<ApiFailure["code"], string>;
