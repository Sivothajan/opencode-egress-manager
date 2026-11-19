// Mirrors the Rust DTOs serialized by Tauri commands (camelCase).

export interface PlatformInfo {
  windowsVersion: string;
  isWin11: boolean;
  isWin10OrNewer: boolean;
  isAdmin: boolean;
  machineName: string;
  appVersion: string;
}

export interface WarpState {
  installed: boolean;
  bin: string;
  connected: boolean;
  mode: string;
  egressMode: string;
  statusText: string;
  adapterV6: string;
  egressV6: string;
  egressPrefix: string;
  isCloudflare: boolean;
  checkingEgress: boolean;
  rotating: boolean;
}

export interface ServicesState {
  keeperRunning: boolean;
  guardRunning: boolean;
  keeperTaskExists: boolean;
  guardTaskExists: boolean;
  keeperTaskEnabled: boolean;
  guardTaskEnabled: boolean;
  proxyListening: boolean;
  proxyPort: number;
  firewallInstalled: boolean;
  isAdmin: boolean;
  autoResume: boolean;
  keeperHeartbeat: number | null;
  guardHeartbeat: number | null;
  autostartRegistered: boolean;
}

export interface LaneState {
  pinnedEdge: string;
  edgeOverride: string;
  edgePoolCount: number;
  strictMode: boolean;
  cliPath: string;
  desktopPath: string;
  pathWired: boolean;
  desktopServiceWired: boolean;
  shimsInstalled: boolean;
}

export interface StatsState {
  hops: number;
  blacklistCount: number;
  blacklist: string[];
  lastHop: number;
  failedHops: number;
  backoffUntil: number;
  freeWallUntil: number;
  projects: number;
  sessions: number;
  messages: number;
  logSizeBytes: number;
}

export interface LogsState {
  guard: string[];
  keeper: string[];
  opencode: string[];
  proxy: string[];
}

export interface ProxyEventEntry {
  ts: number;
  kind: string;
  detail: string;
  host: string;
  edge: string;
}

export interface ProxyState {
  errors5m: number;
  streamResets5m: number;
  fallbacks5m: number;
  lastError: string | null;
  edge: string | null;
  lastBadEdge: string | null;
  fallbackAge: number | null;
  fallbackActive: boolean;
  recent: ProxyEventEntry[];
}

export interface EgressState {
  mode: "warp" | "clean" | "tor" | "custom" | string;
  warpScope: "tun" | "isolated" | string;
  customProxy: string;
  customType: "socks5" | "http" | string;
  torActive: boolean;
  torTarget: string;
  torRunning: boolean;
  customPoolCount: number;
  customCurrentNode: string;
  cleanEndpoints: string[];
}

export interface HealthIssue {
  level: "bad" | "warn" | "info" | string;
  title: string;
  detail: string;
  fix?: string | null;
  fixLabel: string;
}

export interface HealthSummary {
  status: "ok" | "warn" | "bad" | "busy" | string;
  title: string;
  sub: string;
  issues: HealthIssue[];
}

export interface ProbeState {
  ok: boolean | null;
  code: string;
  ms: number;
  ts: number;
  checking: boolean;
}

export interface RepairActionEntry {
  ts: number;
  action: string;
  reason: string;
  ok: boolean;
  message: string;
}

export interface RepairSnapshot {
  enabled: boolean;
  status: string;
  actions: RepairActionEntry[];
  maxPerHour: number;
}

export interface AppSettings {
  autoResume: boolean;
  strictMode: boolean;
  autoRepair: boolean;
  autostart: boolean;
  theme: "system" | "light" | "dark" | string;
}

export interface ManagementStatus {
  pythonInstalled: boolean;
  pythonKeeperRunning: boolean;
  pythonGuardRunning: boolean;
  warpOwner: "tauri" | "python" | "none" | string;
  serviceOwner: "tauri" | "python" | "none" | string;
  firewallOwner: "tauri" | "python" | "none" | string;
  shimOwner: "tauri" | "python" | "none" | string;
}

export interface AppState {
  platform: PlatformInfo;
  warp: WarpState;
  services: ServicesState;
  lane: LaneState;
  stats: StatsState;
  logs: LogsState;
  egress: EgressState;
  proxy: ProxyState;
  autoRepair: RepairSnapshot;
  health: HealthSummary;
  probe: ProbeState;
  settings: AppSettings;
  management: ManagementStatus;
}

export interface StatusResponse {
  ok: boolean;
  message: string;
}

export interface EndpointLatency {
  endpoint: string;
  latency: number | null;
}

export interface LogsResponse {
  kind: string;
  lines: string[];
}

export const defaultSettings: AppSettings = {
  autoResume: true,
  strictMode: false,
  autoRepair: true,
  autostart: false,
  theme: "system",
};
