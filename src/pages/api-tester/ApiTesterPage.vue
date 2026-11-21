<!-- Keeps the API Tester draft and latest result for the lifetime of the desktop window. -->
<script setup lang="ts">
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from "reka-ui";
import { computed, nextTick, reactive, ref, shallowRef } from "vue";

import { ApiFailure, sendApiRequest, type ApiResponse } from "../../api/api-tester";
import { notify } from "../../composables/use-notifications";
import { apiErrors } from "../../const/api-tester/errors";
import {
  evaluateChecks, headerFieldId, prepareRequest, type FieldError, type RequestDraft,
} from "../../utils/api-tester/request";
import { formatResponseJson, parseResponseJson, responseRows } from "../../utils/api-tester/response-table";

// Keep this page cached by the workspace while allowing other routes to unmount.
defineOptions({ name: "ApiTesterPage" });
const methods = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
// Offer common payload and authentication headers with no preset values.
const defaultHeaders = ["Content-Type", "Authorization", "X-API-Key"];
const draft = reactive<RequestDraft>({
  method: "GET",
  url: "",
  headers: defaultHeaders.map((name, index) => ({ id: index + 1, enabled: true, name, value: "" })),
  body: "",
  expectedStatus: "",
  maxTime: "",
});
const form = ref<HTMLFormElement>();
const requestTab = ref("headers");
const responseTab = ref("body");
const bodyView = ref<"raw" | "table">("raw");
const expandedRows = reactive(new Set(["root"]));
const errors = ref<Record<string, string>>({});
const sending = ref(false);
const failure = ref("");
const result = shallowRef<{
  method: string;
  url: string;
  response: ApiResponse;
  checks: ReturnType<typeof evaluateChecks>;
}>();

// Parse only complete text previews, and recompute rows when a branch opens or closes.
const responseJson = computed(() => {
  const body = result.value?.response.body;
  return body?.kind === "text" && !body.truncated ? parseResponseJson(body.text) : undefined;
});
// Leave plain text and incomplete previews as received.
const bodyText = computed(() => {
  const body = result.value?.response.body;
  if (body?.kind !== "text") {
    return "";
  }
  return responseJson.value ? formatResponseJson(body.text) : body.text;
});

// Copy only on demand and report the clipboard outcome through the shared toast host.
async function copyResponse(text: string, kind: "Body" | "Headers" | "Body preview") {
  try {
    await navigator.clipboard.writeText(text);
    notify({ severity: "success", title: kind + " copied" });
  } catch {
    notify({ severity: "error", title: "Couldn’t copy " + kind.toLowerCase() });
  }
}
const tableRows = computed(() => responseJson.value
  ? responseRows(responseJson.value.value, expandedRows)
  : []);

// Keep each branch independently expandable with a keyboard-accessible button.
function toggleRow(id: string) {
  if (!expandedRows.delete(id)) {
    expandedRows.add(id);
  }
}

// Format valid drafts after editing so typing and pasting keep their normal cursor behavior.
function formatRequestBody() {
  if (parseResponseJson(draft.body)) {
    draft.body = formatResponseJson(draft.body);
  }
}

// Give each header row a stable input ID for its labels and errors.
let nextHeaderId = defaultHeaders.length + 1;
async function addHeader() {
  const id = nextHeaderId++;
  draft.headers.push({ id, enabled: true, name: "", value: "" });
  await nextTick();
  form.value?.querySelector<HTMLInputElement>("#" + headerFieldId(id, "name"))?.focus();
}

function removeHeader(index: number) {
  const [row] = draft.headers.splice(index, 1);
  delete errors.value[headerFieldId(row.id, "name")];
  delete errors.value[headerFieldId(row.id, "value")];
}

// Associate errors with inputs and reveal the first invalid field before focusing it.
function fieldState(id: string) {
  const invalid = Boolean(errors.value[id]);
  return {
    "aria-invalid": invalid || undefined,
    "aria-describedby": invalid ? id + "-error" : undefined,
  };
}

async function showFieldErrors(issues: FieldError[]) {
  errors.value = Object.fromEntries(issues.map(({ id, message }) => [id, message]));
  const first = issues[0];
  if (first.tab) {
    requestTab.value = first.tab;
  }
  await nextTick();
  form.value?.querySelector<HTMLElement>("#" + first.id)?.focus();
}

// Translate native validation errors back to the submitted fields after blank rows were removed.
function nativeFieldError(error: ApiFailure, headerIds: number[]): FieldError | undefined {
  const { code, index } = error;
  if (code === "invalidUrl" || code === "invalidMethod") {
    return { id: code === "invalidUrl" ? "api-url" : "api-method", message: apiErrors[code] };
  }
  const rowId = index === undefined ? undefined : headerIds[index];
  if (rowId !== undefined && (
    code === "invalidHeaderName" || code === "invalidHeaderValue" || code === "managedHeader"
  )) {
    return {
      id: headerFieldId(rowId, code === "invalidHeaderValue" ? "value" : "name"),
      tab: "headers",
      message: apiErrors[code],
    };
  }
  return undefined;
}

// Keep the request and its checks tied to a single Send even if the draft is edited while waiting.
async function send() {
  if (sending.value) {
    return;
  }
  const prepared = prepareRequest(draft);
  if ("errors" in prepared) {
    await showFieldErrors(prepared.errors);
    return;
  }

  const { request, checks, headerIds } = prepared;
  errors.value = {};
  failure.value = "";
  result.value = undefined;
  responseTab.value = "body";
  expandedRows.clear();
  expandedRows.add("root");
  sending.value = true;
  try {
    const response = await sendApiRequest(request);
    result.value = {
      method: request.method,
      url: request.url,
      response,
      checks: evaluateChecks(checks, response),
    };
  } catch (cause) {
    const error = cause instanceof ApiFailure ? cause : new ApiFailure("unexpected");
    const issue = nativeFieldError(error, headerIds);
    if (issue) {
      await showFieldErrors([issue]);
    } else {
      failure.value = apiErrors[error.code];
      notify({ severity: "error", title: "Couldn’t complete the request", description: failure.value });
    }
  } finally {
    sending.value = false;
  }
}
</script>

<template>
  <main class="api-tester">
    <!-- Keep the endpoint and Send visible above the optional request details. -->
    <form ref="form" class="api-section" aria-labelledby="api-request-title" @submit.prevent="send">
      <h1 id="api-request-title">Request</h1>
      <div class="request-bar">
        <div class="field">
          <label for="api-method">Method</label>
          <select id="api-method" v-model="draft.method" v-bind="fieldState('api-method')" @change="delete errors['api-method']">
            <option v-for="method in methods" :key="method">{{ method }}</option>
          </select>
          <p v-if="errors['api-method']" id="api-method-error" class="text-status-error-text">
            {{ errors["api-method"] }}
          </p>
        </div>
        <div class="field">
          <label for="api-url">URL</label>
          <input
            id="api-url"
            v-model="draft.url"
            class="code"
            inputmode="url"
            autocomplete="off"
            spellcheck="false"
            placeholder="http://localhost:3000/health"
            v-bind="fieldState('api-url')"
            @input="delete errors['api-url']"
          />
          <p v-if="errors['api-url']" id="api-url-error" class="text-status-error-text">
            {{ errors["api-url"] }}
          </p>
        </div>
        <button type="submit" class="primary-button" :disabled="sending">
          {{ sending ? "Sending…" : "Send" }}
        </button>
      </div>

      <!-- Use the installed tabs to keep headers, raw body, and optional checks easy to reach. -->
      <TabsRoot v-model="requestTab">
        <TabsList class="tabs" aria-label="Request options">
          <TabsTrigger value="headers" type="button">Headers</TabsTrigger>
          <TabsTrigger value="body" type="button">Body</TabsTrigger>
          <TabsTrigger value="checks" type="button">Checks</TabsTrigger>
        </TabsList>
        <TabsContent value="headers" class="tab-panel">
          <div v-for="(row, index) in draft.headers" :key="row.id" class="header-row">
            <label class="header-inclusion">
              <input
                v-model="row.enabled"
                type="checkbox"
                :aria-label="'Include header ' + (row.name.trim() || (index + 1))"
                @change="delete errors[headerFieldId(row.id, 'name')]; delete errors[headerFieldId(row.id, 'value')]"
              />
              <span>Send</span>
            </label>
            <div class="field">
              <label :for="headerFieldId(row.id, 'name')">Name</label>
              <input
                :id="headerFieldId(row.id, 'name')"
                v-model="row.name"
                class="code"
                autocomplete="off"
                spellcheck="false"
                v-bind="fieldState(headerFieldId(row.id, 'name'))"
                @input="delete errors[headerFieldId(row.id, 'name')]"
              />
              <p
                v-if="errors[headerFieldId(row.id, 'name')]"
                :id="headerFieldId(row.id, 'name') + '-error'"
                class="text-status-error-text"
              >
                {{ errors[headerFieldId(row.id, "name")] }}
              </p>
            </div>
            <div class="field">
              <label :for="headerFieldId(row.id, 'value')">Value</label>
              <input
                :id="headerFieldId(row.id, 'value')"
                v-model="row.value"
                class="code"
                autocomplete="off"
                spellcheck="false"
                v-bind="fieldState(headerFieldId(row.id, 'value'))"
                @input="delete errors[headerFieldId(row.id, 'value')]"
              />
              <p
                v-if="errors[headerFieldId(row.id, 'value')]"
                :id="headerFieldId(row.id, 'value') + '-error'"
                class="text-status-error-text"
              >
                {{ errors[headerFieldId(row.id, "value")] }}
              </p>
            </div>
            <button
              type="button"
              :aria-label="'Remove header ' + (index + 1)"
              @click="removeHeader(index)"
            >
              Remove
            </button>
          </div>
          <button type="button" class="secondary-action" @click="addHeader">Add header</button>
          <p class="muted">Only checked headers with a value are sent.</p>
        </TabsContent>
        <TabsContent value="body" class="tab-panel">
          <div class="field">
            <label for="api-body">Raw request body</label>
            <textarea
              id="api-body"
              v-model="draft.body"
              class="body-textarea code"
              rows="6"
              spellcheck="false"
              @blur="formatRequestBody"
            />
            <p class="muted">Paste the payload here. For JSON, set Content-Type to application/json in Headers.</p>
          </div>
        </TabsContent>
        <TabsContent value="checks" class="tab-panel">
          <div class="check-fields">
            <div class="field">
              <label for="api-expected-status">Status equals</label>
              <input
                id="api-expected-status"
                v-model="draft.expectedStatus"
                inputmode="numeric"
                placeholder="200"
                v-bind="fieldState('api-expected-status')"
                @input="delete errors['api-expected-status']"
              />
              <p
                v-if="errors['api-expected-status']"
                id="api-expected-status-error"
                class="text-status-error-text"
              >
                {{ errors["api-expected-status"] }}
              </p>
            </div>
            <div class="field">
              <label for="api-max-time">Time to headers is at most (ms)</label>
              <input
                id="api-max-time"
                v-model="draft.maxTime"
                inputmode="numeric"
                placeholder="500"
                v-bind="fieldState('api-max-time')"
                @input="delete errors['api-max-time']"
              />
              <p v-if="errors['api-max-time']" id="api-max-time-error" class="text-status-error-text">
                {{ errors["api-max-time"] }}
              </p>
            </div>
          </div>
          <p class="muted">Leave a field blank to skip its check.</p>
        </TabsContent>
      </TabsRoot>
    </form>

    <!-- Expose the lifecycle once to assistive technology and keep transport errors visible. -->
    <section class="api-section" aria-labelledby="api-response-title">
      <h2 id="api-response-title">Response</h2>
      <p v-if="sending" role="status" class="muted">Waiting for the response…</p>
      <p v-else-if="failure" class="text-status-error-text">{{ failure }}</p>
      <p v-else-if="!result" class="muted">Send a request to see its response here.</p>
      <template v-else>
        <p class="response-summary" role="status">
          <strong>HTTP {{ result.response.status }}</strong>
          <span>{{ result.response.timeToHeadersMs }} ms to headers</span>
        </p>
        <p class="response-target code muted">{{ result.method }} {{ result.url }}</p>
        <TabsRoot v-model="responseTab">
          <TabsList class="tabs" aria-label="Response details">
            <TabsTrigger value="body" type="button">Body</TabsTrigger>
            <TabsTrigger value="headers" type="button">Headers</TabsTrigger>
            <TabsTrigger v-if="result.checks.length" value="checks" type="button">
              Check results ({{ result.checks.filter((check) => check.passed).length }}/{{ result.checks.length }})
            </TabsTrigger>
          </TabsList>
          <TabsContent value="body" class="tab-panel">
            <!-- Offer an indented text view and an expandable table for complete JSON. -->
            <div v-if="result.response.body.kind === 'text'" class="response-body-actions">
              <div role="group" aria-label="Response body view">
                <button type="button" :aria-pressed="bodyView === 'raw'" @click="bodyView = 'raw'">Raw</button>
                <button type="button" :aria-pressed="bodyView === 'table'" @click="bodyView = 'table'">Table</button>
              </div>
              <button
                type="button"
                class="secondary-action"
                @click="copyResponse(bodyText, result.response.body.truncated ? 'Body preview' : 'Body')"
              >
                {{ result.response.body.truncated ? "Copy preview" : "Copy body" }}
              </button>
            </div>
            <p v-if="result.response.body.truncated" class="text-status-warning-text">
              The response body exceeds the 1 MiB preview limit.
            </p>
            <textarea
              v-if="result.response.body.kind === 'text' && bodyView === 'raw'"
              class="body-textarea response-body code"
              aria-label="Response body text"
              :value="bodyText"
              rows="6"
              readonly
              spellcheck="false"
            />
            <table v-else-if="result.response.body.kind === 'text' && responseJson" class="response-headers">
              <thead>
                <tr>
                  <th scope="col">Key or index</th>
                  <th scope="col">Value</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in tableRows" :key="row.id" class="code">
                  <th scope="row">
                    <div :style="{ paddingInlineStart: 'min(50%, ' + row.depth * 0.75 + 'rem)' }">
                      <button
                        v-if="row.expandable"
                        type="button"
                        class="response-branch"
                        :aria-expanded="expandedRows.has(row.id)"
                        @click="toggleRow(row.id)"
                      >
                        <span aria-hidden="true">{{ expandedRows.has(row.id) ? "▾" : "▸" }}</span>
                        {{ row.name || '""' }}
                      </button>
                      <span v-else>{{ row.name || '""' }}</span>
                    </div>
                  </th>
                  <td>{{ row.value }}</td>
                </tr>
              </tbody>
            </table>
            <p v-else-if="result.response.body.kind === 'text'" class="muted">
              Table view requires a complete JSON response. Use Raw to inspect this body.
            </p>
            <p v-else-if="result.response.body.kind === 'empty'" class="muted">The response body is empty.</p>
            <p v-else class="muted">The body format cannot be displayed as UTF-8 text.</p>
          </TabsContent>
          <TabsContent value="headers" class="tab-panel">
            <p v-if="!result.response.headers.length" class="muted">No response headers.</p>
            <button
              v-if="result.response.headers.length"
              type="button"
              class="secondary-action"
              @click="copyResponse(result.response.headers.map(([name, value]) => name + ': ' + value).join('\n'), 'Headers')"
            >
              Copy headers
            </button>
            <table v-if="result.response.headers.length" class="response-headers">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Value</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="([name, value], index) in result.response.headers" :key="index" class="code">
                  <th scope="row">{{ name }}</th>
                  <td>{{ value }}</td>
                </tr>
              </tbody>
            </table>
          </TabsContent>
          <TabsContent v-if="result.checks.length" value="checks" class="tab-panel">
            <ul class="check-results">
              <li v-for="check in result.checks" :key="check.label">
                <strong :class="check.passed ? 'text-status-success-text' : 'text-status-error-text'">
                  {{ check.passed ? "Pass" : "Fail" }}
                </strong>
                {{ check.label }} {{ check.expected }}
                <span class="muted">(actual {{ check.actual }})</span>
              </li>
            </ul>
          </TabsContent>
        </TabsRoot>
      </template>
    </section>
  </main>
</template>

<style scoped>
/* Fit both sections into the workspace's scrollable column at the default desktop width. */
.api-tester {
  display: grid;
  gap: 1rem;
  max-width: 64rem;
  margin-inline: auto;
  padding: clamp(0.75rem, 3vw, 1.75rem);
}

.api-section {
  display: grid;
  min-width: 0;
  gap: 0.75rem;
  padding: clamp(0.75rem, 2vw, 1.25rem);
  border: 1px solid var(--workspace-edge);
  border-radius: 0.65rem;
  background: var(--surface-editor);
}

h1, h2 {
  font-size: 1.125rem;
  font-weight: 650;
}

/* Let the endpoint and header value fields shrink while preserving intrinsic button sizes. */
.request-bar, .header-row, .check-fields {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: start;
  gap: 0.65rem;
}

.header-row {
  grid-template-columns: auto minmax(0, 1fr) minmax(0, 1.4fr) auto;
}

/* Keep the native checkbox aligned with the header fields. */
.header-inclusion {
  display: flex;
  align-items: center;
  gap: 0.3rem;
  min-height: 2rem;
  margin-top: 1.5rem;
  font-size: 0.875rem;
}

.header-inclusion input {
  width: 1rem;
  min-height: 1rem;
  accent-color: var(--accent-strong);
}

.check-fields {
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 11rem), 1fr));
}

.field {
  display: grid;
  min-width: 0;
  gap: 0.25rem;
  font-size: 0.875rem;
}

.request-bar > button, .header-row > button {
  margin-top: 1.5rem;
}

/* Share the workspace palette across native fields, tab states, and read-only previews. */
input, select, textarea {
  width: 100%;
  min-width: 0;
  min-height: 2rem;
  padding: 0.25rem 0.5rem;
  border: 1px solid var(--text-muted);
  border-radius: 0.35rem;
  background: var(--surface-editor);
  color: var(--text-primary);
}

:is(select, textarea, [role="tabpanel"]):focus-visible {
  outline: 2px solid var(--accent-strong);
  outline-offset: 2px;
}

:is(input, select)[aria-invalid="true"] {
  border-color: var(--status-error-indicator);
}

/* Use the panel width for wrapped lines and let the workspace handle vertical scrolling. */
.body-textarea {
  min-height: 9rem;
  max-width: 100%;
  resize: vertical;
}

@supports (field-sizing: content) {
  .body-textarea {
    field-sizing: content;
    overflow: hidden;
    resize: none;
  }
}

.code {
  font-family: var(--font-code);
  font-size: 0.875rem;
}

.muted {
  color: var(--text-muted);
  font-size: 0.875rem;
}

.tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem;
  padding: 0.25rem;
  border-radius: 0.35rem;
  background: var(--surface-toolbar);
}

.tabs [aria-selected="true"] {
  border-bottom: 2px solid var(--accent-strong);
  background: var(--surface-selected);
  color: var(--text-primary);
}

/* Preserve Reka's hidden state while laying out the active panel. */
.tab-panel:not([hidden]) {
  display: grid;
  gap: 0.5rem;
  padding-top: 0.75rem;
}

.secondary-action {
  justify-self: start;
  border-color: var(--text-muted);
}

/* Wrap response values and keep arbitrary server text inside the result section. */
.response-summary {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0.5rem 1rem;
}

.response-target, .response-headers {
  overflow-wrap: anywhere;
}

.response-body {
  background: var(--surface-toolbar);
}

/* Keep copy beside the view selector and allow it to wrap at narrow widths. */
.response-body-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 0.5rem;
}

.response-headers {
  width: 100%;
  table-layout: fixed;
  text-align: start;
}

.response-headers th, .response-headers td {
  padding: 0.375rem;
  border-bottom: 1px solid var(--workspace-edge);
  text-align: start;
  vertical-align: top;
}

.response-headers th:first-child {
  width: 37%;
}

/* Keep expandable keys within the same column as ordinary keys. */
.response-branch {
  max-width: 100%;
  text-align: start;
  overflow-wrap: anywhere;
  font: inherit;
}

.check-results strong {
  display: inline-block;
  min-width: 2.75rem;
}
</style>
