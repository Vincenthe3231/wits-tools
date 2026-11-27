/** Lists the tools that are ready to launch from the workspace. */

// Keep navigation labels and searchable descriptions in one catalog.
export const tools = [
  {
    routeName: "editor",
    name: "Editor",
    description: "Create and reopen local documents.",
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
  // Use the shared catalog so Vault appears in sidebar navigation and search.
  {
    routeName: "vault",
    name: "Vault",
    description: "Protect files and folders with a password.",
    keywords: "encrypt lock secure private files folders",
    icon: "lock",
  },
  // Keep the private browser discoverable through the same navigation and search.
  {
    routeName: "browser",
    name: "Private Browser",
    description: "Browse in a separate window that clears its local data when closed.",
    keywords: "browser web website tabs private cookies session downloads",
    icon: "globe",
  },
] as const;
