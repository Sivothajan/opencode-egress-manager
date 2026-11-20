import { lazy } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "next-themes";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useBackendEvents, useSystemAccent } from "@/hooks/useAppState";
import { useWindowFocusRestore } from "@/hooks/useWindowFocusRestore";
import AppShell from "./AppShell";

// Split each feature into its own chunk so the initial bundle only carries the
// shell plus whichever route is open.
const DashboardPage = lazy(() => import("@/features/dashboard/DashboardPage"));
const NetworkPage = lazy(() => import("@/features/networking/NetworkPage"));
const ProxyPage = lazy(() => import("@/features/proxy/ProxyPage"));
const WarpPage = lazy(() => import("@/features/warp/WarpPage"));
const TorPage = lazy(() => import("@/features/tor/TorPage"));
const OpencodePage = lazy(() => import("@/features/opencode/OpencodePage"));
const DiagnosticsPage = lazy(() => import("@/features/diagnostics/DiagnosticsPage"));
const SettingsPage = lazy(() => import("@/features/settings/SettingsPage"));

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { refetchOnWindowFocus: false, retry: 1 },
  },
});

function EventBridge() {
  useBackendEvents();
  useSystemAccent();
  useWindowFocusRestore();
  return null;
}

export default function Providers() {
  return (
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <HashRouter>
            <EventBridge />
            <Routes>
              <Route element={<AppShell />}>
                <Route index element={<DashboardPage />} />
                <Route path="network" element={<NetworkPage />} />
                <Route path="proxy" element={<ProxyPage />} />
                <Route path="warp" element={<WarpPage />} />
                <Route path="tor" element={<TorPage />} />
                <Route path="opencode" element={<OpencodePage />} />
                <Route path="diagnostics" element={<DiagnosticsPage />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Route>
            </Routes>
          </HashRouter>
        </TooltipProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
