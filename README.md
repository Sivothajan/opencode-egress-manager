# Opencode Egress Manager

Windows desktop app that routes OpenCode traffic through Cloudflare WARP IPv6 `/64`, Tor,
or a custom proxy pool. It rotates the egress when OpenCode hits a free-tier limit and clears
the client-side lockout.

Stack: Tauri 2, React, TypeScript, Rust. No Python runtime is bundled.

## Requirements

- Windows 10 (build 19041+) or Windows 11, x64
- WebView2 Runtime (the installer can embed the bootstrapper)
- Cloudflare WARP for WARP modes
- OpenCode Desktop or CLI

## Develop

```bash
bun install
bun run setup        # clean checkout only: stages the admin helper path for cargo
bun run tauri dev
bun run typecheck
bun run test
bun run format:check
```

On a clean clone, run `bun run setup` before any cargo command. Tauri validates its
externalBin resource while its build script runs, before that binary can exist. CI and
`bun run build:release` do this automatically.

## Build

```powershell
bun run build:release
```

This produces the NSIS installer and `SHA256SUMS.txt`. To sign, set `OUC_SIGN_PFX` and
`OUC_SIGN_PASSWORD`, or `OUC_SIGN_THUMBPRINT`, before running it.

## Layout

```text
src/           React app (features, components, services, hooks)
src-tauri/     Rust app, services, worker roles, admin helper
scripts/       release and icon generation
docs/          architecture, security, release notes
```

## CI and releases

- `.github/workflows/ci.yml` runs checks only. It does not publish anything.
- `.github/workflows/release.yml` is manual (`workflow_dispatch`) and creates the release.

See [docs/RELEASE.md](docs/RELEASE.md) for signing inputs and the release workflow.

## Credits

Original idea and Python/PyWebView implementation:
[ShadowySphinX/Opencode-Unlimited-Limit-Bypass-](https://github.com/ShadowySphinX/Opencode-Unlimited-Limit-Bypass-).
This project reuses its routing concepts and egress behavior in a Windows-first Tauri/Rust
rewrite.

## License

[MIT](LICENSE).
