/** Registers the dashboard and tool locations for the desktop shell. */
import { createRouter, createWebHashHistory } from "vue-router";

import DashboardPage from "../pages/dashboard/DashboardPage.vue";
import MarkdownEditorPage from "../pages/markdown-editor/MarkdownEditorPage.vue";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "dashboard", component: DashboardPage },
    { path: "/tools/markdown", name: "markdown-editor", component: MarkdownEditorPage },
    // Load the prototype editor only when its dashboard shortcut is opened.
    {
      path: "/tools/prototype-builder",
      name: "prototype-builder",
      component: () => import("../pages/prototype-builder/PrototypeBuilderPage.vue"),
    },
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
  ],
});
