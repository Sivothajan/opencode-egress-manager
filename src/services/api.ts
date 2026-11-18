import { cmd } from "@/lib/tauri";
import type {
  AppSettings,
  AppState,
  EndpointLatency,
  LogsResponse,
  PlatformInfo,
  ProbeState,
  StatusResponse,
} from "@/types";

/** Typed wrappers around the Tauri command surface. */

export const getPlatformInfo = () => cmd<PlatformInfo>("get_platform_info");

export interface NetworkInterfaceInfo {
  name: string;
  description: string;
  mac: string;
  ipv4: string[];
  ipv6: string[];
  isUp: boolean;
  isLoopback: boolean;
}

export const getInterfaces = () => cmd<NetworkInterfaceInfo[]>("get_interfaces");

export const getState = () => cmd<AppState>("get_state");

export const getProbe = () => cmd<ProbeState>("refresh_probe");

export const setEgressMode = (mode: string, customProxy?: string, customType?: string) =>
  cmd<StatusResponse>("set_egress_mode", {
    mode,
    custom_proxy: customProxy,
    custom_type: customType,
  });

export const setWarpScope = (scope: string) => cmd<StatusResponse>("set_warp_scope", { scope });

export const warpConnect = () => cmd<StatusResponse>("warp_connect");
export const warpDisconnect = () => cmd<StatusResponse>("warp_disconnect");
export const hopNow = () => cmd<StatusResponse>("hop_now");
export const repairWarp = () => cmd<StatusResponse>("repair_warp_connection");
export const autoHealEndpoint = () => cmd<StatusResponse>("auto_heal_endpoint");

export const setCleanEndpoint = (endpoint: string) =>
  cmd<StatusResponse>("set_clean_endpoint", { endpoint });
export const resetCleanEndpoint = () => cmd<StatusResponse>("reset_clean_endpoint");
export const testCleanEndpoints = (timeoutMs = 900) =>
  cmd<EndpointLatency[]>("test_clean_endpoints", { timeout_ms: timeoutMs });

export const toggleTorService = (enable: boolean) =>
  cmd<StatusResponse>("toggle_tor_service", { enable });
export const rotateTorCircuit = () => cmd<StatusResponse>("rotate_tor_circuit");
export const rotateCustomProxy = () => cmd<StatusResponse>("rotate_custom_proxy");

export const toggleWorker = (name: string, enable: boolean) =>
  cmd<StatusResponse>("toggle_worker", { name, enable });
export const restartWorkers = () => cmd<StatusResponse>("restart_workers");
export const toggleAutostart = (enable: boolean) =>
  cmd<StatusResponse>("toggle_autostart", { enable });
export const toggleDesktopServiceProxy = (enable: boolean) =>
  cmd<StatusResponse>("toggle_desktop_service_proxy", { enable });
export const toggleFirewall = (enable: boolean) =>
  cmd<StatusResponse>("toggle_firewall", { enable });

export const toggleAutoResume = (enable: boolean) =>
  cmd<StatusResponse>("toggle_auto_resume", { enable });
export const toggleStrictMode = (enable: boolean) =>
  cmd<StatusResponse>("toggle_strict_mode", { enable });
export const toggleAutoRepair = (enable: boolean) =>
  cmd<StatusResponse>("toggle_auto_repair", { enable });

export const rerollEdge = () => cmd<StatusResponse>("reroll_edge");
export const setEdgeOverride = (edge: string) => cmd<StatusResponse>("set_edge_override", { edge });
export const refreshEdgePool = () => cmd<StatusResponse>("refresh_edge_pool");
export const clearBlacklist = () => cmd<StatusResponse>("clear_blacklist");
export const deepResetIdentity = () => cmd<StatusResponse>("deep_reset_identity");

export const fixIssue = (action: string) => cmd<StatusResponse>("fix_issue", { action });
export const fixEverything = () => cmd<StatusResponse>("fix_everything");

export const launchDesktop = () => cmd<StatusResponse>("launch_desktop");
export const launchCli = () => cmd<StatusResponse>("launch_cli");
export const fixOpencodeService = () => cmd<StatusResponse>("fix_opencode_service");
export const installOrRepair = () => cmd<StatusResponse>("install_or_repair");
export const uninstallAll = (purgeState: boolean) =>
  cmd<StatusResponse>("uninstall_all", { purge_state: purgeState });

export const readLogs = (kind: string) => cmd<LogsResponse>("read_logs", { kind });

export const openLogsFolder = () => cmd<StatusResponse>("open_logs_folder");
export const openExternalUrl = (url: string) => cmd<StatusResponse>("open_external_url", { url });

export const getSettings = () => cmd<AppSettings>("get_settings");
export const saveSettings = (patch: Partial<AppSettings>) =>
  cmd<AppSettings>("save_settings", {
    auto_resume: patch.autoResume,
    strict_mode: patch.strictMode,
    auto_repair: patch.autoRepair,
    autostart: patch.autostart,
    theme: patch.theme,
  });
