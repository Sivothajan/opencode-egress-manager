import { useState } from "react";
import { useTheme } from "next-themes";
import { ExternalLink, FolderOpen, Trash2, Wrench } from "lucide-react";
import {
  Button,
  Card,
  Modal,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  TipButton,
  ToggleRow,
} from "@/components/ui";
import { PageContent } from "@/components/PageContent";
import { useStateAction, useStateQuery } from "@/hooks/useAppState";
import {
  installOrRepair,
  openExternalUrl,
  openLogsFolder,
  saveSettings,
  toggleAutoRepair,
  toggleAutoResume,
  toggleAutostart,
  toggleFirewall,
  toggleStrictMode,
  uninstallAll,
} from "@/services/api";
import { settingsPatchSchema, validate } from "@/lib/validation";

export default function SettingsPage() {
  const query = useStateQuery();
  const action = useStateAction();
  const { setTheme } = useTheme();
  const [confirmUninstall, setConfirmUninstall] = useState(false);
  const [purgeState, setPurgeState] = useState(true);

  return (
    <PageContent query={query}>
      {(state) => (
        <>
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <Card title="Appearance">
              <p className="mb-2 text-xs" style={{ color: "var(--muted-text)" }}>
                The window titlebar follows the selected theme on Windows 10/11.
              </p>
              <Select
                value={state.settings.theme}
                onValueChange={(value) => {
                  if (!value) return;
                  const result = validate(settingsPatchSchema, { theme: value });
                  if (!result.success) return;
                  setTheme(value);
                  action.mutate(() =>
                    saveSettings({ theme: value as "system" | "light" | "dark" }),
                  );
                }}
              >
                <SelectTrigger className="w-56">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="system">System</SelectItem>
                  <SelectItem value="light">Light</SelectItem>
                  <SelectItem value="dark">Dark</SelectItem>
                </SelectContent>
              </Select>
            </Card>

            <Card title="Protection behavior">
              <div className="divide-y" style={{ borderColor: "var(--border)" }}>
                <ToggleRow
                  title="Auto-resume after rotation"
                  description="Send 'continue' to chats that hit the limit."
                  checked={state.settings.autoResume}
                  disabled={action.isPending}
                  onChange={(value) => action.mutate(() => toggleAutoResume(value))}
                  tipTitle="Auto-Resume on Bypass"
                  tipBody="After a successful rotation, reloads the OpenCode window and sends a 'continue' prompt through the CLI or keystrokes."
                />
                <ToggleRow
                  title="Strict fail-closed mode"
                  description="Block requests instead of falling back to a direct route."
                  checked={state.settings.strictMode}
                  disabled={action.isPending}
                  onChange={(value) => action.mutate(() => toggleStrictMode(value))}
                  tipTitle="Strict Fail-Closed Mode"
                  tipBody="When ON, the proxy refuses connections if WARP is down. When OFF (Recommended), it falls back to a direct connection instead of returning a 503 error."
                />
                <ToggleRow
                  title="Auto-repair"
                  description="Safely restore a dropped proxy or WARP route."
                  checked={state.autoRepair.enabled}
                  disabled={action.isPending}
                  onChange={(value) => action.mutate(() => toggleAutoRepair(value))}
                  tipTitle="Automatic Route Repair"
                  tipBody="Watches the proxy and tunnel. If OpenCode loses its route, it restarts the proxy, reconnects WARP, or changes edge automatically. It never changes your selected mode, clears chats, or restarts OpenCode."
                />
                <ToggleRow
                  title="Logon auto-start"
                  description="Register Keeper and Guard scheduled tasks."
                  checked={state.services.autostartRegistered}
                  disabled={action.isPending}
                  onChange={(value) => action.mutate(() => toggleAutostart(value))}
                  tipTitle="Auto-Start at Windows Login"
                  tipBody="Registers OpencodeEgressManager-Keeper and OpencodeEgressManager-Guard in Windows Task Scheduler (AtLogOn) so protection starts silently on every reboot."
                />
                <ToggleRow
                  title="Firewall kill-switch"
                  description="Requires a UAC prompt; blocks direct non-loopback egress for opencode-cli."
                  checked={state.services.firewallInstalled}
                  disabled={action.isPending || !state.lane.cliPath}
                  onChange={(value) => action.mutate(() => toggleFirewall(value))}
                  tipTitle="Windows Firewall Kill-Switch"
                  tipBody="Blocks opencode-cli.exe from making direct outbound internet connections, forcing all of its traffic through the localhost proxy and WARP. Prompts for UAC approval when toggled."
                />
              </div>
            </Card>

            <Card title="Maintenance">
              <div className="flex flex-wrap gap-2">
                <TipButton
                  disabled={action.isPending}
                  onClick={() => action.mutate(installOrRepair)}
                  tipTitle="Install / Repair Everything"
                  tipBody="Deploys shims to %LOCALAPPDATA%\opencode-egress-manager, updates User PATH, registers logon tasks, connects WARP in the selected scope, and wires OpenCode Desktop."
                >
                  <Wrench size={14} />
                  Install / Repair
                </TipButton>
                <TipButton
                  variant="ghost"
                  onClick={() => openLogsFolder()}
                  tipTitle="Open Data & Logs Folder"
                  tipBody="Opens %LOCALAPPDATA%\opencode-egress-manager in File Explorer so you can inspect logs and state directly."
                >
                  <FolderOpen size={14} />
                  Logs folder
                </TipButton>
                <TipButton
                  variant="danger"
                  onClick={() => setConfirmUninstall(true)}
                  tipTitle="Uninstall Cleanly"
                  tipBody="Stops Keeper & Guard, removes Scheduled Tasks, removes the firewall rule, restores OpenCode Desktop service to direct egress, cleans PATH, and deletes %LOCALAPPDATA%\opencode-egress-manager. Your OpenCode projects & chats are untouched."
                >
                  <Trash2 size={14} />
                  Uninstall
                </TipButton>
              </div>
              <p className="mt-3 text-xs" style={{ color: "var(--muted-text)" }}>
                Install creates shims and scheduled tasks, wires OpenCode's background service, and
                connects WARP in the selected scope. Uninstall reverts all of it without touching
                OpenCode itself.
              </p>
            </Card>

            <Card title="About">
              <div className="flex flex-col gap-1 text-xs" style={{ color: "var(--muted-text)" }}>
                <p>Version v{state.platform.appVersion}</p>
                <p>
                  Windows {state.platform.windowsVersion} · {state.platform.machineName}
                </p>
                <p>Tauri 2 + React + Rust. No Python runtime ships with this application.</p>
              </div>
              <div className="mt-3">
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    openExternalUrl("https://github.com/Sivothajan/opencode-egress-manager")
                  }
                >
                  <ExternalLink size={13} />
                  Project repository
                </Button>
              </div>
            </Card>
          </div>

          <Modal
            open={confirmUninstall}
            onClose={() => setConfirmUninstall(false)}
            title="Uninstall Opencode Egress Manager?"
            description="This removes scheduled tasks, PATH entries, the CLI shim, OpenCode proxy wiring, and stored credentials. OpenCode, WARP, Tor, and your chats are left installed."
            footer={
              <>
                <Button variant="outline" onClick={() => setConfirmUninstall(false)}>
                  Cancel
                </Button>
                <Button
                  variant="destructive"
                  disabled={action.isPending}
                  onClick={() => {
                    setConfirmUninstall(false);
                    action.mutate(() => uninstallAll(purgeState));
                  }}
                >
                  Uninstall
                </Button>
              </>
            }
          >
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={purgeState}
                onChange={(event) => setPurgeState(event.target.checked)}
              />
              Also delete local state and logs
            </label>
          </Modal>
        </>
      )}
    </PageContent>
  );
}
