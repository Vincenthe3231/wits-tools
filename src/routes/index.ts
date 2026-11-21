/** Registers the dashboard and tool locations for the desktop shell. */
import { createRouter, createWebHashHistory } from "vue-router";

import DashboardPage from "../pages/dashboard/DashboardPage.vue";
import MarkdownEditorPage from "../pages/markdown-editor/MarkdownEditorPage.vue";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "dashboard", component: DashboardPage },
    { path: "/tools/markdown", name: "markdown-editor", component: MarkdownEditorPage },
    // Load the API workspace when the user first opens it.
    {
      path: "/tools/api-tester",
      name: "api-tester",
      component: () => import("../pages/api-tester/ApiTesterPage.vue"),
    },
  ],
});
