import { Link } from "react-router-dom";
import { Cloud, Database, EyeOff, Globe, RefreshCw, Route, Wand2, Zap } from "lucide-react";
import { Badge, Button, Card, KeyValue } from "@/components/ui";
import { PageContent } from "@/components/PageContent";
import { useStateAction, useStateQuery } from "@/hooks/useAppState";
import {
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

export default function DashboardPage() {
  const query = useStateQuery();
  const action = useStateAction();
  return (
    <PageContent query={query}>
      {(state) => {
        const busy = action.isPending;
        return (
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
            <Card className="xl:col-span-2">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <h2 className="text-lg font-semibold">{state.health.title}</h2>
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
                <ul className="mt-4 space-y-2">
                  {state.health.issues.map((issue) => (
                    <li
                      key={`${issue.title}-${issue.detail}`}
                      className="flex items-start justify-between gap-3 rounded-lg border px-3 py-2"
                      style={{ borderColor: "var(--border)", background: "var(--surface-2)" }}
                    >
                      <div>
                        <p className="text-[13px] font-medium">{issue.title}</p>
                        <p className="text-xs" style={{ color: "var(--muted)" }}>
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
                <Button disabled={busy} onClick={() => action.mutate(launchDesktop)}>
                  Launch Desktop
                </Button>
                <Button disabled={busy} onClick={() => action.mutate(launchCli)}>
                  Open CLI
                </Button>
                <Button
                  variant="primary"
                  disabled={busy || state.warp.rotating}
                  onClick={() => action.mutate(hopNow)}
                >
                  <Zap size={15} />
                  Rotate now
                </Button>
                <Button
                  disabled={busy}
                  onClick={() => action.mutate(state.warp.connected ? warpDisconnect : warpConnect)}
                >
                  {state.warp.connected ? "Disconnect WARP" : "Connect WARP"}
                </Button>
                <Button disabled={busy} onClick={() => action.mutate(repairWarp)}>
                  Repair WARP
                </Button>
                <Button
                  variant="danger"
                  disabled={busy}
                  onClick={() => action.mutate(fixEverything)}
                >
                  <Wand2 size={15} />
                  Fix everything
                </Button>
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
                {MODES.map((mode) => {
                  const active = state.egress.mode === mode.id;
                  return (
                    <button
                      key={mode.id}
                      className="btn"
                      style={
                        active
                          ? {
                              background: "var(--accent)",
                              borderColor: "var(--accent)",
                              color: "#06120d",
                            }
                          : undefined
                      }
                      disabled={busy}
                      onClick={() => action.mutate(() => setEgressMode(mode.id))}
                    >
                      <mode.icon size={15} />
                      {mode.label}
                    </button>
                  );
                })}
              </div>
            </Card>

            <Card title="Public egress" subtitle="Verified by an outside echo service">
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

            <Card title="Adapter & edge">
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

            <Card title="Local proxy health">
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

            <Card className="xl:col-span-3" title="OpenCode data">
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
