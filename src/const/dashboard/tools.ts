/** Lists the tools that are ready to launch from the workspace. */

// Keep navigation labels and searchable descriptions in one catalog.
export const tools = [
  {
    routeName: "prototype-builder",
    name: "Prototype Builder",
    description: "Import a GitHub dashboard starter, rearrange cards, and save a local variant.",
    keywords: "prototype github dashboard drag drop remix starter",
    icon: "document",
  },
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
  {
    routeName: "crawler",
    name: "Crawler",
    description: "Crawl websites, inspect collected assets, and download media.",
    keywords: "website links scrape html proxy media download video hls dash",
    icon: "globe",
  },
] as const;
