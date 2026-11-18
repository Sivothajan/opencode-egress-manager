import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Activity,
  CheckCircle2,
  ClipboardCopy,
  FolderOpen,
  Loader2,
  XCircle,
  AlertTriangle,
} from "lucide-react";
import { Badge, Button, Card, KeyValue, Switch } from "@/components/ui";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { PageContent } from "@/components/PageContent";
import { useStateAction, useStateQuery } from "@/hooks/useAppState";
import { openLogsFolder, readLogs, toggleAutoRepair } from "@/services/api";
import type { AppState } from "@/types";

const LOG_TABS = [
  { id: "guard", label: "Guard" },
  { id: "keeper", label: "Keeper" },
  { id: "opencode", label: "OpenCode" },
  { id: "proxy", label: "Proxy" },
  { id: "tor", label: "Tor" },
];

export default function DiagnosticsPage() {
  const query = useStateQuery();
  const action = useStateAction();
  const [logTab, setLogTab] = useState("guard");

  const logQuery = useQuery({
    queryKey: ["logs", logTab],
    queryFn: () => readLogs(logTab),
    refetchInterval: 4000,
  });

  const copyDiagnostics = async (state: AppState) => {
    try {
      await navigator.clipboard.writeText(JSON.stringify(state, null, 2));
    } catch {
      // Clipboard can be blocked before the window has focus; the UI still shows data.
    }
  };

  return (
    <PageContent query={query}>
      {(state) => {
        const checks = buildChecks(state);
        return (
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
            <Card title="System">
              <KeyValue label="Windows" value={state.platform.windowsVersion} />
              <KeyValue
                label="Release"
                value={state.platform.isWin11 ? "Windows 11" : "Windows 10"}
              />
              <KeyValue label="Machine" value={state.platform.machineName} />
              <KeyValue label="Administrator" value={state.platform.isAdmin ? "yes" : "no"} />
              <KeyValue label="Application version" value={`v${state.platform.appVersion}`} />
            </Card>

            <Card
              title="Checks"
              subtitle={`${checks.filter((c) => c.tone === "ok").length}/${checks.length} healthy`}
            >
              <ul className="space-y-2">
                {checks.map((check) => (
                  <li key={check.label} className="flex items-start justify-between gap-3">
                    <div className="flex items-start gap-2">
                      {check.tone === "ok" ? (
                        <CheckCircle2
                          size={15}
                          className="mt-0.5 shrink-0"
                          style={{ color: "var(--ok)" }}
                        />
                      ) : check.tone === "warn" ? (
                        <AlertTriangle
                          size={15}
                          className="mt-0.5 shrink-0"
                          style={{ color: "var(--warn)" }}
                        />
                      ) : (
                        <XCircle
                          size={15}
                          className="mt-0.5 shrink-0"
                          style={{ color: "var(--danger)" }}
                        />
                      )}
                      <div>
                        <p className="text-[13px] font-medium">{check.label}</p>
                        <p className="text-xs" style={{ color: "var(--muted-text)" }}>
                          {check.detail}
                        </p>
                      </div>
                    </div>
                    <Badge tone={check.tone}>{check.tone}</Badge>
                  </li>
                ))}
              </ul>
            </Card>

            <Card
              title="Auto-repair"
              subtitle={state.autoRepair.status}
              actions={
                <Switch
                  checked={state.autoRepair.enabled}
                  onCheckedChange={(value) => action.mutate(() => toggleAutoRepair(value))}
                  disabled={action.isPending}
                  aria-label="Auto-repair"
                />
              }
            >
              <p className="mb-3 text-xs" style={{ color: "var(--muted-text)" }}>
                Safe actions only: restart the proxy, reconnect WARP, re-roll an edge, or rotate the
                egress. It never changes your mode, restarts OpenCode, or deletes registration.
              </p>
              {state.autoRepair.actions.length === 0 ? (
                <p className="text-xs" style={{ color: "var(--muted-text)" }}>
                  No repairs recorded.
                </p>
              ) : (
                <ul className="space-y-1.5 text-xs">
                  {state.autoRepair.actions
                    .slice()
                    .reverse()
                    .map((entry, index) => (
                      <li key={`${entry.ts}-${index}`} className="flex items-center gap-2">
                        <Activity
                          size={12}
                          style={{ color: entry.ok ? "var(--ok)" : "var(--danger)" }}
                        />
                        <span className="mono">{entry.action}</span>
                        <span style={{ color: "var(--muted-text)" }}>{entry.message}</span>
                      </li>
                    ))}
                </ul>
              )}
              <div className="mt-3 flex flex-wrap gap-2">
                <Button size="sm" variant="ghost" onClick={() => copyDiagnostics(state)}>
                  <ClipboardCopy size={13} />
                  Copy diagnostics
                </Button>
                <Button size="sm" variant="ghost" onClick={() => openLogsFolder()}>
                  <FolderOpen size={13} />
                  Logs folder
                </Button>
              </div>
            </Card>

            <Card className="xl:col-span-3" title="Event logs">
              <Tabs value={logTab} onValueChange={setLogTab}>
                <TabsList>
                  {LOG_TABS.map((tab) => (
                    <TabsTrigger key={tab.id} value={tab.id}>
                      {tab.label}
                    </TabsTrigger>
                  ))}
                </TabsList>
                {LOG_TABS.map((tab) => (
                  <TabsContent key={tab.id} value={tab.id}>
                    <div className="mb-2 flex justify-end">
                      <Button size="sm" variant="ghost" onClick={() => logQuery.refetch()}>
                        {logQuery.isFetching ? (
                          <Loader2 size={13} className="animate-spin" />
                        ) : null}
                        Reload
                      </Button>
                    </div>
                    <pre className="log-box scroll-thin">
                      {logQuery.data?.lines.join("\n") || "No log output yet."}
                    </pre>
                  </TabsContent>
                ))}
              </Tabs>
            </Card>
          </div>
        );
      }}
    </PageContent>
  );
}

type Tone = "ok" | "warn" | "bad";

function buildChecks(state: AppState): Array<{ label: string; detail: string; tone: Tone }> {
  const checks: Array<{ label: string; detail: string; tone: Tone }> = [];
  checks.push({
    label: "Cloudflare WARP",
    detail: state.warp.installed
      ? state.warp.connected
        ? `connected (${state.warp.mode})`
        : "installed but disconnected"
      : "warp-cli.exe not found",
    tone: !state.warp.installed ? "bad" : state.warp.connected ? "ok" : "warn",
  });
  checks.push({
    label: "Local proxy",
    detail: state.services.proxyListening
      ? `listening on ${state.services.proxyPort}`
      : `not listening on ${state.services.proxyPort}`,
    tone: state.services.proxyListening ? "ok" : "bad",
  });
  checks.push({
    label: "WARP Keeper",
    detail: heartbeatDetail(state.services.keeperHeartbeat),
    tone: state.services.keeperRunning
      ? state.services.keeperHeartbeat !== null && state.services.keeperHeartbeat > 240
        ? "warn"
        : "ok"
      : "warn",
  });
  checks.push({
    label: "Zen Guard",
    detail: heartbeatDetail(state.services.guardHeartbeat),
    tone: state.services.guardRunning
      ? state.services.guardHeartbeat !== null && state.services.guardHeartbeat > 240
        ? "warn"
        : "ok"
      : "warn",
  });
  checks.push({
    label: "Autostart tasks",
    detail: state.services.autostartRegistered
      ? `${state.services.keeperTaskExists ? "keeper" : "—"} / ${state.services.guardTaskExists ? "guard" : "—"}`
      : "not registered",
    tone: state.services.autostartRegistered ? "ok" : "warn",
  });
  checks.push({
    label: "Firewall kill-switch",
    detail: state.services.firewallInstalled
      ? "OUC_LEAKGUARD_CLI installed"
      : "not installed (optional)",
    tone: state.services.firewallInstalled ? "ok" : "warn",
  });
  checks.push({
    label: "OpenCode wiring",
    detail: state.lane.desktopServiceWired
      ? "service.json routed through the local proxy"
      : state.lane.pathWired
        ? "CLI shim on PATH"
        : "direct connection",
    tone: state.lane.desktopServiceWired || state.lane.pathWired ? "ok" : "warn",
  });
  checks.push({
    label: "OpenCode CLI",
    detail: state.lane.cliPath || "not found",
    tone: state.lane.cliPath ? "ok" : "bad",
  });
  checks.push({
    label: "Probe",
    detail:
      state.probe.ok === true
        ? `reached opencode.ai in ${state.probe.ms} ms`
        : state.probe.ok === false
          ? `failed (${state.probe.code})`
          : "pending",
    tone: state.probe.ok === true ? "ok" : state.probe.ok === false ? "warn" : "warn",
  });
  return checks;
}

function heartbeatDetail(age: number | null): string {
  if (age === null) return "running, no heartbeat yet";
  if (age < 60) return `alive ${Math.round(age)}s ago`;
  if (age < 240) return `last check-in ${Math.round(age / 60)} min ago`;
  return `stale for ${Math.round(age / 60)} min`;
}
