import { Link } from "react-router-dom";
import { Cloud, Database, EyeOff, Globe, RefreshCw, Route, Wand2, Zap } from "lucide-react";
import { Badge, Button, Card, HelpTip, KeyValue, Tip, TipButton } from "@/components/ui";
import { PageContent } from "@/components/PageContent";
import { useStateAction, useStateQuery } from "@/hooks/useAppState";
import {
  deepResetIdentity,
  fixEverything,
  fixIssue,
  hopNow,
  launchCli,
  launchDesktop,
  repairWarp,
  setEgressMode,
  warpConnect,
  warpDisconnect,
} from "@/services/api";
import { formatAgo, normalizePrefix } from "@/lib/format";
import type { EgressMode } from "./modes";

const MODES: Array<{ id: EgressMode; label: string; icon: typeof Cloud }> = [
  { id: "warp", label: "WARP", icon: Cloud },
  { id: "tor", label: "Tor", icon: EyeOff },
  { id: "custom", label: "Custom Proxy", icon: Route },
  { id: "clean", label: "Clean Endpoint", icon: Globe },
];

const MODE_TIPS: Record<EgressMode, { title: string; body: string }> = {
  warp: {
    title: "Cloudflare WARP Mode (Default)",
    body: "Routes opencode.ai through local CloudflareWARP WinTun adapter, rotating /64 IPv6 subnets automatically on quota limit.",
  },
  tor: {
    title: "Tor Network Mode",
    body: "Routes opencode.ai traffic through Tor SOCKS5 (port 9050/9150). Uses Tor exit nodes in regions where WARP is blocked (Russia, Iran, etc.).",
  },
  custom: {
    title: "Custom Proxy Mode",
    body: "Route through your own SOCKS5 or HTTP proxy (e.g. Clash, V2Ray, Xray, SingBox, Shadowsocks on 127.0.0.1:10808 or remote).",
  },
  clean: {
    title: "Clean WARP Endpoint Selector",
    body: "In regions where Cloudflare's default WARP endpoint is blocked by ISPs, switch WARP's WireGuard tunnel endpoint to an alternate address.",
  },
};

export default function DashboardPage() {
  const query = useStateQuery();
  const action = useStateAction();
  return (
    <PageContent query={query}>
      {(state) => {
        const busy = action.isPending;
        const pythonOwnsWarp = state.management.warpOwner === "python";
        return (
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
            <Card className="xl:col-span-2">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <h2 className="flex items-center gap-1.5 text-lg font-semibold">
                    {state.health.title}
                    <HelpTip
                      title="Protection & Tunnel Status"
                      body="Live overall state. Green means WARP is connected and the egress route is verified. Standard mode falls back to the local route if WARP drops; strict mode refuses requests instead."
                    />
                  </h2>
                  <p className="mt-1 text-sm" style={{ color: "var(--muted)" }}>
                    {state.health.sub}
                  </p>
                </div>
                <Badge
                  tone={
                    state.health.status === "ok"
                      ? "ok"
                      : state.health.status === "bad"
                        ? "bad"
                        : "warn"
                  }
                >
                  {state.health.status.toUpperCase()}
                </Badge>
              </div>
              {state.health.issues.length > 0 && (
                <ul className="mt-4 flex flex-col gap-2">
                  {state.health.issues.map((issue) => (
                    <li
                      key={`${issue.title}-${issue.detail}`}
                      className="flex items-start justify-between gap-3 rounded-lg border px-3 py-2"
                      style={{ borderColor: "var(--border)", background: "var(--surface-2)" }}
                    >
                      <div className="min-w-0">
                        <p className="text-[13px] font-medium">{issue.title}</p>
                        <p className="break-words text-xs" style={{ color: "var(--muted)" }}>
                          {issue.detail}
                        </p>
                      </div>
                      {issue.fix && (
                        <Button
                          size="sm"
                          disabled={busy}
                          onClick={() => action.mutate(() => fixIssue(issue.fix!))}
                        >
                          {issue.fixLabel}
                        </Button>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </Card>

            <Card title="Connection">
              <div className="grid grid-cols-2 gap-2">
                <TipButton
                  disabled={busy || pythonOwnsWarp}
                  onClick={() => action.mutate(launchDesktop)}
                  tipTitle="Launch OpenCode Desktop (OpenCode.exe)"
                  tipBody="Opens the OpenCode Desktop GUI app wired to 127.0.0.1:41988. All 5 projects, 36+ existing chats, and ongoing sessions load normally from opencode.db."
                >
                  Launch Desktop
                </TipButton>
                <TipButton
                  disabled={busy}
                  onClick={() => action.mutate(launchCli)}
                  tipTitle="Launch OpenCode CLI (Zen Lane)"
                  tipBody="Opens a fresh terminal running OpenCode CLI in --standalone mode with its own per-launch IPv6 proxy and random Cloudflare edge."
                >
                  Open CLI
                </TipButton>
                <TipButton
                  variant="primary"
                  disabled={busy || state.warp.rotating || pythonOwnsWarp}
                  onClick={() => action.mutate(hopNow)}
                  tipTitle="Rotate Active Egress Now"
                  tipBody="Rotates the active egress. In WARP mode this pauses the keeper, renews the WireGuard keys, reconnects in the current scope, and waits for a new IPv6 /64. In Tor mode it builds a new circuit; in custom mode it switches to the next proxy node. The WARP device registration is kept."
                >
                  <Zap size={15} />
                  Rotate now
                </TipButton>
                <TipButton
                  disabled={busy || pythonOwnsWarp}
                  onClick={() => action.mutate(state.warp.connected ? warpDisconnect : warpConnect)}
                  tipTitle="Connect / Disconnect Cloudflare WARP"
                  tipBody="Toggles the Cloudflare WARP tunnel directly. Strict mode refuses requests while WARP is down; standard mode keeps OpenCode usable through the local fallback chain."
                >
                  {state.warp.connected ? "Disconnect WARP" : "Connect WARP"}
                </TipButton>
                <TipButton
                  disabled={busy || pythonOwnsWarp}
                  onClick={() => action.mutate(repairWarp)}
                  tipTitle="Fix WARP / Clear MITM Block"
                  tipBody="Renews WARP session keys, reconnects, and recreates the device registration only if it is actually missing. Resolves CF_HAPPY_EYEBALLS_MITM_FAILURE and 'Unable to reach destination' blocks."
                >
                  Repair WARP
                </TipButton>
                <TipButton
                  variant="danger"
                  disabled={busy}
                  onClick={() => action.mutate(fixEverything)}
                  tipTitle="Fix Everything"
                  tipBody="Runs the one-click fixes suggested by the health check, most serious first. It won't delete WARP registration or touch your chats."
                >
                  <Wand2 size={15} />
                  Fix everything
                </TipButton>
              </div>
              <div className="mt-3 text-right">
                <Link
                  to="/diagnostics"
                  className="text-xs"
                  style={{ color: "var(--accent-strong)" }}
                >
                  Open diagnostics →
                </Link>
              </div>
            </Card>

            <Card className="xl:col-span-3">
              <h2 className="mb-3 text-[15px] font-semibold">Egress route</h2>
              <div className="flex flex-wrap gap-2">
                <TipButton
                  variant="danger"
                  disabled={busy}
                  onClick={() => action.mutate(deepResetIdentity)}
                  tipTitle="Deep Reset Identity"
                  tipBody="Clears transient cache, clears the /64 blacklist, and immediately rotates your IP. Chat sessions and Project data are preserved."
                >
                  <RefreshCw size={15} />
                  Deep Reset
                </TipButton>
                {MODES.map((mode) => {
                  const active = state.egress.mode === mode.id;
                  const tip = MODE_TIPS[mode.id];
                  return (
                    <Tip title={tip.title} body={tip.body}>
                      <button
                        key={mode.id}
                        className="btn"
                        style={
                          active
                            ? {
                                background: "var(--accent)",
                                borderColor: "var(--accent)",
                                color: "#ffffff",
                              }
                            : undefined
                        }
                        disabled={busy || (pythonOwnsWarp && mode.id !== "custom")}
                        onClick={() => action.mutate(() => setEgressMode(mode.id))}
                      >
                        <mode.icon size={15} />
                        {mode.label}
                      </button>
                    </Tip>
                  );
                })}
              </div>
            </Card>

            <Card
              title={
                <span className="flex items-center gap-1.5">
                  Public egress
                  <HelpTip
                    title="External Egress Address"
                    body="The public address seen by opencode.ai. In WARP mode this is your IPv6 /64 prefix; Tor and custom-proxy modes show the node exit IP. Click to copy the full address."
                  />
                </span>
              }
              subtitle="Verified by an outside echo service"
            >
              <div
                className="mono text-xl"
                style={{ color: state.warp.isCloudflare ? "var(--ok)" : "var(--text)" }}
              >
                {normalizePrefix(state.warp.egressV6) || "—"}
              </div>
              <KeyValue label="Address" value={state.warp.egressV6 || "checking"} />
              <KeyValue label="Cloudflare" value={state.warp.isCloudflare ? "yes" : "no"} />
              <KeyValue label="Last hop" value={formatAgo(state.stats.lastHop)} />
            </Card>

            <Card
              title={
                <span className="flex items-center gap-1.5">
                  Adapter & edge
                  <HelpTip
                    title="Active Egress Interface / Proxy Node"
                    body="Egress tunnel interface or local proxy node. The proxy explicitly routes sockets through this interface to prevent ISP tracking."
                  />
                </span>
              }
            >
              <KeyValue label="WARP adapter IPv6" value={state.warp.adapterV6 || "missing"} />
              <KeyValue label="Pinned edge" value={state.lane.pinnedEdge || "auto"} />
              <KeyValue label="Scope" value={state.egress.warpScope} />
              <div className="mt-2 flex gap-2">
                <Button
                  size="sm"
                  disabled={busy}
                  onClick={() => action.mutate(() => setEgressMode("warp"))}
                >
                  <RefreshCw size={13} />
                  Re-check
                </Button>
              </div>
            </Card>

            <Card
              title={
                <span className="flex items-center gap-1.5">
                  Local proxy health
                  <HelpTip
                    title="WARP Keeper (Background Watchdog + Desktop Proxy)"
                    body="Runs silently every 45s to keep WARP connected in the selected scope, repair dead tunnels, and host the Desktop/service proxy on 127.0.0.1:41988."
                  />
                </span>
              }
            >
              <div className="flex items-center gap-2">
                <Badge tone={state.services.proxyListening ? "ok" : "bad"}>
                  {state.services.proxyListening ? "listening" : "down"}
                </Badge>
                <span className="mono text-xs">{state.services.proxyPort}</span>
              </div>
              <KeyValue label="Errors (5m)" value={state.proxy.errors5m} />
              <KeyValue label="Stream resets (5m)" value={state.proxy.streamResets5m} />
              <KeyValue label="Fallbacks (5m)" value={state.proxy.fallbacks5m} />
              <KeyValue
                label="Probe"
                value={
                  state.probe.ok === null
                    ? "checking"
                    : state.probe.ok
                      ? `ok (${state.probe.ms} ms)`
                      : state.probe.code
                }
              />
            </Card>

            <Card
              className="xl:col-span-3"
              title={
                <span className="flex items-center gap-1.5">
                  OpenCode data
                  <HelpTip
                    title="Auto-Rotation & Existing Data"
                    body="Counts automatic and manual /64 rotations, blacklisted /64s (up to 200), and the projects and sessions stored in opencode.db."
                  />
                </span>
              }
            >
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
                <Stat label="Projects" value={state.stats.projects} icon={<Database size={15} />} />
                <Stat label="Chats" value={state.stats.sessions} icon={<Database size={15} />} />
                <Stat label="Messages" value={state.stats.messages} icon={<Database size={15} />} />
                <Stat
                  label="Log size"
                  value={`${(state.stats.logSizeBytes / 1024).toFixed(0)} KB`}
                  icon={<Database size={15} />}
                />
              </div>
            </Card>
          </div>
        );
      }}
    </PageContent>
  );
}

function Stat({
  label,
  value,
  icon,
}: {
  label: string;
  value: number | string;
  icon: React.ReactNode;
}) {
  return (
    <div
      className="rounded-lg border p-3"
      style={{ borderColor: "var(--border)", background: "var(--surface-2)" }}
    >
      <div className="flex items-center gap-2 text-[11.5px]" style={{ color: "var(--muted)" }}>
        {icon}
        {label}
      </div>
      <div className="mono mt-1 text-lg font-semibold">{value}</div>
    </div>
  );
}
