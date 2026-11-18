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
                />
                <ToggleRow
                  title="Strict fail-closed mode"
                  description="Block requests instead of falling back to a direct route."
                  checked={state.settings.strictMode}
                  disabled={action.isPending}
                  onChange={(value) => action.mutate(() => toggleStrictMode(value))}
                />
                <ToggleRow
                  title="Auto-repair"
                  description="Safely restore a dropped proxy or WARP route."
                  checked={state.autoRepair.enabled}
                  disabled={action.isPending}
                  onChange={(value) => action.mutate(() => toggleAutoRepair(value))}
                />
                <ToggleRow
                  title="Logon auto-start"
                  description="Register Keeper and Guard scheduled tasks."
                  checked={state.services.autostartRegistered}
                  disabled={action.isPending}
                  onChange={(value) => action.mutate(() => toggleAutostart(value))}
                />
                <ToggleRow
                  title="Firewall kill-switch"
                  description="Requires a UAC prompt; blocks direct non-loopback egress for opencode-cli."
                  checked={state.services.firewallInstalled}
                  disabled={action.isPending || !state.lane.cliPath}
                  onChange={(value) => action.mutate(() => toggleFirewall(value))}
                />
              </div>
            </Card>

            <Card title="Maintenance">
              <div className="flex flex-wrap gap-2">
                <Button disabled={action.isPending} onClick={() => action.mutate(installOrRepair)}>
                  <Wrench size={14} />
                  Install / Repair
                </Button>
                <Button variant="ghost" onClick={() => openLogsFolder()}>
                  <FolderOpen size={14} />
                  Logs folder
                </Button>
                <Button variant="danger" onClick={() => setConfirmUninstall(true)}>
                  <Trash2 size={14} />
                  Uninstall
                </Button>
              </div>
              <p className="mt-3 text-xs" style={{ color: "var(--muted-text)" }}>
                Install creates shims and scheduled tasks, wires OpenCode's background service, and
                connects WARP in the selected scope. Uninstall reverts all of it without touching
                OpenCode itself.
              </p>
            </Card>

            <Card title="About">
              <div className="space-y-1 text-xs" style={{ color: "var(--muted-text)" }}>
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
