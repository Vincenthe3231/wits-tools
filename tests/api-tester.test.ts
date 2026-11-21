/** Verifies API Tester validation and check results without making network calls. */
import { describe, expect, test } from "bun:test";

import type { ApiResponse } from "../src/api/api-tester";
import { evaluateChecks, prepareRequest, type RequestDraft } from "../src/utils/api-tester/request";

// Start from a minimal valid draft and a response with a deterministic timing value.
const draft: RequestDraft = {
  method: "GET",
  url: "http://localhost:3000/health",
  headers: [],
  body: "",
  expectedStatus: "",
  maxTime: "",
};
const response: ApiResponse = {
  status: 404,
  headers: [],
  timeToHeadersMs: 200,
  body: { kind: "empty", truncated: false },
};

describe("API Tester request validation", () => {
  // Keep repeated populated headers and skip empty or whitespace-only values.
  test("takes a snapshot and leaves out blank header rows", () => {
    const source = {
      ...draft,
      url: " http://localhost:3000/health?x=1&x=2 ",
      headers: [
        { id: 1, enabled: true, name: "", value: "" },
        { id: 2, enabled: false, name: "Authorization", value: "Bearer example" },
        { id: 3, enabled: true, name: "x-example", value: "two" },
        { id: 4, enabled: true, name: "Content-Type", value: "  " },
        { id: 5, enabled: true, name: "x-example", value: "three" },
      ],
      body: " raw text ",
      expectedStatus: "404",
      maxTime: "200",
    };
    const result = prepareRequest(source);
    source.headers[2].value = "changed";
    source.expectedStatus = "200";
    expect(result).toEqual({
      request: {
        method: "GET",
        url: "http://localhost:3000/health?x=1&x=2",
        headers: [["x-example", "two"], ["x-example", "three"]],
        body: " raw text ",
      },
      checks: { expectedStatus: 404, maxTime: 200 },
      headerIds: [3, 5],
    });
  });

  // Associate bad URLs, headers, and thresholds with the exact inputs that need correction.
  test("reports field errors for invalid input", () => {
    const result = prepareRequest({
      ...draft,
      url: "http:/localhost",
      headers: [
        { id: 4, enabled: true, name: "bad name", value: "line\nbreak" },
        { id: 9, enabled: true, name: "Content-Length", value: "2" },
      ],
      expectedStatus: "600",
      maxTime: "2.5",
    });
    expect("errors" in result && result.errors.map(({ id, tab }) => [id, tab])).toEqual([
      ["api-url", undefined],
      ["api-header-4-name", "headers"],
      ["api-header-4-value", "headers"],
      ["api-header-9-name", "headers"],
      ["api-expected-status", "checks"],
      ["api-max-time", "checks"],
    ]);
  });
});

describe("API Tester checks", () => {
  // Status failures can be expected and the timing comparison includes the threshold itself.
  test("reports expected and actual values and includes the time boundary", () => {
    expect(evaluateChecks({ expectedStatus: 404, maxTime: 200 }, response)).toEqual([
      { label: "Status equals", expected: "404", actual: "404", passed: true },
      { label: "Time to headers is at most", expected: "200 ms", actual: "200 ms", passed: true },
    ]);
    expect(evaluateChecks({ expectedStatus: 200, maxTime: 199 }, response).map(({ passed }) => passed))
      .toEqual([false, false]);
    expect(evaluateChecks({}, response)).toEqual([]);
  });
});
