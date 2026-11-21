/** Starts Vue after the initial desktop route is resolved. */
import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./routes";
import "./styles/app.css";
// Load the shared presets once so all tools can use the same interaction timing.
import "./styles/navigation-motion.css";

const app = createApp(App).use(router);
void router.isReady().then(() => app.mount("#app"));
