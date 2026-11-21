/** Owns the native window close listener and waits for the shell's close decision. */
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { onMounted, onScopeDispose, ref } from "vue";

export function useWindowClose(authorize: () => Promise<boolean>) {
  const error = ref("");
  let closing = false;
  let disposed = false;
  let unlisten: UnlistenFn | undefined;

  // Register only in the desktop app because browser previews have no native window.
  onMounted(async () => {
    if (!isTauri()) {
      return;
    }

    const window = getCurrentWindow();
    try {
      const stop = await window.onCloseRequested(async (event) => {
        event.preventDefault();
        if (closing) {
          return;
        }

        // Ignore duplicate close requests while a prompt or save is already pending.
        closing = true;
        error.value = "";
        try {
          if (await authorize()) {
            await window.destroy();
          }
        } catch (cause) {
          error.value = String(cause);
        } finally {
          closing = false;
        }
      });

      // Registration can finish after teardown, so release the late listener immediately.
      if (disposed) {
        stop();
      } else {
        unlisten = stop;
      }
    } catch (cause) {
      error.value = String(cause);
    }
  });

  // Release the shell's native listener when the application scope ends.
  onScopeDispose(() => {
    disposed = true;
    unlisten?.();
  });

  return error;
}
