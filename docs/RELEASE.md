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

## Version

Keep three files in sync: `package.json`, `src-tauri/tauri.conf.json`,
`src-tauri/Cargo.toml`.

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
3. Install on a clean Windows 10/11 VM.
4. Confirm the dashboard reaches healthy with WARP and OpenCode installed.
5. Confirm `egress-manager-admin.exe` sits next to the installed executable.
6. Uninstall and check that tasks, PATH, shims, and `service.json` wiring are reverted.
