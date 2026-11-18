# Security Notes

Security design notes for the Tauri implementation.

## Boundary

- Capability file grants core event and window permissions only.
- No fs, shell, http, or dialog plugin access from the webview.
- CSP set in `index.html` and `tauri.conf.json`. TLS verification is never disabled.

## Input handling

- Process launches use argument vectors. No shell interpolation.
- `netsh`, `schtasks`, and `warp-cli` use fixed flags plus validated values.
- `validate_cli_path` rejects traversal, relative paths, metacharacters, non-`.exe` files,
  and paths outside allowlisted OpenCode roots.
- Proxy credentials are stripped before persistence; parsed with a strict parser.
- Edge overrides accept `auto` or a parsed IPv6 literal.
- Frontend validates with Zod before calling the backend.

## Privileged path

One operation needs admin rights: the firewall rule. The UI launches
`egress-manager-admin.exe` through `ShellExecuteW("runas")`. It accepts only
`firewall-status`, `firewall-install <validated-path>`, and `firewall-remove`.

## Secrets

Proxy credentials are encrypted with DPAPI in `lane/credentials.json`. Logs redact URL
userinfo, proxy authorization headers, bearer tokens, and password assignments.

## Downloads

Tor Expert Bundle downloads use HTTPS with rustls. The digest comes from the official
`sha256sums.txt` for that exact filename, and every mirror must match it. Tar entries are
checked for path traversal.

## Config and destructive actions

JSON and text writes are atomic with a `.bak` snapshot. `service.json` is merged, not
rewritten, and snapshotted before replacement. Uninstall, firewall changes, deep reset, and
service unwiring require UI confirmation.

## Tests

Rust tests cover config rollback, guard policy, proxy parsing and sanitization, firewall
path rejection, scheduler XML escaping, secret redaction, and public-IP validation.
Frontend tests cover Zod schemas including injection payloads.
