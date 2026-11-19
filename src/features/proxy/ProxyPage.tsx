import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, RefreshCw, ShieldAlert } from "lucide-react";
import {
  Badge,
  Card,
  EmptyState,
  HelpTip,
  Input,
  KeyValue,
  TipButton,
  ToggleRow,
} from "@/components/ui";
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
  rotateCustomProxy,
  setEdgeOverride,
  setEgressMode,
  toggleDesktopServiceProxy,
  toggleStrictMode,
} from "@/services/api";
import { edgeOverrideSchema, proxyPoolSchema, validate } from "@/lib/validation";
import { formatAgo } from "@/lib/format";

export default function ProxyPage() {
  const query = useStateQuery();
  const action = useStateAction();
  const [override, setOverride] = useState("auto");
  const [overrideError, setOverrideError] = useState<string | null>(null);
  const [pool, setPool] = useState("");
  const [poolError, setPoolError] = useState<string | null>(null);

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

  const applyPool = useMutation({
    mutationFn: async (value: string) => {
      const result = validate(proxyPoolSchema, value);
      if (!result.success) {
        setPoolError(result.error);
        throw new Error(result.error);
      }
      setPoolError(null);
      return setEgressMode("custom", value.trim());
    },
  });

  return (
    <PageContent query={query}>
      {(state) => (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <Card
            title={
              <span className="flex items-center gap-1.5">
                Local HTTP/CONNECT proxy
                <HelpTip
                  title="WARP Keeper (Background Watchdog + Desktop Proxy)"
                  body="Runs silently every 45s to keep WARP connected in the selected scope, repair dead tunnels, and host the Desktop/service proxy on 127.0.0.1:41988."
                />
              </span>
            }
            subtitle="Bound to loopback only"
          >
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

          <Card
            title={
              <span className="flex items-center gap-1.5">
                Cloudflare edge
                <HelpTip
                  title="Pinned Cloudflare Edge (AAAA)"
                  body="opencode.ai resolves to multiple Cloudflare IPv6 edge nodes. Pinning an AAAA edge makes new connections use that node; re-roll anytime or pin a specific node."
                />
              </span>
            }
          >
            <KeyValue label="Pinned edge" value={state.lane.pinnedEdge || "auto"} />
            <KeyValue label="Override" value={state.lane.edgeOverride} />
            <KeyValue label="Last bad edge" value={state.proxy.lastBadEdge ?? "—"} />
            {state.stats.blacklist.length > 0 && (
              <div className="mt-3">
                <p className="text-xs font-medium">Blacklisted /64s</p>
                <div className="mt-1 flex flex-wrap gap-1">
                  {state.stats.blacklist.map((prefix) => (
                    <Badge key={prefix} tone="neutral">
                      {prefix}
                    </Badge>
                  ))}
                </div>
              </div>
            )}
            <div className="mt-3 flex flex-wrap gap-2">
              <TipButton
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(rerollEdge)}
                tipTitle="Re-roll Cloudflare Edge"
                tipBody="Switches the active Desktop proxy (127.0.0.1:41988) to a different random opencode.ai IPv6 edge immediately without restarting OpenCode."
              >
                <RefreshCw size={13} />
                Re-roll
              </TipButton>
            </div>
            <div className="mt-3 flex gap-2">
              <Input
                value={override}
                onChange={(event) => setOverride(event.target.value)}
                placeholder="auto or IPv6"
                aria-label="Edge override"
              />
              <TipButton
                size="sm"
                disabled={applyOverride.isPending}
                onClick={() => applyOverride.mutate(override)}
                tipTitle="Edge Selection Mode"
                tipBody="Choose 'Auto-Random' to rotate automatically per session/hop, or pin a specific IPv6 AAAA edge from the live DNS pool."
              >
                Pin edge
              </TipButton>
            </div>
            {overrideError && (
              <p className="mt-2 text-xs" style={{ color: "var(--danger)" }}>
                {overrideError}
              </p>
            )}
          </Card>

          <Card
            title={
              <span className="flex items-center gap-1.5">
                Custom proxy pool
                <HelpTip
                  title="Custom Proxy Mode"
                  body="Route through your own SOCKS5 or HTTP proxy (e.g. Clash, V2Ray, Xray, SingBox, Shadowsocks on 127.0.0.1:10808 or remote)."
                />
              </span>
            }
            subtitle="SOCKS5/HTTP nodes tried in order"
          >
            <div className="flex flex-wrap gap-2">
              <Input
                value={pool}
                onChange={(event) => setPool(event.target.value)}
                placeholder="127.0.0.1:10808 (or multiple: 1.1.1.1:1080, 2.2.2.2:1080)"
                aria-label="Custom proxy pool"
                className="min-w-0 flex-1"
              />
              <TipButton
                size="sm"
                disabled={applyPool.isPending}
                onClick={() => applyPool.mutate(pool)}
                tipTitle="Apply Custom Proxy Pool"
                tipBody="Saves the pool and switches the active egress to custom proxy mode."
              >
                Apply pool
              </TipButton>
              <TipButton
                size="sm"
                variant="ghost"
                disabled={action.isPending}
                onClick={() => action.mutate(rotateCustomProxy)}
                tipTitle="Cycle Next Proxy Node"
                tipBody="Immediately switch to the next proxy node in your multi-proxy pool."
              >
                Next Node
              </TipButton>
            </div>
            {poolError && (
              <p className="mt-2 text-xs" style={{ color: "var(--danger)" }}>
                {poolError}
              </p>
            )}
            <div className="mt-3">
              <KeyValue label="Configured nodes" value={state.egress.customPoolCount} />
              <KeyValue label="Current node" value={state.egress.customCurrentNode || "—"} />
            </div>
          </Card>

          <Card
            title={
              <span className="flex items-center gap-1.5">
                Behavior
                <HelpTip
                  title="Strict Fail-Closed Mode"
                  body="When ON, the proxy refuses connections if WARP is down. When OFF (Recommended), it falls back to a direct connection instead of returning a 503 error."
                />
              </span>
            }
          >
            <div className="flex flex-col gap-3">
              <ToggleRow
                title="Strict fail-closed mode"
                description="Never fall back to a direct route when the selected egress is down."
                checked={state.settings.strictMode}
                disabled={action.isPending}
                onChange={(value) => action.mutate(() => toggleStrictMode(value))}
              />
              <ToggleRow
                title="OpenCode Desktop service"
                description="Wire OpenCode's background service through this proxy."
                checked={state.lane.desktopServiceWired}
                disabled={action.isPending}
                onChange={(value) => action.mutate(() => toggleDesktopServiceProxy(value))}
              />
              <div className="flex flex-wrap gap-2 pt-1">
                <TipButton
                  size="sm"
                  variant="danger"
                  disabled={action.isPending}
                  onClick={() => action.mutate(clearBlacklist)}
                  tipTitle="Clear /64 Blacklist"
                  tipBody="Clears the saved burned /64 list so those prefixes can be tried again after a quota reset."
                >
                  <Ban size={13} />
                  Clear blacklist ({state.stats.blacklistCount})
                </TipButton>
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
                      <TableCell className="whitespace-normal text-xs">{event.detail}</TableCell>
                      <TableCell className="mono text-xs">{event.host || "—"}</TableCell>
                      <TableCell className="mono text-xs">{event.edge || "—"}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Card>

          <Card
            className="xl:col-span-3"
            title={
              <span className="flex items-center gap-1.5">
                Proxy log
                <HelpTip
                  title="Proxy Route Log"
                  body="Connection events from the local proxy: edge failover, dropped streams, direct fallbacks, and tunnel repairs."
                />
              </span>
            }
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
