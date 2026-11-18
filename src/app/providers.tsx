import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "next-themes";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useBackendEvents } from "@/hooks/useAppState";
import AppShell from "./AppShell";
import DashboardPage from "@/features/dashboard/DashboardPage";
import NetworkPage from "@/features/networking/NetworkPage";
import ProxyPage from "@/features/proxy/ProxyPage";
import WarpPage from "@/features/warp/WarpPage";
import TorPage from "@/features/tor/TorPage";
import OpencodePage from "@/features/opencode/OpencodePage";
import DiagnosticsPage from "@/features/diagnostics/DiagnosticsPage";
import SettingsPage from "@/features/settings/SettingsPage";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { refetchOnWindowFocus: false, retry: 1 },
  },
});

function EventBridge() {
  useBackendEvents();
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
