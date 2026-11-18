import { NavLink, Outlet, useLocation } from "react-router-dom";
import { useEffect, useState } from "react";
import { useTheme } from "next-themes";
import {
  Activity,
  ArrowLeftRight,
  Cloud,
  EyeOff,
  LayoutDashboard,
  Moon,
  RefreshCw,
  Settings as SettingsIcon,
  Sun,
  Terminal,
  Wifi,
} from "lucide-react";
import logo from "@/assets/logo-mark.png";
import { Badge, Button } from "@/components/ui";
import { useStateQuery } from "@/hooks/useAppState";
import { Toasts } from "@/components/Toasts";

const NAV = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/network", label: "Network", icon: Wifi },
  { to: "/proxy", label: "Proxy", icon: ArrowLeftRight },
  { to: "/warp", label: "WARP", icon: Cloud },
  { to: "/tor", label: "Tor", icon: EyeOff },
  { to: "/opencode", label: "OpenCode", icon: Terminal },
  { to: "/diagnostics", label: "Diagnostics", icon: Activity },
  { to: "/settings", label: "Settings", icon: SettingsIcon },
];

function healthTone(status: string): "ok" | "warn" | "bad" | "info" {
  if (status === "ok") return "ok";
  if (status === "bad") return "bad";
  if (status === "busy") return "info";
  return "warn";
}

function ThemeToggle() {
  const { resolvedTheme, setTheme } = useTheme();
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  if (!mounted) return null;
  const isDark = resolvedTheme === "dark";
  return (
    <button
      className="btn btn-sm"
      onClick={() => setTheme(isDark ? "light" : "dark")}
      aria-label={isDark ? "Switch to light theme" : "Switch to dark theme"}
      title={isDark ? "Switch to light theme" : "Switch to dark theme"}
    >
      {isDark ? <Sun size={14} /> : <Moon size={14} />}
    </button>
  );
}

export default function AppShell() {
  const stateQuery = useStateQuery();
  const location = useLocation();
  const state = stateQuery.data;
  const refreshing = stateQuery.isFetching && !stateQuery.isLoading;

  const active = NAV.find((item) =>
    item.end ? location.pathname === item.to : location.pathname.startsWith(item.to),
  );

  return (
    <div className="flex h-full">
      <aside
        className="flex w-60 shrink-0 flex-col border-r"
        style={{ background: "var(--surface)", borderColor: "var(--border)" }}
      >
        <div className="flex items-center gap-3 px-4 py-4">
          <img src={logo} alt="" width={34} height={34} className="rounded-lg" />
          <div>
            <p className="text-[13px] font-semibold leading-tight">Opencode</p>
            <p className="text-[11px] leading-tight" style={{ color: "var(--muted)" }}>
              Egress Manager
            </p>
          </div>
        </div>
        <nav className="flex flex-col gap-1 px-2">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}
            >
              <item.icon size={17} strokeWidth={1.9} />
              <span className="text-[13px]">{item.label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="mt-auto space-y-2 px-4 py-4 text-[11px]" style={{ color: "var(--muted)" }}>
          <div className="flex items-center justify-between">
            <span>Keeper</span>
            <Badge tone={state?.services.keeperRunning ? "ok" : "warn"}>
              {state?.services.keeperRunning ? "running" : "off"}
            </Badge>
          </div>
          <div className="flex items-center justify-between">
            <span>Guard</span>
            <Badge tone={state?.services.guardRunning ? "ok" : "warn"}>
              {state?.services.guardRunning ? "running" : "off"}
            </Badge>
          </div>
          <div className="flex items-center justify-between">
            <span>Proxy</span>
            <Badge tone={state?.services.proxyListening ? "ok" : "warn"}>
              {state?.services.proxyPort ?? 41987}
            </Badge>
          </div>
        </div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        <header
          className="flex items-center justify-between gap-3 border-b px-5 py-3"
          style={{ background: "var(--surface)", borderColor: "var(--border)" }}
        >
          <div>
            <h1 className="text-[15px] font-semibold">{active?.label ?? "Dashboard"}</h1>
            <p className="text-[11.5px]" style={{ color: "var(--muted)" }}>
              {state?.platform.machineName} · Windows {state?.platform.windowsVersion} · v
              {state?.platform.appVersion}
            </p>
          </div>
          <div className="flex items-center gap-2">
            {state && (
              <Badge tone={healthTone(state.health.status)}>
                <Activity size={12} />
                {state.health.title}
              </Badge>
            )}
            <Button
              size="sm"
              onClick={() => stateQuery.refetch()}
              disabled={refreshing}
              aria-label="Refresh"
            >
              <RefreshCw size={14} className={refreshing ? "animate-spin" : ""} />
              Refresh
            </Button>
            <ThemeToggle />
          </div>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto p-5 scroll-thin">
          <Outlet />
        </div>
      </main>
      <Toasts />
    </div>
  );
}
