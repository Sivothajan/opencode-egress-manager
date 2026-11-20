import { NavLink, Outlet, useLocation } from "react-router-dom";
import { Suspense, useEffect, useState } from "react";
import { useTheme } from "next-themes";
import { focusActiveNavOnRouteChange } from "@/lib/tauri";
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
import { Badge, HelpTip, LoadingState, TipButton, WarningBanner } from "@/components/ui";
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

  // WebView2 can drop DOM focus during client-side navigation; return it to the
  // active nav link so keyboard users are not left on <body>.
  useEffect(() => {
    focusActiveNavOnRouteChange();
  }, [location.pathname]);

  return (
    <div className="flex h-dvh min-h-0 overflow-hidden bg-background">
      <aside
        className="flex min-h-0 w-[260px] shrink-0 flex-col overflow-hidden border-r border-border"
        style={{ background: "var(--bg)" }}
      >
        <div className="flex items-center gap-3 px-4 pt-5 pb-4">
          <img src={logo} alt="" width={32} height={32} className="rounded-md" />
          <div className="min-w-0">
            <p className="flex items-center gap-1.5 font-heading text-[14px] leading-tight font-semibold text-foreground">
              Opencode Egress Manager
              <HelpTip
                title="Opencode Egress Manager"
                body="Windows controller for Cloudflare WARP IPv6 identity rotation, Cloudflare edge pinning, and quota-wall auto-healing across OpenCode Desktop and OpenCode CLI."
              />
            </p>
            <p className="text-[11px] leading-tight text-muted-foreground">
              Windows control center
            </p>
          </div>
        </div>

        <nav className="scroll-thin flex min-h-0 flex-1 flex-col gap-0.5 overflow-x-hidden overflow-y-auto px-2">
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
              <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
                Keeper
                <HelpTip
                  title="WARP Keeper (Background Watchdog + Desktop Proxy)"
                  body="Runs silently every 45s to keep WARP connected in the selected scope, repair dead tunnels, and host the Desktop/service proxy on 127.0.0.1:41988."
                />
              </span>
              <Badge tone={state?.services.keeperRunning ? "ok" : "warn"}>
                {state?.services.keeperRunning ? "Running" : "Off"}
              </Badge>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
                Guard
                <HelpTip
                  title="Zen Guard (Quota Wall Auto-Rotator)"
                  body="Tails opencode.log every 10s by byte offset. When 'QuotaExceeded' appears, it blacklists the current /64 and rotates WARP automatically. A rotation usually completes within a minute."
                />
              </span>
              <Badge tone={state?.services.guardRunning ? "ok" : "warn"}>
                {state?.services.guardRunning ? "Running" : "Off"}
              </Badge>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="text-[12px] text-muted-foreground">Proxy</span>
              <Badge tone={state?.services.proxyListening ? "ok" : "warn"}>
                {state?.services.proxyPort ?? 41988}
              </Badge>
            </div>
          </div>
        </div>
      </aside>

      <main className="flex min-h-0 min-w-0 flex-1 flex-col">
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
            <HelpTip
              title="Protection & Tunnel Status"
              body="Live overall state. Green means WARP is connected and the egress route is verified. Standard mode falls back to the local route if WARP drops; strict mode refuses requests instead."
            />
            <TipButton
              size="sm"
              onClick={() => stateQuery.refetch()}
              disabled={refreshing}
              aria-label="Refresh"
              tipTitle="Force Live Refresh"
              tipBody="Immediately polls WARP status, queries the active egress route, refreshes the opencode.ai edge pool, and updates all logs."
            >
              <RefreshCw size={13} className={refreshing ? "animate-spin" : ""} />
              Refresh
            </TipButton>
            <ThemeToggle />
          </div>
        </header>

        {state?.management?.pythonInstalled && state.management.warpOwner === "python" && (
          <div className="mx-6 mt-3">
            <WarningBanner>
              <strong>Opencode Unlimited is managing WARP/Tor.</strong> Tauri will not move the
              shared tunnel while its keeper runs. Stop the Python keeper (or uninstall it) to
              control WARP from here.
            </WarningBanner>
          </div>
        )}
        <div className="scroll-thin min-h-0 min-w-0 flex-1 overflow-x-hidden overflow-y-auto px-6 pb-6">
          <Suspense fallback={<LoadingState label="Loading view…" />}>
            <Outlet />
          </Suspense>
        </div>
      </main>
      <Toasts />
    </div>
  );
}
