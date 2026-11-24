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
  {
    routeName: "crawler",
    name: "Crawler",
    description: "Crawl websites, inspect collected assets, and download media.",
    keywords: "website links scrape html proxy media download video hls dash",
    icon: "globe",
  },
  // Make both direct links and page capture discoverable from the workspace.
  {
    routeName: "downloader",
    name: "Downloader",
    description: "Download files from direct URLs or capture them from a download page.",
    keywords: "download file zip rar exe installer accelerate capture",
    icon: "arrow",
  },
] as const;
