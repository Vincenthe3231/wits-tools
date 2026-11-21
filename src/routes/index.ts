/** Registers the dashboard and Markdown editor locations for the desktop shell. */
import { createRouter, createWebHashHistory } from "vue-router";

import DashboardPage from "../pages/dashboard/DashboardPage.vue";
import MarkdownEditorPage from "../pages/markdown-editor/MarkdownEditorPage.vue";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "dashboard", component: DashboardPage },
    { path: "/tools/markdown", name: "markdown-editor", component: MarkdownEditorPage },
  ],
});
