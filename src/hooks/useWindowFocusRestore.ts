import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { isTauriEnvironment } from "@/lib/tauri";

/**
 * Restore keyboard focus after the OS activates the window.
 *
 * On Windows, Alt+Tab or clicking the taskbar focuses the top-level Tauri window, but
 * WebView2 does not always move input focus into the webview child. The Rust side calls
 * `Window::set_focus` and emits `window-focus-restore` when the window is activated;
 * this hook puts DOM focus back on the element the user had selected, without stealing
 * focus while the app is already active or a dialog has taken focus.
 */
export function useWindowFocusRestore() {
  useEffect(() => {
    if (!isTauriEnvironment()) return;

    let lastFocused: HTMLElement | null = null;
    const trackFocus = (event: FocusEvent) => {
      const target = event.target;
      if (target instanceof HTMLElement && target !== document.body) {
        lastFocused = target;
      }
    };
    document.addEventListener("focusin", trackFocus);

    const unlisten = listen("window-focus-restore", () => {
      const active = document.activeElement;
      const focusWasLost = !active || active === document.body;
      if (
        document.hasFocus() &&
        focusWasLost &&
        lastFocused?.isConnected &&
        lastFocused !== document.body
      ) {
        lastFocused.focus({ preventScroll: true });
      }
    });

    return () => {
      document.removeEventListener("focusin", trackFocus);
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, []);
}
