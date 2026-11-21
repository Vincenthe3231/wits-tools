/** Checks JSON parsing and the visible rows of expanded response branches. */
import { expect, test } from "bun:test";

import { formatResponseJson, parseResponseJson, responseRows } from "../src/utils/api-tester/response-table";

// Formatting preserves literals, including numbers that JavaScript cannot represent exactly.
test("indents JSON without rewriting its values or duplicate keys", () => {
  const text = `{"id":9007199254740993,"id":1e+5,"data":[{"text":" a,\\n b "},[ ],{ }]}`;
  expect(formatResponseJson(text)).toBe([
    "{",
    '  "id": 9007199254740993,',
    '  "id": 1e+5,',
    '  "data": [',
    "    {",
    '      "text": " a,\\n b "',
    "    },",
    "    [],",
    "    {}",
    "  ]",
    "}",
  ].join("\n"));
});

// Opening and closing branches changes descendant visibility while preserving their depth.
test("expands and collapses nested objects and arrays", () => {
  const payload = { users: [{ active: true }], count: 1 };
  const expanded = new Set(["root"]);
  expect(responseRows(payload, expanded).map((row) => row.name)).toEqual(["Root", "users", "count"]);
  expanded.add("root.0");
  expanded.add("root.0.0");
  expect(responseRows(payload, expanded).find((row) => row.name === "active"))
    .toMatchObject({ depth: 3, value: "true", expandable: false });
  expanded.delete("root.0");
  expect(responseRows(payload, expanded).map((row) => row.name)).toEqual(["Root", "users", "count"]);
});

// A valid scalar response also has a table representation.
test("distinguishes JSON null from invalid JSON", () => {
  expect(parseResponseJson("{")).toBeUndefined();
  expect(parseResponseJson("null")).toEqual({ value: null });
  expect(responseRows(null, new Set())[0]).toMatchObject({ value: "null", expandable: false });
});
