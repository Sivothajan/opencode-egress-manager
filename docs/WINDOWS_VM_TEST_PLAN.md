# Windows VM test plan — isolated installer lifecycle

Use this plan on a **throwaway virtual machine only**. Never run it on a machine that
hosts the Python reference app or the user's data. The VM must be reverted to a clean
snapshot after the run.

## 0. Scope

Verifies the NSIS installer/uninstaller lifecycle for the Tauri build and proves the
Python reference app and its resources are untouched.

Out of scope: live quota-wall bypass, identity reset, WARP/Tor traffic, and any
operation that changes the host network.

## 1. Prerequisites

- Windows 10 build 19041+ or Windows 11 x64 VM (Generation 2, Secure Boot off or
  Microsoft-signed DB), 4 GB RAM, 40 GB disk.
- Hyper-V, VirtualBox, or VMware Workstation/Player.
- Network access in the VM **or** a pre-installed WebView2 Runtime (the installer embeds
  the Evergreen bootstrapper, which needs network on first run).
- Copy in:
  - `src-tauri/target/release/bundle/nsis/Opencode Egress Manager_0.3.0_x64-setup.exe`
  - `SHA256SUMS.txt`
  - (for the upgrade test) the previous installer, e.g. `..._0.2.1_x64-setup.exe`.

## 2. Baseline snapshot (before anything)

Run inside the VM and save the output:

```powershell
$out = "$env:USERPROFILE\Desktop\baseline.txt"
"=== installer dir ==="        | Set-Content $out
Get-ChildItem "$env:ProgramFiles\Opencode Egress Manager" -ErrorAction SilentlyContinue | Select-Object Name,Length | Add-Content $out
"=== local app data (Tauri) ===" | Add-Content $out
Get-ChildItem "$env:LOCALAPPDATA\opencode-egress-manager" -Recurse -ErrorAction SilentlyContinue | Select-Object FullName,Length | Add-Content $out
"=== python data dir ==="      | Add-Content $out
Get-ChildItem "$env:LOCALAPPDATA\opencode-unlimited" -Recurse -ErrorAction SilentlyContinue | Select-Object FullName,Length | Add-Content $out
"=== service.json ==="         | Add-Content $out
Get-Content "$env:USERPROFILE\.config\opencode\service.json" -Raw -ErrorAction SilentlyContinue | Add-Content $out
"=== firewall rules ==="       | Add-Content $out
netsh advfirewall firewall show rule name=all | Select-String 'OUC_LEAKGUARD' | Add-Content $out
"=== scheduled tasks ==="      | Add-Content $out
schtasks /Query /FO CSV /NH | Select-String 'Opencode' | Add-Content $out
"=== user PATH ==="            | Add-Content $out
[Environment]::GetEnvironmentVariable('Path','User') | Add-Content $out
"=== processes ==="            | Add-Content $out
Get-Process | Where-Object { $_.ProcessName -match 'opencode|pythonw|egress' } | Select-Object Id,ProcessName,Path | Add-Content $out
```

Record the baseline hash of the Python resources:

```powershell
Get-FileHash "$env:LOCALAPPDATA\opencode-unlimited\scripts\*.py" -Algorithm SHA256 | Out-File "$env:USERPROFILE\Desktop\python-hashes-before.txt"
```

## 3. Test cases

### TC-1 Fresh install

The installer is NSIS. Supported switches: `/S` (silent), `/P` (passive), `/R`
(restart app), `/NS` (no shortcuts in silent/passive), `/D=<path>` (install dir).
`installMode` is `both`, so **Administrator (UAC) is required** even for a
current-user install.

```powershell
cd $env:USERPROFILE\Desktop
Get-FileHash ".\Opencode Egress Manager_0.3.0_x64-setup.exe" -Algorithm SHA256
# compare with SHA256SUMS.txt
.\Opencode Egress Manager_0.3.0_x64-setup.exe /S
```

Expected:

- Exit code 0; installer disappears; no error dialog.
- `%PROGRAMFILES%\Opencode Egress Manager\opencode-egress-manager.exe` exists (per-machine)
  or `%LOCALAPPDATA%\Programs\Opencode Egress Manager\...` (per-user). Record which.
- `egress-manager-admin.exe` sits next to the app exe.
- `uninstall.exe` exists.
- Version metadata: `(Get-Item <app exe>).VersionInfo.FileVersion -eq "0.3.0"`.

### TC-2 First launch + production UI smoke test

```powershell
Start-Process "<install path>\opencode-egress-manager.exe"
Start-Sleep -Seconds 8
Get-Process opencode-egress-manager | Select-Object Id,ProcessName
```

Expected: process alive; window title "Opencode Egress Manager"; no console window.
Manual click-through (record a short video/screenshots):

- Navigate all 8 routes (Dashboard, Network, Proxy, WARP, Tor, OpenCode, Diagnostics,
  Settings); no blank page, no horizontal scrollbar, no window-level scroll.
- Hover the info icons; tooltips appear with the reference descriptions and match the
  theme (light tooltip in light mode, dark in dark mode).
- Confirm the management banner appears when the Python keeper is running, and that
  WARP/Tor buttons are disabled with that reason.
- Confirm the window fits the viewport and the shell/header/nav/sidebar stay fixed while
  only the content panel scrolls.

### TC-3 Settings persistence across restart

```powershell
# toggle "Logon auto-start" or the theme in the UI, then:
Get-Content "$env:LOCALAPPDATA\opencode-egress-manager\settings.json"
Stop-Process -Name opencode-egress-manager -Force
Start-Process "<install path>\opencode-egress-manager.exe"
Start-Sleep -Seconds 8
# re-open Settings and confirm the value persisted
```

Expected: `settings.json` keeps the value; the UI shows it after restart.

### TC-4 Upgrade installation

```powershell
# with 0.3.0 installed and running once:
.\Opencode Egress Manager_0.3.0_x64-setup.exe /S   # same version = repair/upgrade path
# or install the older build first, then the new one
```

Expected: installer completes; app exe `FileVersion` is the new build; the data directory
(`settings.json`, `lane\`, `guard\`, `keeper\`) is preserved, not reset.

### TC-5 Uninstall

```powershell
& "<install path>\uninstall.exe" /S
Start-Sleep -Seconds 5
```

If the per-machine install requires elevation, run the uninstaller from an
elevated prompt; the silent switch is the same `/S`.

Expected:

- App exe, `egress-manager-admin.exe`, and `uninstall.exe` are gone from the install dir.
- `%LOCALAPPDATA%\opencode-egress-manager` is removed.
- Scheduled tasks `OpencodeEgressManager-Keeper`/`-Guard` are gone.
- The user PATH no longer contains `%LOCALAPPDATA%\opencode-egress-manager\bin`.
- Firewall rule `OUC_LEAKGUARD_CLI_EGRESS` is gone.
- **Python-owned resources remain** (see TC-7).

### TC-6 Reinstall

Repeat TC-1 after TC-5. Expected: clean install succeeds; no leftover state causes a
failure; the app launches.

### TC-7 Python resource integrity (after TC-5/TC-6)

```powershell
Get-FileHash "$env:LOCALAPPDATA\opencode-unlimited\scripts\*.py" -Algorithm SHA256 | Out-File "$env:USERPROFILE\Desktop\python-hashes-after.txt"
Compare-Object (Get-Content "$env:USERPROFILE\Desktop\python-hashes-before.txt") (Get-Content "$env:USERPROFILE\Desktop\python-hashes-after.txt")
Get-Process pythonw -ErrorAction SilentlyContinue | Select-Object Id,ProcessName
netsh advfirewall firewall show rule name=all | Select-String 'OUC_LEAKGUARD_CLI'
Get-Content "$env:USERPROFILE\.config\opencode\service.json" -Raw
```

Expected:

- Python script hashes unchanged.
- Python keeper/guard processes still running.
- `OUC_LEAKGUARD_CLI` still present; `OUC_LEAKGUARD_CLI_EGRESS` absent.
- `service.json` still points at `127.0.0.1:41987`.

## 4. Failure-recovery test (where practical)

- Kill the installer mid-run (or disconnect the VM network during the WebView2 step) and
  confirm the VM can still install cleanly afterwards.
- Run the installer twice without uninstalling and confirm it does not corrupt state.

## 5. Cleanup

Revert the VM to the pre-test snapshot, or delete the VM. Do not keep a VM that holds a
copy of the installer and user data.

## 6. Signing note

The distributed binaries are **unsigned** unless `OUC_SIGN_PFX`/`OUC_SIGN_THUMBPRINT`
was set before `bun run build:release`. Windows SmartScreen will warn on first run of an
unsigned executable; record this in the release notes.
