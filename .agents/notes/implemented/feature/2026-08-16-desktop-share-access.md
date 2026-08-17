# Agent Note: Desktop share sheet joins other browsers to the live instance

Status: implemented

English | [中文](2026-08-16-desktop-share-access.zh.md)

## Problem

The packaged desktop application already runs the official Web GUI in a WebView, but that GUI cannot be opened from the same machine's browser or from another device. The sidecar binds loopback only, `apps/desktop/src-tauri/src/overlay.rs` refuses `0.0.0.0` and `--trusted-host`, and the per-launch bootstrap nonce is consumed by the WebView. Pasting `http://127.0.0.1:<port>` into Chrome therefore 401s; a phone on the same Wi-Fi cannot complete a TCP handshake. The control plane the user wants to share is already there. What is missing is a revocable admission for a second origin and, when asked, nearby or tailnet traffic, without changing `dsh web`.

## Decision

The desktop shell ships one complete share capability, not a CLI plugin and not a sequence of product versions. Codex review of the first draft (`proposals/2026-08-16-desktop-share-codex-review-raw.md`) falsified pairing-through-the-launch-cookie, live `--patch` trustedHosts, Tailscale `--bg` teardown, and automatic remote directory-picker fallback; the mechanism below is the one that shipped. The walkthrough is [`proposals/2026-08-16-desktop-share-access.md`](../../../../proposals/2026-08-16-desktop-share-access.md).

The metaphor is "open this window on another screen". The menu and tray grow **Open in Browser** / **在浏览器中打开** and **Use on Another Device…** / **在其他设备上使用…**. The latter opens a small native window owned by the shell. External browsers never receive the launch token.

A Desktop-only **share gateway** in the sidecar process (installed only when the paired desktop env is set) is a separate `http.Server`: it holds revocable `dsh-share` sessions, checks Host/Origin/local interface, strips client `X-DSH-Token` and forwarding headers, blocks `/__dshd_*`, and injects the launch token only on the hop to the loopback sidecar. Local browser traffic is rewritten to the loopback authority so settings still work; nearby and Tailscale traffic is rewritten to the static non-loopback name `dshd.share.internal`, which the desktop overlay lists in `trustedHosts` at boot, so `PRIVILEGED_METHODS` stay 403. `composeLive` does not re-read argv `--patch` overlays (`apps/cli/src/profile-boot.ts`), so no generated patch is used.

The injected bootstrap script posts the WebView nonce as today; **an absent nonce must not POST** and must resolve `__DSH_DESKTOP_BOOTSTRAP_DONE__`, or a paired browser hangs in the connection loop. Open in Browser opens `http://127.0.0.1:<gateway>/` during a short loopback pairing window so the ticket never enters `open` argv. QR pairing uses prefix `/p` (the webserver has no parameterized routes): GET returns a no-store interstitial forced to light color-scheme. A same-origin POST consumes the ticket and answers 200 with `Set-Cookie` plus `location.replace('/')`, not 303, because several phone WebViews drop cookies on redirects. The interstitial submits with `fetch` so Chrome stores the cookie before navigation; a form POST under `Referrer-Policy: no-referrer` sends `Origin: null`, which the gateway treats as a missing Origin while still rejecting `sec-fetch-site: cross-site`. The share window follows `ui-theme.preference` in `$DSH_HOME/settings.yaml`.

Nearby does not rebind the sidecar. `overlay.rs` stays fail-closed. The gateway tracks HTTP, SSE, and WebSocket clients and destroys them when a mode turns off or the generation rotates. Tailscale Serve preserves the external Host and `--bg` outlives the app: the shell runs a **foreground** `tailscale serve` at the gateway loopback port on a HTTPS port this feature owns, and never blindly `off`s 443. `setTailscaleAudience` runs only after `wait_https_listed` succeeds; a miss stops the child and returns the error to the share window, so a QR is never issued for a port Serve has not published.

`tapIndex` still injects the `randomUUID` polyfill. `desktop-state.json` gains merged `{ nearby, tailscale }` via read-validate-merge-rename. Remote directory picking is documented as unavailable — the picker backend is chosen from the primary loopback bind, not from the page's `isLoopback`.

Without the desktop env, none of this installs. `dsh web` stays loopback and unauthenticated.

This extends [the per-launch token](2026-08-14-desktop-per-launch-token.md) and does not change [the CLI bind address](2026-07-22-web-bind-address.md). The share window sits in the shell rather than `sidebar.footer.action` because, unlike [plugin restart](2026-08-16-desktop-plugin-restart-prompt.md), bind state belongs to the shell.

## Alternatives considered

**A general `dsh` plugin in the style of dsh-lan.** Overlay-binding `0.0.0.0` is the official composition seam, and the polyfill tap is the right fix for insecure-context UUID. It still has no authentication, so it would hand LAN RCE to every `dsh web` user who installed it, and it cannot draw a QR or drive Tailscale. This fork also forbids changing the non-desktop surfaces.

**Give external browsers the launch-token cookie and reverse-proxy with the client Host intact.** The first draft. Falsified: the bootstrap script POSTs an empty nonce and hangs the connection loop; cookies are not port-isolated; turning sharing off does not revoke the WebView's token; Tailscale keeps the external Host and `--bg` outlives the process; `composeLive` does not reload argv patches.

**Rebind the sidecar to `0.0.0.0` when nearby is on, restarting it.** One listen, reuse of `resolveLanTrust`. Rejected because a running agent dies.

**Always bind `0.0.0.0` in the desktop composition and fence with the admission guard.** The first launch prompts the OS firewall and the port answers SYN on the LAN while sharing is off.

**Put the share UI in the sidecar sidebar.** The QR has to stay visible beside the transcript, and a new client package edits aggregate tsconfigs this fork wants to merge from upstream.

**Capability URL in the fragment (`#dshd-nonce=`).** Several phone scanners drop fragments. Prefix `/p` plus an interstitial POST is the pairing URL.

**Cloudflare Tunnel or Tailscale Funnel.** A public HTTPS URL is a different product.

**Unlock `PRIVILEGED_METHODS` for token-bearing requests, or rewrite remote Host to loopback.** Either would let a phone drive native dialogs on the Mac. Remote traffic uses `dshd.share.internal`.

**A six-digit code typed on the phone.** Not the default.

**`tailscale serve --bg`.** Officially persists across reboot until explicit off, so quit would not retract the port.

## Testing

`pnpm vitest run packages/bundle/web-app --coverage.enabled --coverage.include='packages/bundle/web-app/src/**'` — 64 tests, 100% statements/branches/functions/lines on the package src, including `share-gateway.ts`.

`cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` — clippy clean, 95 tests passed.

CLI posture: `pnpm dsh --profile web --port 18765` with neither desktop env var set printed `dsh web: http://127.0.0.1:18765`; `GET /p/x` returned HTTP 200, `text/html`, body contains `__DSH_BOOT__`, and does not contain `此码已失效` or `请回到 Desktop`.

## Consequences

Open in Browser and the share sheet join another screen to the live instance without handing that screen the launch token. Nearby and Tailscale are off until asked; turning them off rotates generation and destroys tracked connections. Existing user Serve/Funnel on 443 is left intact because this feature never `off`s that port.

A photographed QR is a short-lived pairing URL. The interstitial POST and generation rotation bound that window; the copy still has to say so.

The gateway listen is a new attack surface even behind share sessions. The default remains off.

LAN HTTP has no transport encryption. The window copy names interception, not only QR theft.

Remote directory picking, settings, and credentials stay unavailable. Pretending the existing `isLoopback` UI degrades them would be false.

Wi-Fi client isolation looks like a product bug. The nearby page names that failure.
