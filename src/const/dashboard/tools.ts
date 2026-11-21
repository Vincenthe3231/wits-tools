/** Lists the tools that are ready to launch from the workspace. */

// Keep navigation labels and searchable descriptions in one catalog.
export const tools = [
  {
    routeName: "markdown-editor",
    name: "Markdown Editor",
    description: "Write and edit local Markdown files in Visual or Source mode.",
    keywords: "writing documents notes md text",
    icon: "document",
  },
  {
    routeName: "api-tester",
    name: "API Tester",
    description: "Send HTTP requests, inspect responses, and check status and timing.",
    keywords: "api http https rest request response curl test status headers",
    icon: "arrow",
  },
] as const;
