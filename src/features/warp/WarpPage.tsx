import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Cloud, CloudOff, Loader2, RefreshCw, Wand2, Zap } from "lucide-react";
import {
  Badge,
  Card,
  EmptyState,
  HelpTip,
  KeyValue,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  TipButton,
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
  autoHealEndpoint,
  hopNow,
  repairWarp,
  resetCleanEndpoint,
  setCleanEndpoint,
  setWarpScope,
  testCleanEndpoints,
  warpConnect,
  warpDisconnect,
} from "@/services/api";
import { cleanEndpointSchema, validate } from "@/lib/validation";
import { formatAgo } from "@/lib/format";

export default function WarpPage() {
  const query = useStateQuery();
  const action = useStateAction();
  const [selectedEndpoint, setSelectedEndpoint] = useState("");
  const [endpointError, setEndpointError] = useState<string | null>(null);

  const ping = useMutation({
    mutationFn: () => testCleanEndpoints(900),
  });

  const apply = useMutation({
    mutationFn: async (endpoint: string) => {
      const result = validate(cleanEndpointSchema, endpoint);
      if (!result.success) {
        setEndpointError(result.error);
        throw new Error(result.error);
      }
      setEndpointError(null);
      return setCleanEndpoint(endpoint);
    },
  });

  return (
    <PageContent query={query}>
      {(state) => (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <Card
            title="Cloudflare WARP"
            subtitle={state.warp.bin || "warp-cli.exe not found"}
            actions={
              state.warp.installed ? (
                <Badge tone={state.warp.connected ? "ok" : "warn"}>{state.warp.statusText}</Badge>
              ) : (
                <Badge tone="bad">not installed</Badge>
              )
            }
          >
            <KeyValue label="Connected" value={state.warp.connected ? "yes" : "no"} />
            <KeyValue label="Operation mode" value={state.warp.mode} />
            <KeyValue label="Scope" value={state.egress.warpScope} />
            <KeyValue label="Adapter IPv6" value={state.warp.adapterV6 || "missing"} />
            <KeyValue label="Public IPv6" value={state.warp.egressV6 || "—"} />
            <div className="mt-3 flex flex-wrap gap-2">
              <TipButton
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(state.warp.connected ? warpDisconnect : warpConnect)}
                tipTitle="Connect / Disconnect Cloudflare WARP"
                tipBody="Toggles the Cloudflare WARP tunnel directly. Strict mode refuses requests while WARP is down; standard mode keeps OpenCode usable through the local fallback chain."
              >
                {state.warp.connected ? <CloudOff size={13} /> : <Cloud size={13} />}
                {state.warp.connected ? "Disconnect" : "Connect"}
              </TipButton>
              <TipButton
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(hopNow)}
                tipTitle="Rotate Active Egress Now"
                tipBody="Renews the WireGuard keys, reconnects in the current scope, and waits for a new IPv6 /64. The WARP device registration is kept."
              >
                <Zap size={13} />
                Hop /64
              </TipButton>
              <TipButton
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(repairWarp)}
                tipTitle="Fix WARP / Clear MITM Block"
                tipBody="Renews WARP session keys, reconnects, and recreates the device registration only if it is actually missing. Resolves CF_HAPPY_EYEBALLS_MITM_FAILURE and 'Unable to reach destination' blocks."
              >
                Repair keys
              </TipButton>
            </div>
          </Card>

          <Card title="Scope">
            <p className="mb-3 text-xs" style={{ color: "var(--muted-text)" }}>
              Full TUN routes the whole system through WARP. App-Only keeps normal routing and
              exposes a SOCKS proxy on 127.0.0.1:40000.
            </p>
            <div className="flex flex-col gap-2">
              {(["tun", "isolated"] as const).map((scope) => (
                <label
                  key={scope}
                  className="flex items-center justify-between rounded-lg border px-3 py-2"
                  style={{
                    borderColor:
                      state.egress.warpScope === scope ? "var(--accent)" : "var(--border)",
                  }}
                >
                  <span className="flex items-center gap-1.5 text-[13px]">
                    {scope === "tun" ? "Full System TUN" : "App-Only (Isolated Proxy)"}
                    <HelpTip
                      title={
                        scope === "tun"
                          ? "Full System TUN Mode"
                          : "App-Only Isolated Mode (Recommended)"
                      }
                      body={
                        scope === "tun"
                          ? "Routes all Windows traffic through the CloudflareWARP virtual network adapter. Useful if you need a full system VPN."
                          : "Only OpenCode traffic routes through WARP (port 40000 SOCKS5). Games, Discord, and browsing stay on the normal network path."
                      }
                    />
                  </span>
                  <input
                    type="radio"
                    name="warp-scope"
                    checked={state.egress.warpScope === scope}
                    disabled={action.isPending}
                    onChange={() => action.mutate(() => setWarpScope(scope))}
                  />
                </label>
              ))}
            </div>
            <div className="mt-3">
              <TipButton
                size="sm"
                variant="ghost"
                disabled={action.isPending}
                onClick={() => action.mutate(autoHealEndpoint)}
                tipTitle="Clean WARP Endpoint Selector"
                tipBody="In regions where Cloudflare's default WARP endpoint is blocked by ISPs, switch WARP's WireGuard tunnel endpoint to an alternate address."
              >
                <Wand2 size={13} />
                Auto-heal fastest endpoint
              </TipButton>
            </div>
          </Card>

          <Card title="Rotation">
            <KeyValue label="Hops" value={state.stats.hops} />
            <KeyValue label="Blacklisted /64s" value={state.stats.blacklistCount} />
            <KeyValue label="Failed hops" value={state.stats.failedHops} />
            <KeyValue label="Last hop" value={formatAgo(state.stats.lastHop)} />
            <KeyValue
              label="Paused until"
              value={
                state.stats.backoffUntil > Date.now() / 1000
                  ? formatAgo(state.stats.backoffUntil)
                  : "not paused"
              }
            />
            <KeyValue
              label="Free-wall pause"
              value={
                state.stats.freeWallUntil > Date.now() / 1000
                  ? formatAgo(state.stats.freeWallUntil)
                  : "—"
              }
            />
          </Card>

          <Card
            className="xl:col-span-3"
            title="Clean WireGuard endpoints"
            subtitle="Use when an ISP throttles or blocks Cloudflare's default Anycast path"
            actions={
              <TipButton
                size="sm"
                disabled={ping.isPending}
                onClick={() => ping.mutate()}
                tipTitle="Clean WireGuard Endpoint Selector"
                tipBody="Benchmarks every built-in WireGuard endpoint so you can pick the fastest one for your ISP."
              >
                {ping.isPending ? (
                  <Loader2 size={13} className="animate-spin" />
                ) : (
                  <RefreshCw size={13} />
                )}
                Ping test
              </TipButton>
            }
          >
            <div className="mb-3 flex flex-wrap gap-2">
              <Select
                value={selectedEndpoint}
                onValueChange={(value) => {
                  if (!value) return;
                  setSelectedEndpoint(value);
                  setEndpointError(null);
                }}
              >
                <SelectTrigger className="w-64">
                  <SelectValue placeholder="Select endpoint" />
                </SelectTrigger>
                <SelectContent>
                  {state.egress.cleanEndpoints.map((endpoint) => (
                    <SelectItem key={endpoint} value={endpoint}>
                      {endpoint}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <TipButton
                size="sm"
                disabled={!selectedEndpoint || apply.isPending}
                onClick={() => apply.mutate(selectedEndpoint)}
                tipTitle="Clean WARP Endpoint Selector"
                tipBody="Applies the selected WireGuard endpoint so WARP tunnels through an alternate address."
              >
                Apply endpoint
              </TipButton>
              <TipButton
                size="sm"
                variant="ghost"
                disabled={action.isPending}
                onClick={() => action.mutate(resetCleanEndpoint)}
                tipTitle="Clean WARP Endpoint Selector"
                tipBody="Resets WARP's WireGuard tunnel endpoint to the default (WireGuard) address."
              >
                Reset default
              </TipButton>
            </div>
            {endpointError && (
              <p className="mb-2 text-xs" style={{ color: "var(--danger)" }}>
                {endpointError}
              </p>
            )}
            {ping.data ? (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Endpoint</TableHead>
                    <TableHead>Latency</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {ping.data.map((item) => (
                    <TableRow key={item.endpoint}>
                      <TableCell className="mono">{item.endpoint}</TableCell>
                      <TableCell className="mono">
                        {item.latency === null ? "unreachable" : `${item.latency.toFixed(1)} ms`}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ) : (
              <EmptyState
                title="No latency data yet"
                detail="Press Ping test to benchmark all built-in endpoints."
              />
            )}
          </Card>
        </div>
      )}
    </PageContent>
  );
}
