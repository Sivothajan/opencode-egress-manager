# Build and Release

## Commands

```bash
bun install
bun run tauri dev
bun run build:release   # installer + SHA256SUMS.txt
```

Other checks:

```bash
bun run typecheck
bun run test
bun run format:check
cargo test --lib --manifest-path src-tauri/Cargo.toml
```

Lockfiles `bun.lock` and `src-tauri/Cargo.lock` are committed.

The admin helper is a Tauri `externalBin`, so its target file must exist when the build
script runs. `scripts/stage-admin-sidecar.ps1` creates a placeholder on a clean checkout and
`build-release.ps1` overwrites it with the real PE and verifies the header before bundling.

## Version

`src-tauri/Cargo.toml` is the single source of truth. Tauri derives its bundle version
from `CARGO_PKG_VERSION`, and the UI reports the same value at runtime. Do not add
duplicate version fields elsewhere.

Current prerelease candidate: **0.3.1** (`v0.3.1`). The published `v0.3.0`
prerelease and its assets remain unchanged.

## Changes since v0.3.0

- Fix auto-resume command construction (the CLI path was dropped, so resume tried to
  spawn the literal argument `run`).
- Stop replaying truncated or backlogged OpenCode logs as fresh quota walls.
- Refactor `build_state` (single fact snapshot + named DTO builders) and
  `build_health` (phase checks + summary), preserving messages, severity, and order.
- Enable `#![deny(missing_docs)]` and document the public Rust API.
- Harden SOCKS5 domain length handling, Win32 string-length handling, process exit-code
  conversion, and cache-expiry `Instant` math.
- Add the portable ZIP distribution and document installer/portable coexistence.

## Signing

Set one pair before building:

```powershell
$env:OUC_SIGN_PFX = "C:\certs\my.pfx"
$env:OUC_SIGN_PASSWORD = "..."
# or
$env:OUC_SIGN_THUMBPRINT = "ABC123..."
```

`build-release.ps1` signs `target/release/opencode-egress-manager.exe` and the NSIS installer
when a certificate is present. Without one it builds unsigned.

## Distribution

### Installer (primary)

NSIS (`installMode: both`, Administrator required). Embeds the Evergreen WebView2
bootstrapper, stages the admin helper next to the app, registers scheduled tasks,
updates `PATH`, and installs/removes the firewall kill-switch on request. MSIX would
add capability declarations and store constraints without helping this app, since it
manages network routes, firewall rules, and an elevated helper.

### Portable ZIP (no installer)

A portable package contains only:

```
opencode-egress-manager.exe   main application (frontend embedded)
egress-manager-admin.exe      privileged firewall helper (keep beside the app)
README-PORTABLE.txt           usage and coexistence notes
```

It runs from any folder and does not require the repository, `dist`, or a dev server.
The helper is located relative to the running executable (same folder or `bin/`), so
the two executables must stay together.

Runtime requirements and limits:

- Windows 10 build 19041+ / Windows 11, **x64 only**.
- **WebView2 Evergreen Runtime**. The installer can bootstrap it; the portable ZIP does
  not install anything, so WebView2 must already be present (Windows 11 ships it).
- Cloudflare WARP only for WARP/clean modes; Tor mode downloads a portable bundle into
  the app data directory on first use.
- No VC++ redistributable needed (CRT is statically linked).
- Data is shared with an installed copy: `%LOCALAPPDATA%\opencode-egress-manager`.
- One UI instance at a time (single-instance mutex). Keeper/guard worker mutexes still
  prevent duplicate background workers.
- Do not enable logon tasks or install the firewall rule from a portable copy while an
  installed copy exists; the scheduled tasks would point at different paths.
- Privileged actions (firewall rule, scheduled tasks) still trigger UAC.
- Binaries are unsigned unless built with a certificate; SmartScreen may warn.

Conventional name: `Opencode.Egress.Manager_<version>_x64-portable.zip`.

> Versioning note: a portable build made after the published `v0.3.0` prerelease still
> reports `0.3.0` in its metadata but is not byte-identical to the released installer.
> Do not publish it under the existing v0.3.0 assets; cut a new version (recommend
> `v0.3.1`).

## Coexistence with Opencode Unlimited (Python)

The legacy Python app (`%LOCALAPPDATA%\opencode-unlimited`) and this app can be installed
side by side. This app never modifies Python-owned processes or files.

| Resource           | Python                              | Tauri                                    |
| :----------------- | :---------------------------------- | :--------------------------------------- |
| Data root          | `%LOCALAPPDATA%\opencode-unlimited` | `%LOCALAPPDATA%\opencode-egress-manager` |
| Service proxy port | 41987                               | 41988                                    |
| Firewall rule      | `OUC_LEAKGUARD_CLI`                 | `OUC_LEAKGUARD_CLI_EGRESS`               |
| Scheduled tasks    | `OpencodeUnlimited-Keeper`/`-Guard` | `OpencodeEgressManager-Keeper`/`-Guard`  |
| CLI shim on PATH   | first when present                  | only when Python's bin is absent         |
| `service.json`     | owned when installed                | untouched while Python is present        |
| WARP / Tor         | Python keeper while it runs         | Tauri keeper otherwise                   |

Tauri's WARP/Tor commands refuse with an explanatory message while the Python keeper runs,
and Tauri's guard stands down while the Python guard runs. Ownership is reported through
`AppState.management` and shown as a banner.

## Known limitations

- Tor ports (9050/9051/9150) and WARP's isolated SOCKS port (40000) are shared with the
  Python app; do not run Tor or isolated-WARP mode in both at the same time.
- Only one quota guard may rotate at a time; Tauri yields to the Python guard.
- Ownership is detected from named mutexes at operation time. A keeper that starts in the
  few milliseconds between the check and the operation is not guarded against.
- Installer/uninstaller lifecycle (fresh install, upgrade, uninstall, reinstall) must be
  verified on an isolated VM; it was not run against the active installation. Use
  [docs/WINDOWS_VM_TEST_PLAN.md](WINDOWS_VM_TEST_PLAN.md).
- Portable execution is verified for launch-from-arbitrary-directory and single-instance
  refusal only. Settings persistence and full click-through in the portable binary still
  require an environment where the installed instance is stopped.
- Two Tauri copies share one data directory by design. The single-instance mutex
  (`Local\OpencodeEgressManagerUI`) has existed since the initial commit, so any two
  builds (including the published v0.3.0 and a portable copy) cannot run concurrently;
  a second launch focuses the first window and exits.
- A portable UI can still issue manual WARP/Tor actions while another instance's
  background keeper owns the tunnel; prefer using the instance that owns the workers.

## Verification status (v0.3.1 candidate, 2026-11-20)

Executed non-destructively:

- `cargo check`; `cargo test` (57/57); strict Clippy `--all-targets --all-features --locked
-D warnings`; `cargo fmt --check`.
- `bun run typecheck`; `bun run test` (20/20); `bun run build`; `bun run format:check`.
- `bun run build:release` produced the v0.3.1 installer, portable ZIP, and checksums.

| Artifact                                             | Size (bytes) | SHA256                                                             |
| :--------------------------------------------------- | -----------: | :----------------------------------------------------------------- |
| `Opencode Egress Manager_0.3.1_x64-setup.exe` (NSIS) |    6,535,148 | `a3bd16e47747e1b98fd3b57113b3c36c8c74854bf75efe89971ef42ed1365b11` |
| `opencode-egress-manager.exe`                        |   13,953,024 | `f77c2f32cf8381566789b3e15c60965b24190b904eb2b8842fc59184fb4bfdf6` |
| `egress-manager-admin.exe`                           |    1,787,904 | `a7e41874f5fa821aee7ddb6de391fe5402c871bf1b67ed13c17933df5f000cd4` |
| `Opencode.Egress.Manager_0.3.1_x64-portable.zip`     |    6,138,038 | `4bd93ab534f32aabd48d70330b2442820e6c98788b77e6a646b5d959b1ae7272` |

- Installer payload extracted and checked: app metadata **0.3.1**, valid PE admin helper,
  uninstaller, embedded WebView2 bootstrapper. Installer hash equals `SHA256SUMS.txt`.
- Portable ZIP extracted to an isolated directory; extracted hashes match; launching it
  while the single-instance mutex is held exits immediately with code 0 (conflict
  refusal) instead of starting a competing instance.
- Frontend served from the production `dist`: all eight routes render with correct
  headings, eight nav entries, and no horizontal overflow. The only console error is
  preview-only (`@tauri-apps/api` event `listen()` requires Tauri internals) plus CSP
  meta-tag notices that do not apply when Tauri delivers CSP as a header.

Still pending / not executed:

- NSIS fresh install / upgrade / uninstall / reinstall / failed-install recovery — needs
  an isolated Windows VM (none available on this host; see
  [docs/WINDOWS_VM_TEST_PLAN.md](WINDOWS_VM_TEST_PLAN.md)).
- Production-UI settings persistence across restart, dialog/tooltip interaction in the
  real WebView2 window, and safe worker shutdown — blocked because the installed
  instance is running and must not be disturbed.
- Code signing (no certificate configured).
- Optional Clippy pedantic advisories remain (see `cargo clippy -- -W clippy::pedantic`);
  four confirmed risky casts/duration issues were fixed, the rest are documented as
  low-value or FFI-inherent.

## GitHub Actions

| Workflow                        | Trigger                    | Publishes                |
| :------------------------------ | :------------------------- | :----------------------- |
| `.github/workflows/ci.yml`      | push to main, PRs          | No, checks only          |
| `.github/workflows/release.yml` | manual `workflow_dispatch` | Yes, artifacts + release |

The release workflow takes an optional version tag and git ref. It sets up Bun and Rust,
installs dependencies with `--frozen-lockfile`, prepares the optional certificate from
secrets, runs `bun run build:release`, uploads artifacts, and creates the GitHub Release.

Secrets:

```text
OUC_CERT_BASE64
OUC_CERT_PASSWORD
OUC_SIGN_THUMBPRINT
```

Pinned actions: `actions/checkout@v7.0.1`, `oven-sh/setup-bun@v2.2.0`,
`dtolnay/rust-toolchain@v1`, `actions/upload-artifact@v7.0.2`,
`softprops/action-gh-release@v3.0.4`.

## Verify

1. Run `bun run build:release`.
2. Compare the installer hash with `SHA256SUMS.txt`.
3. Install on a clean, isolated Windows 10/11 VM (never on the machine that runs the Python app).
4. Confirm the dashboard reaches healthy with WARP and OpenCode installed.
5. Confirm `egress-manager-admin.exe` sits next to the installed executable.
6. Uninstall and check that only Tauri-owned tasks, PATH entries, shims, the
   `OUC_LEAKGUARD_CLI_EGRESS` rule, and Tauri's data directory are removed.
7. Reinstall and confirm the Python app's keeper/guard, `service.json`, and
   `OUC_LEAKGUARD_CLI` rule are untouched.

The v0.3.1 candidate is **unsigned** (no `OUC_SIGN_PFX` / `OUC_SIGN_THUMBPRINT`
configured). The published v0.3.0 prerelease and its assets were not modified.
