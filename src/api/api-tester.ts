/** Defines the typed Tauri boundary for sending requests from API Tester. */
import { invoke } from "@tauri-apps/api/core";

import { errorCode } from "../utils/errors";

// Carry header pairs in their original multiplicity across IPC.
export type Header = [name: string, value: string];

export interface ApiRequest {
  method: string;
  url: string;
  headers: Header[];
  body: string | null;
}

type BodyContent = { kind: "empty" | "unsupported" } | { kind: "text"; text: string };

export interface ApiResponse {
  status: number;
  headers: Header[];
  timeToHeadersMs: number;
  body: BodyContent & { truncated: boolean };
}

// Accept stable error identifiers and a numeric header index from native code.
const nativeCodes = [
  "invalidMethod", "invalidUrl", "invalidHeaderName", "invalidHeaderValue",
  "managedHeader", "timeout", "connectionFailed", "responseFailed", "unexpected",
] as const;

export class ApiFailure extends Error {
  constructor(
    readonly code: (typeof nativeCodes)[number],
    readonly index?: number,
  ) {
    super(code);
  }
}

// Keep raw network and IPC diagnostics out of the presentation layer.
export async function sendApiRequest(request: ApiRequest): Promise<ApiResponse> {
  try {
    return await invoke<ApiResponse>("api_tester_send", { request });
  } catch (cause) {
    const index = cause && typeof cause === "object" && "index" in cause
      && typeof cause.index === "number" && Number.isSafeInteger(cause.index) && cause.index >= 0
      ? cause.index
      : undefined;
    throw new ApiFailure(errorCode(cause, nativeCodes), index);
  }
}
