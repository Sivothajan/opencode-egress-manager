import { useQuery } from "@tanstack/react-query";
import { EyeOff, Loader2, RefreshCw, Play, Square } from "lucide-react";
import { Badge, Button, Card, KeyValue } from "@/components/ui";
import { PageContent } from "@/components/PageContent";
import { useStateAction, useStateQuery } from "@/hooks/useAppState";
import { readLogs, rotateTorCircuit, toggleTorService } from "@/services/api";

export default function TorPage() {
  const query = useStateQuery();
  const action = useStateAction();

  const logsQuery = useQuery({
    queryKey: ["logs", "tor"],
    queryFn: () => readLogs("tor"),
    refetchInterval: 5000,
  });

  return (
    <PageContent query={query}>
      {(state) => (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <Card title="Tor network" subtitle="Isolated circuit routing for OpenCode traffic">
            <div className="flex items-center gap-2">
              <Badge tone={state.egress.torActive ? "ok" : "warn"}>
                {state.egress.torActive ? "active" : "not running"}
              </Badge>
              <span className="mono text-xs">{state.egress.torTarget}</span>
            </div>
            <div className="mt-3">
              <KeyValue
                label="Portable daemon"
                value={state.egress.torRunning ? "running" : "stopped"}
              />
              <KeyValue label="SOCKS port" value="9050" />
              <KeyValue label="Control port" value="9051" />
              <KeyValue label="Tor Browser hook" value="9150" />
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(() => toggleTorService(true))}
              >
                <Play size={13} />
                Start Tor
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={action.isPending}
                onClick={() => action.mutate(() => toggleTorService(false))}
              >
                <Square size={13} />
                Stop
              </Button>
              <Button
                size="sm"
                disabled={action.isPending}
                onClick={() => action.mutate(rotateTorCircuit)}
              >
                <RefreshCw size={13} />
                New circuit
              </Button>
            </div>
          </Card>

          <Card title="How rotation works">
            <ul
              className="list-disc flex flex-col gap-2 pl-5 text-xs"
              style={{ color: "var(--muted-text)" }}
            >
              <li>
                Sends <code className="mono">SIGNAL NEWNYM</code> to the Tor control port.
              </li>
              <li>Waits for a different public exit address before reporting success.</li>
              <li>If no control port is available, the portable daemon is restarted once.</li>
              <li>Existing Tor Browser on 127.0.0.1:9150 is reused automatically.</li>
            </ul>
            <div
              className="mt-3 flex items-start gap-2 rounded-lg px-3 py-2"
              style={{ background: "var(--accent-soft)", color: "var(--text)" }}
            >
              <EyeOff size={15} className="mt-0.5 shrink-0" style={{ color: "var(--accent)" }} />
              <p className="text-xs">
                Tor changes the exit IP, not the upstream quota policy. Use it when your ISP blocks
                UDP/WireGuard, not as a substitute for another provider.
              </p>
            </div>
          </Card>

          <Card
            title="Tor log"
            actions={
              <button
                className="btn btn-sm"
                onClick={() => logsQuery.refetch()}
                disabled={logsQuery.isFetching}
              >
                {logsQuery.isFetching ? (
                  <Loader2 size={13} className="animate-spin" />
                ) : (
                  <RefreshCw size={13} />
                )}
                Reload
              </button>
            }
          >
            <pre className="log-box scroll-thin">
              {logsQuery.data?.lines.join("\n") || "No Tor log output yet."}
            </pre>
          </Card>
        </div>
      )}
    </PageContent>
  );
}
