/** Registers the dashboard and tool locations for the desktop shell. */
import { createRouter, createWebHashHistory } from "vue-router";

import DashboardPage from "../pages/dashboard/DashboardPage.vue";
import MarkdownEditorPage from "../pages/markdown-editor/MarkdownEditorPage.vue";
import VisualEditorPage from "../pages/visual-editor/VisualEditorPage.vue";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "dashboard", component: DashboardPage },
    // Keep the existing Markdown location available to bookmarks and retained sessions.
    { path: "/tools/editor", name: "editor", component: () => import("../pages/editor/EditorHomePage.vue") },
    { path: "/tools/markdown", name: "markdown-editor", component: MarkdownEditorPage },
    { path: "/tools/visual", name: "visual-editor", component: VisualEditorPage },
    // Load the API workspace when the user first opens it.
    {
      path: "/tools/api-tester",
      name: "api-tester",
      component: () => import("../pages/api-tester/ApiTesterPage.vue"),
    },
    // Load the crawler workspace on demand like other native tools.
    {
      path: "/tools/crawler",
      name: "crawler",
      component: () => import("../pages/crawler/CrawlerPage.vue"),
    },
    // Open the general file downloader independently of the crawler.
    {
      path: "/tools/downloader",
      name: "downloader",
      component: () => import("../pages/downloader/DownloaderPage.vue"),
    },
    // Load the Vault interface only when it is opened.
    {
      path: "/tools/vault",
      name: "vault",
      component: () => import("../pages/vault/VaultPage.vue"),
    },
    // Keep the browser launcher separate from the native browser window.
    {
      path: "/tools/browser",
      name: "browser",
      component: () => import("../pages/browser/BrowserLauncher.vue"),
    },
  ],
});
