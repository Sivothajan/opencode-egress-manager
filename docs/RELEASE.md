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

Current prerelease: **0.3.0** (`v0.3.0`).

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

NSIS is the primary target. MSIX would add capability declarations and store constraints
without helping this app, since it manages network routes, firewall rules, and an elevated
helper. Revisit if store distribution is required.

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

The 0.3.0 build is **unsigned** (no `OUC_SIGN_PFX` / `OUC_SIGN_THUMBPRINT` configured).
