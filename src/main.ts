/** Starts Vue after the initial desktop route is resolved. */
import { createApp } from "vue";

import { notify } from "./composables/use-notifications";
import App from "./App.vue";
import { router } from "./routes";
import "./styles/app.css";
import "./styles/form-grid.css";
// Load the shared presets once so all tools can use the same interaction timing.
import "./styles/navigation-motion.css";

const app = createApp(App).use(router);
// Report otherwise unhandled Vue failures without displaying their diagnostic details.
app.config.errorHandler = (cause, _instance, context) => {
  console.error("Unexpected WITS Tools failure", context, cause);
  notify({
    id: "unexpected-app-error",
    severity: "error",
    title: "Something went wrong",
    description: "Try the action again.",
  });
};

// Mount after the initial route settles so the correct tool owns the first interaction.
void router.isReady().then(() => app.mount("#app"));
