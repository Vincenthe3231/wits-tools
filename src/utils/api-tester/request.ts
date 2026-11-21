/** Validates the request draft and evaluates checks against the submitted response. */
import type { ApiRequest, ApiResponse } from "../../api/api-tester";
import { apiErrors } from "../../const/api-tester/errors";

// Keep row identity separate from its position in the filtered IPC header list.
export interface HeaderRow {
  id: number;
  enabled: boolean;
  name: string;
  value: string;
}

export interface RequestDraft {
  method: string;
  url: string;
  headers: HeaderRow[];
  body: string;
  expectedStatus: string;
  maxTime: string;
}

export interface FieldError {
  id: string;
  tab?: "headers" | "checks";
  message: string;
}

interface CheckSettings {
  expectedStatus?: number;
  maxTime?: number;
}

interface PreparedRequest {
  request: ApiRequest;
  headerIds: number[];
  checks: CheckSettings;
}

// Route header errors to unique form fields even when earlier rows have been removed.
export function headerFieldId(id: number, field: "name" | "value"): string {
  return "api-header-" + id + "-" + field;
}

// Accept absolute web URLs and leave credentials to the explicit request headers.
function validUrl(value: string): boolean {
  if (!/^https?:\/\/[^/\\]/i.test(value)) {
    return false;
  }
  try {
    const url = new URL(value);
    return Boolean(url.hostname) && !url.username && !url.password;
  } catch {
    return false;
  }
}

// Empty check fields are optional. Reject partial numbers, decimals, and unsafe integers.
function checkNumber(value: string, min: number, max: number): number | undefined {
  const input = value.trim();
  const number = Number(input);
  return /^\d+$/.test(input) && Number.isSafeInteger(number) && number >= min && number <= max
    ? number
    : undefined;
}

// Produce one snapshot for the command and its checks, or field errors before any network call.
export function prepareRequest(draft: RequestDraft): PreparedRequest | { errors: FieldError[] } {
  const errors: FieldError[] = [];
  const url = draft.url.trim();
  if (!validUrl(url)) {
    errors.push({ id: "api-url", message: apiErrors.invalidUrl });
  }

  const headers: ApiRequest["headers"] = [];
  const headerIds: number[] = [];
  for (const row of draft.headers) {
    // Skip unchecked rows and blank values before validating or indexing sent headers.
    if (!row.enabled || !row.value.trim()) {
      continue;
    }
    const name = row.name.trim();
    if (!/^[!#$%&'*+.^_\x60|~0-9A-Za-z-]+$/.test(name)) {
      errors.push({ id: headerFieldId(row.id, "name"), tab: "headers", message: apiErrors.invalidHeaderName });
    } else if (/^(content-length|transfer-encoding)$/i.test(name)) {
      errors.push({ id: headerFieldId(row.id, "name"), tab: "headers", message: apiErrors.managedHeader });
    }
    if (/[\x00-\x08\x0a-\x1f\x7f]/.test(row.value)) {
      errors.push({ id: headerFieldId(row.id, "value"), tab: "headers", message: apiErrors.invalidHeaderValue });
    }
    headers.push([name, row.value]);
    headerIds.push(row.id);
  }

  const expectedStatus = checkNumber(draft.expectedStatus, 100, 599);
  if (draft.expectedStatus.trim() && expectedStatus === undefined) {
    errors.push({ id: "api-expected-status", tab: "checks", message: "Enter a status from 100 to 599." });
  }
  const maxTime = checkNumber(draft.maxTime, 0, Number.MAX_SAFE_INTEGER);
  if (draft.maxTime.trim() && maxTime === undefined) {
    errors.push({ id: "api-max-time", tab: "checks", message: "Enter a whole number of milliseconds, zero or greater." });
  }

  return errors.length
    ? { errors }
    : {
      request: { method: draft.method, url, headers, body: draft.body || null },
      headerIds,
      checks: { expectedStatus, maxTime },
    };
}

// Check the displayed values from the response against the settings captured at Send.
export function evaluateChecks(settings: CheckSettings, response: ApiResponse) {
  const results = [];
  if (settings.expectedStatus !== undefined) {
    results.push({
      label: "Status equals",
      expected: String(settings.expectedStatus),
      actual: String(response.status),
      passed: response.status === settings.expectedStatus,
    });
  }
  if (settings.maxTime !== undefined) {
    results.push({
      label: "Time to headers is at most",
      expected: settings.maxTime + " ms",
      actual: response.timeToHeadersMs + " ms",
      passed: response.timeToHeadersMs <= settings.maxTime,
    });
  }
  return results;
}
