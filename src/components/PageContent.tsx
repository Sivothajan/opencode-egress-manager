import type { UseQueryResult } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { ErrorState, LoadingState } from "@/components/ui";
import { isBackendUnavailable } from "@/lib/tauri";
import type { AppState } from "@/types";

export function PageContent({
  query,
  children,
}: {
  query: UseQueryResult<AppState>;
  children: (state: AppState) => ReactNode;
}) {
  if (query.isPending) return <LoadingState label="Reading live state…" />;
  if (query.error) {
    const unavailable = isBackendUnavailable(query.error);
    return (
      <ErrorState
        title={unavailable ? "Backend unavailable" : "Could not read application state"}
        detail={
          unavailable
            ? "The Tauri backend did not answer. Run the app through bun run tauri dev."
            : String(query.error)
        }
        onRetry={() => query.refetch()}
      />
    );
  }
  if (!query.data) return <LoadingState />;
  return <>{children(query.data)}</>;
}
