import { useQuery } from "@tanstack/react-query";
import { Globe, RefreshCw } from "lucide-react";
import { Badge, Card, EmptyState, ErrorState, LoadingState } from "@/components/ui";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { getInterfaces, type NetworkInterfaceInfo } from "@/services/api";
import { PageContent } from "@/components/PageContent";
import { useStateQuery } from "@/hooks/useAppState";

export default function NetworkPage() {
  const stateQuery = useStateQuery();
  const interfacesQuery = useQuery({
    queryKey: ["interfaces"],
    queryFn: getInterfaces,
    refetchInterval: 10000,
  });

  return (
    <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
      <div className="xl:col-span-2">
        <Card
          title="Network interfaces"
          subtitle="IPv4 and IPv6 addresses reported by the Windows IpHelper API"
          actions={
            <button
              className="btn btn-sm"
              onClick={() => interfacesQuery.refetch()}
              disabled={interfacesQuery.isFetching}
            >
              <RefreshCw size={13} className={interfacesQuery.isFetching ? "animate-spin" : ""} />
              Rescan
            </button>
          }
        >
          {interfacesQuery.isPending ? (
            <LoadingState label="Scanning adapters…" />
          ) : interfacesQuery.error ? (
            <ErrorState title="Could not scan adapters" detail={String(interfacesQuery.error)} />
          ) : !interfacesQuery.data || interfacesQuery.data.length === 0 ? (
            <EmptyState
              title="No adapters found"
              detail="The network stack did not report any interfaces."
            />
          ) : (
            <div className="overflow-x-auto scroll-thin">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Name</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>IPv4</TableHead>
                    <TableHead>IPv6</TableHead>
                    <TableHead>MAC</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {interfacesQuery.data.map((iface) => (
                    <InterfaceRow key={`${iface.name}-${iface.mac}`} iface={iface} />
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </Card>
      </div>

      <PageContent query={stateQuery}>
        {(state) => (
          <div className="flex flex-col gap-4">
            <Card title="Egress verification">
              <div className="flex items-center gap-2">
                <Globe size={18} style={{ color: "var(--accent)" }} />
                <div>
                  <p className="text-[13px] font-medium">Public address</p>
                  <p className="mono text-xs" style={{ color: "var(--muted-text)" }}>
                    {state.warp.egressV6 || "not confirmed"}
                  </p>
                </div>
              </div>
              <div
                className="mt-3 flex flex-col gap-1 text-xs"
                style={{ color: "var(--muted-text)" }}
              >
                <Row label="Prefix" value={state.warp.egressPrefix || "—"} />
                <Row label="Adapter IPv6" value={state.warp.adapterV6 || "missing"} />
                <Row label="Cloudflare range" value={state.warp.isCloudflare ? "yes" : "no"} />
                <Row
                  label="Probe"
                  value={
                    state.probe.ok === null
                      ? "checking"
                      : state.probe.ok
                        ? `ok · ${state.probe.ms} ms`
                        : state.probe.code
                  }
                />
              </div>
              <div className="mt-3">
                <Badge tone={state.probe.ok ? "ok" : state.probe.ok === false ? "warn" : "info"}>
                  {state.probe.ok
                    ? "route verified"
                    : state.probe.ok === false
                      ? "unverified"
                      : "pending"}
                </Badge>
              </div>
            </Card>

            <Card title="Route">
              <div className="flex flex-col gap-1 text-xs" style={{ color: "var(--muted-text)" }}>
                <Row label="Mode" value={state.egress.mode} />
                <Row label="WARP scope" value={state.egress.warpScope} />
                <Row label="Custom node" value={state.egress.customCurrentNode || "—"} />
                <Row label="Tor" value={state.egress.torTarget} />
              </div>
            </Card>
          </div>
        )}
      </PageContent>
    </div>
  );
}

function InterfaceRow({ iface }: { iface: NetworkInterfaceInfo }) {
  return (
    <TableRow>
      <TableCell>
        <div>
          <p className="text-[13px] font-medium">{iface.name}</p>
          {iface.description && iface.description !== iface.name && (
            <p className="text-[11px]" style={{ color: "var(--muted-text)" }}>
              {iface.description}
            </p>
          )}
        </div>
      </TableCell>
      <TableCell>
        <Badge tone={iface.isUp ? "ok" : "neutral"}>{iface.isUp ? "up" : "down"}</Badge>
      </TableCell>
      <TableCell className="mono text-xs">{iface.ipv4.join(", ") || "—"}</TableCell>
      <TableCell className="mono text-xs">
        {iface.ipv6.length > 2
          ? `${iface.ipv6.slice(0, 2).join(", ")} +${iface.ipv6.length - 2}`
          : iface.ipv6.join(", ") || "—"}
      </TableCell>
      <TableCell className="mono text-xs">{iface.mac || "—"}</TableCell>
    </TableRow>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span>{label}</span>
      <span className="mono text-right">{value}</span>
    </div>
  );
}
