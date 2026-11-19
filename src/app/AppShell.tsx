import { NavLink, Outlet, useLocation } from "react-router-dom";
import { Suspense, useEffect, useState } from "react";
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
import { Badge, Button, LoadingState } from "@/components/ui";
import { useStateQuery } from "@/hooks/useAppState";
import { Toasts } from "@/components/Toasts";

const NAV = [
  {
    to: "/",
    label: "Dashboard",
    description: "Egress route, health, and quick actions",
    icon: LayoutDashboard,
    end: true,
  },
  {
    to: "/network",
    label: "Network",
    description: "Adapters, addresses, and route verification",
    icon: Wifi,
  },
  {
    to: "/proxy",
    label: "Proxy",
    description: "Local proxy, edge pinning, and route events",
    icon: ArrowLeftRight,
  },
  {
    to: "/warp",
    label: "WARP",
    description: "Cloudflare WARP connection and /64 rotation",
    icon: Cloud,
  },
  {
    to: "/tor",
    label: "Tor",
    description: "Tor circuit routing and portable daemon",
    icon: EyeOff,
  },
  {
    to: "/opencode",
    label: "OpenCode",
    description: "Installations, service wiring, and recovery",
    icon: Terminal,
  },
  {
    to: "/diagnostics",
    label: "Diagnostics",
    description: "System checks and worker event logs",
    icon: Activity,
  },
  {
    to: "/settings",
    label: "Settings",
    description: "Appearance, protection, and maintenance",
    icon: SettingsIcon,
  },
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
    <div className="flex h-full bg-background">
      <aside
        className="flex w-[260px] shrink-0 flex-col border-r border-border"
        style={{ background: "var(--bg)" }}
      >
        <div className="flex items-center gap-3 px-4 pt-5 pb-4">
          <img src={logo} alt="" width={32} height={32} className="rounded-md" />
          <div className="min-w-0">
            <p className="font-heading text-[14px] leading-tight font-semibold text-foreground">
              Opencode Egress Manager
            </p>
            <p className="text-[11px] leading-tight text-muted-foreground">
              Windows control center
            </p>
          </div>
        </div>

        <nav className="flex flex-col gap-0.5 px-2">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}
            >
              <item.icon size={16} strokeWidth={1.8} />
              <span>{item.label}</span>
            </NavLink>
          ))}
        </nav>

        <div className="mt-auto px-4 pb-5">
          <p className="px-2 pb-1.5 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
            Services
          </p>
          <div className="flex flex-col gap-2.5">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[12px] text-muted-foreground">Keeper</span>
              <Badge tone={state?.services.keeperRunning ? "ok" : "warn"}>
                {state?.services.keeperRunning ? "Running" : "Off"}
              </Badge>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="text-[12px] text-muted-foreground">Guard</span>
              <Badge tone={state?.services.guardRunning ? "ok" : "warn"}>
                {state?.services.guardRunning ? "Running" : "Off"}
              </Badge>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="text-[12px] text-muted-foreground">Proxy</span>
              <Badge tone={state?.services.proxyListening ? "ok" : "warn"}>
                {state?.services.proxyPort ?? 41987}
              </Badge>
            </div>
          </div>
        </div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-start justify-between gap-4 px-6 pt-6 pb-4">
          <div className="min-w-0">
            <h1 className="font-heading text-[20px] leading-tight font-semibold text-foreground">
              {active?.label}
            </h1>
            <p className="mt-0.5 text-[12px] text-muted-foreground">{active?.description}</p>
          </div>
          <div className="flex items-center gap-2 pt-0.5">
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
              <RefreshCw size={13} className={refreshing ? "animate-spin" : ""} />
              Refresh
            </Button>
            <ThemeToggle />
          </div>
        </header>

        <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-6 pb-6">
          <Suspense fallback={<LoadingState label="Loading view…" />}>
            <Outlet />
          </Suspense>
        </div>
      </main>
      <Toasts />
    </div>
  );
}
