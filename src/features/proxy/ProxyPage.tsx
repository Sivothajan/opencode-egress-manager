import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, RefreshCw, ShieldAlert } from "lucide-react";
import { Badge, Button, Card, EmptyState, Input, KeyValue, Switch } from "@/components/ui";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { PageContent } from "@/components/PageContent";
import { useStateAction, useStateQuery } from "@/hooks/useAppState";
import {
  clearBlacklist,
  readLogs,
  rerollEdge,
  setEdgeOverride,
  toggleDesktopServiceProxy,
  toggleStrictMode,
} from "@/services/api";
import { edgeOverrideSchema, validate } from "@/lib/validation";
import { formatAgo } from "@/lib/format";

export default function ProxyPage() {
  const query = useStateQuery();
  const action = useStateAction();
  const [override, setOverride] = useState("auto");
  const [overrideError, setOverrideError] = useState<string | null>(null);

  const logsQuery = useQuery({
    queryKey: ["logs", "proxy"],
    queryFn: () => readLogs("proxy"),
    refetchInterval: 5000,
  });

  const applyOverride = useMutation({
    mutationFn: async (value: string) => {
      const result = validate(edgeOverrideSchema, value);
      if (!result.success) {
        setOverrideError(result.error);
        throw new Error(result.error);
      }
      setOverrideError(null);
      return setEdgeOverride(value.trim().toLowerCase());
    },
  });

  return (
    <PageContent query={query}>
      {(state) => (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <Card title="Local HTTP/CONNECT proxy" subtitle="Bound to loopback only">
            <div className="flex items-center gap-2">
              <Badge tone={state.services.proxyListening ? "ok" : "bad"}>
                {state.services.proxyListening ? "listening" : "down"}
              </Badge>
              <span className="mono text-xs">127.0.0.1:{state.services.proxyPort}</span>
            </div>
            <div className="mt-3">
              <KeyValue label="Errors (5m)" value={state.proxy.errors5m} />
              <KeyValue label="Stream resets (5m)" value={state.proxy.streamResets5m} />
              <KeyValue label="Fallbacks (5m)" value={state.proxy.fallbacks5m} />
              <KeyValue label="Last error" value={state.proxy.lastError ?? "none"} />
            </div>
          </Card>

          <Card title="Cloudflare edge">
            <KeyValue label="Pinned edge" value={state.lane.pinnedEdge || "auto"} />
            <KeyValue label="Override" value={state.lane.edgeOverride} />
            <KeyValue label="Last bad edge" value={state.proxy.lastBadEdge ?? "—"} />
            <div className="mt-3 flex flex-wrap gap-2">
              <Button
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(rerollEdge)}
              >
                <RefreshCw size={13} />
                Re-roll
              </Button>
            </div>
            <div className="mt-3 flex gap-2">
              <Input
                value={override}
                onChange={(event) => setOverride(event.target.value)}
                placeholder="auto or IPv6"
                aria-label="Edge override"
              />
              <Button
                size="sm"
                disabled={applyOverride.isPending}
                onClick={() => applyOverride.mutate(override)}
              >
                Pin edge
              </Button>
            </div>
            {overrideError && (
              <p className="mt-2 text-xs" style={{ color: "var(--danger)" }}>
                {overrideError}
              </p>
            )}
          </Card>

          <Card title="Behavior">
            <div className="space-y-3">
              <ToggleLine
                label="Strict fail-closed mode"
                description="Never fall back to a direct route when the selected egress is down."
                checked={state.settings.strictMode}
                disabled={action.isPending}
                onChange={(value) => action.mutate(() => toggleStrictMode(value))}
              />
              <ToggleLine
                label="OpenCode Desktop service"
                description="Wire OpenCode's background service through this proxy."
                checked={state.lane.desktopServiceWired}
                disabled={action.isPending}
                onChange={(value) => action.mutate(() => toggleDesktopServiceProxy(value))}
              />
              <div className="flex flex-wrap gap-2 pt-1">
                <Button
                  size="sm"
                  variant="danger"
                  disabled={action.isPending}
                  onClick={() => action.mutate(clearBlacklist)}
                >
                  <Ban size={13} />
                  Clear blacklist ({state.stats.blacklistCount})
                </Button>
              </div>
            </div>
          </Card>

          <Card
            className="xl:col-span-3"
            title="Route events"
            subtitle="Failover, stream resets, and repairs recorded in the last 15 minutes"
            actions={
              state.proxy.fallbackActive ? (
                <Badge tone="warn">
                  <ShieldAlert size={12} />
                  direct fallback active
                </Badge>
              ) : undefined
            }
          >
            {state.proxy.recent.length === 0 ? (
              <EmptyState
                title="No route events yet"
                detail="Events appear as soon as traffic flows through the proxy."
              />
            ) : (
              <div className="overflow-x-auto scroll-thin">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Time</TableHead>
                      <TableHead>Kind</TableHead>
                      <TableHead>Detail</TableHead>
                      <TableHead>Host</TableHead>
                      <TableHead>Edge</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {[...state.proxy.recent].reverse().map((event, index) => (
                      <TableRow key={`${event.ts}-${index}`}>
                        <TableCell>{formatAgo(event.ts)}</TableCell>
                        <TableCell>
                          <Badge
                            tone={
                              event.kind.startsWith("edge") || event.kind === "error"
                                ? "bad"
                                : event.kind === "fallback"
                                  ? "warn"
                                  : "info"
                            }
                          >
                            {event.kind}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-xs">{event.detail}</TableCell>
                        <TableCell className="mono text-xs">{event.host || "—"}</TableCell>
                        <TableCell className="mono text-xs">{event.edge || "—"}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            )}
          </Card>

          <Card
            className="xl:col-span-3"
            title="Proxy log"
            actions={
              <button className="btn btn-sm" onClick={() => logsQuery.refetch()}>
                Reload
              </button>
            }
          >
            <pre className="log-box scroll-thin">
              {logsQuery.data?.lines.join("\n") || "No log output yet."}
            </pre>
          </Card>
        </div>
      )}
    </PageContent>
  );
}

function ToggleLine({
  label,
  description,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <p className="text-[13px] font-medium">{label}</p>
        {description && (
          <p className="text-xs" style={{ color: "var(--muted-text)" }}>
            {description}
          </p>
        )}
      </div>
      <Switch
        checked={checked}
        onCheckedChange={(value) => onChange(value)}
        disabled={disabled}
        aria-label={label}
      />
    </div>
  );
}
