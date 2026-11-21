/** Gives tools a shared notification interface backed by Vue Sonner. */
import { toast } from "vue-sonner";

type Severity = "error" | "warning" | "info" | "success";

type Notification = {
  severity: Severity;
  title: string;
  description?: string;
  id?: string;
};

// Apply the shared status tokens to all severities and keep keyboard focus in the originating view.
export function notify({ severity, title, description, id }: Notification) {
  return toast[severity](title, {
    id,
    description,
    duration: severity === "error" || severity === "warning" ? 8000 : 5000,
    important: severity === "error",
    style: {
      background: `var(--status-${severity}-surface)`,
      color: `var(--status-${severity}-text)`,
      borderColor: `var(--status-${severity}-indicator)`,
      "--status-toast-indicator": `var(--status-${severity}-indicator)`,
    },
  });
}
