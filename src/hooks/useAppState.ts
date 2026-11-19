import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTheme } from "next-themes";
import { getSystemAccentColor, getState } from "@/services/api";
import { useAppStore } from "@/store/app";

type ActionPromise = () => Promise<unknown>;

const STATE_KEY = ["app-state"];

export function useStateQuery() {
  const query = useQuery({
    queryKey: STATE_KEY,
    queryFn: getState,
    refetchInterval: 6000,
    staleTime: 2000,
    retry: 2,
  });
  return query;
}

export function useRefetchState() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: STATE_KEY });
}

/**
 * Run a command that mutates backend state, then refresh the dashboard cache and
 * surface the returned message as a toast.
 */
export function useStateAction() {
  const refetch = useRefetchState();
  const pushToast = useAppStore((s) => s.pushToast);
  return useMutation({
    mutationFn: async (action: ActionPromise): Promise<string> => {
      const result = await action();
      const message =
        result &&
        typeof result === "object" &&
        "message" in result &&
        typeof result.message === "string"
          ? result.message
          : "";
      if (message) pushToast(message);
      refetch();
      return message;
    },
  });
}

const ACCENT_VARS = [
  "--accent",
  "--primary",
  "--ring",
  "--accent-strong",
  "--accent-hover",
  "--accent-pressed",
  "--accent-soft",
  "--info",
  "--info-soft",
] as const;

/**
 * Adopt the Windows system accent color in light mode so the app follows the
 * user's OS personalization. Dark mode keeps the Fluent dark accent.
 */
export function useSystemAccent() {
  const { resolvedTheme } = useTheme();
  useEffect(() => {
    const root = document.documentElement;
    if (resolvedTheme === "dark") {
      ACCENT_VARS.forEach((name) => root.style.removeProperty(name));
      return;
    }
    let cancelled = false;
    getSystemAccentColor()
      .then((color) => {
        if (cancelled || !color) return;
        root.style.setProperty("--accent", color);
        root.style.setProperty("--primary", color);
        root.style.setProperty("--ring", color);
        root.style.setProperty("--accent-strong", `color-mix(in srgb, ${color} 82%, black)`);
        root.style.setProperty("--accent-hover", `color-mix(in srgb, ${color} 86%, black)`);
        root.style.setProperty("--accent-pressed", `color-mix(in srgb, ${color} 72%, black)`);
        root.style.setProperty("--accent-soft", `color-mix(in srgb, ${color} 12%, transparent)`);
        root.style.setProperty("--info", color);
        root.style.setProperty("--info-soft", `color-mix(in srgb, ${color} 12%, transparent)`);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [resolvedTheme]);
}

/** Subscribe to Tauri toast/probe events so background work reaches the UI. */
export function useBackendEvents() {
  const pushToast = useAppStore((s) => s.pushToast);
  const refetch = useRefetchState();
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    // `listen` resolves asynchronously; guard cleanup so a strict-mode remount
    // cannot tear down listeners that registered after cleanup already ran.
    Promise.all([
      listen<string>("toast", (event) => pushToast(event.payload)),
      listen<unknown>("probe-changed", () => refetch()),
    ]).then((unsubs) => {
      if (cancelled) unsubs.forEach((un) => un());
      else unlisteners.push(...unsubs);
    });
    return () => {
      cancelled = true;
      unlisteners.forEach((un) => un());
    };
  }, [pushToast, refetch]);
}
