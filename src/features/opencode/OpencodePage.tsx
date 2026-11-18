import { Database, Monitor, Terminal, Wrench } from "lucide-react";
import { Badge, Button, Card, KeyValue, Switch } from "@/components/ui";
import { PageContent } from "@/components/PageContent";
import { useStateAction, useStateQuery } from "@/hooks/useAppState";
import {
  fixOpencodeService,
  launchCli,
  launchDesktop,
  toggleAutoResume,
  toggleDesktopServiceProxy,
} from "@/services/api";
import { formatBytes } from "@/lib/format";

export default function OpencodePage() {
  const query = useStateQuery();
  const action = useStateAction();

  return (
    <PageContent query={query}>
      {(state) => (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <Card title="Detected installations">
            <div className="space-y-3">
              <div>
                <p className="mb-1 flex items-center gap-2 text-[13px] font-medium">
                  <Terminal size={14} style={{ color: "var(--muted-text)" }} />
                  CLI
                </p>
                <p className="mono break-all text-xs" style={{ color: "var(--muted-text)" }}>
                  {state.lane.cliPath || "not found"}
                </p>
                <Badge tone={state.lane.cliPath ? "ok" : "bad"}>
                  {state.lane.cliPath ? "found" : "missing"}
                </Badge>
              </div>
              <div>
                <p className="mb-1 flex items-center gap-2 text-[13px] font-medium">
                  <Monitor size={14} style={{ color: "var(--muted-text)" }} />
                  Desktop
                </p>
                <p className="mono break-all text-xs" style={{ color: "var(--muted-text)" }}>
                  {state.lane.desktopPath || "not found"}
                </p>
                <Badge tone={state.lane.desktopPath ? "ok" : "neutral"}>
                  {state.lane.desktopPath ? "found" : "missing"}
                </Badge>
              </div>
            </div>
            <div className="mt-4 flex flex-wrap gap-2">
              <Button
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(launchDesktop)}
              >
                Launch Desktop
              </Button>
              <Button
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(launchCli)}
              >
                Open CLI terminal
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={action.isPending}
                onClick={() => action.mutate(fixOpencodeService)}
              >
                <Wrench size={13} />
                Fix service
              </Button>
            </div>
          </Card>

          <Card title="Background service wiring">
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="text-[13px] font-medium">service.json proxy variables</p>
                <p className="text-xs" style={{ color: "var(--muted-text)" }}>
                  HTTPS_PROXY / HTTP_PROXY / NO_PROXY only; other keys are preserved.
                </p>
              </div>
              <Switch
                checked={state.lane.desktopServiceWired}
                onCheckedChange={(value) => action.mutate(() => toggleDesktopServiceProxy(value))}
                disabled={action.isPending}
                aria-label="Wire OpenCode service"
              />
            </div>
            <div className="mt-3">
              <KeyValue label="Wired" value={state.lane.desktopServiceWired ? "yes" : "no"} />
              <KeyValue
                label="Local proxy"
                value={state.services.proxyListening ? "listening" : "down"}
              />
              <KeyValue
                label="User PATH shim"
                value={state.lane.pathWired ? "installed" : "missing"}
              />
            </div>
          </Card>

          <Card title="Session recovery">
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="text-[13px] font-medium">Auto-resume after rotation</p>
                <p className="text-xs" style={{ color: "var(--muted-text)" }}>
                  Clears client lockouts and sends <code className="mono">continue</code> to the
                  affected chat(s).
                </p>
              </div>
              <Switch
                checked={state.settings.autoResume}
                onCheckedChange={(value) => action.mutate(() => toggleAutoResume(value))}
                disabled={action.isPending}
                aria-label="Auto-resume"
              />
            </div>
            <div className="mt-3">
              <KeyValue label="Lockout purge" value="free-limit, go-upsell, quota keys" />
              <KeyValue label="Resume order" value="session → window → last session" />
            </div>
          </Card>

          <Card className="xl:col-span-3" title="Local OpenCode data">
            <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
              <Stat icon={<Database size={15} />} label="Projects" value={state.stats.projects} />
              <Stat icon={<Database size={15} />} label="Chats" value={state.stats.sessions} />
              <Stat icon={<Database size={15} />} label="Messages" value={state.stats.messages} />
              <Stat
                icon={<Database size={15} />}
                label="Log"
                value={formatBytes(state.stats.logSizeBytes)}
              />
            </div>
            <p className="mt-3 text-xs" style={{ color: "var(--muted-text)" }}>
              Only lockout rows in drafts.sqlite are modified. Chat history, sessions, prompts, and
              messages are never touched.
            </p>
          </Card>
        </div>
      )}
    </PageContent>
  );
}

function Stat({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div
      className="rounded-lg border p-3"
      style={{ borderColor: "var(--border)", background: "var(--surface-2)" }}
    >
      <div className="flex items-center gap-2 text-[11.5px]" style={{ color: "var(--muted-text)" }}>
        {icon}
        {label}
      </div>
      <div className="mono mt-1 text-lg font-semibold">{value}</div>
    </div>
  );
}
