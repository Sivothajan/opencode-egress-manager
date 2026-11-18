import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getState } from "@/services/api";
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

/** Subscribe to Tauri toast/probe events so background work reaches the UI. */
export function useBackendEvents() {
  const pushToast = useAppStore((s) => s.pushToast);
  const refetch = useRefetchState();
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    listen<string>("toast", (event) => pushToast(event.payload)).then((un) => unlisteners.push(un));
    listen<unknown>("probe-changed", () => refetch()).then((un) => unlisteners.push(un));
    return () => {
      unlisteners.forEach((un) => un());
    };
  }, [pushToast, refetch]);
}
