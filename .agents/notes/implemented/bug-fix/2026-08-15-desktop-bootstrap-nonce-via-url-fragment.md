# Agent Note: Deliver the desktop bootstrap nonce through the URL fragment

Status: implemented

English | [中文](2026-08-15-desktop-bootstrap-nonce-via-url-fragment.zh.md)

## Problem

The desktop shell handed the page its one-time bootstrap nonce by writing it into the served index: `injectDesktopBootstrapScript(html, nonce)` emitted `window.__DSH_DESKTOP_BOOTSTRAP__ = "<nonce>"`. The page then posted that value to `/__dshd_bootstrap` and received the HttpOnly `dsh-token` cookie for `/api`.

The nonce therefore sat in a response body served by an unauthenticated route. `GET /` on the sidecar port returns 200 without any credential — verified against a running instance — so every local process able to reach that loopback port could read the nonce, exchange it for the token cookie, and drive `/api`. Loopback carries no user identity, so this includes processes owned by a *different* local account, not only the user's own. Since the agent exposes a bash tool, that path is local privilege escalation to arbitrary code execution.

The `/api` namespace itself was never the weak point: it correctly rejects credential-less requests with 401. Only the delivery channel for the nonce was public.

## Decision

The shell navigates to `http://127.0.0.1:<port>/#dshd-nonce=<nonce>` and the injected script reads the value from `location.hash`. User agents never put the fragment on the wire, so the nonce reaches the page without appearing in any response body, and the served index becomes byte-identical across launches.

`injectDesktopBootstrapScript(html)` no longer takes a nonce. `encodeBootstrapNonceLiteral` is deleted: it existed only to escape a nonce being embedded in HTML, which no longer happens.

After reading the value the script calls `history.replaceState` to drop the key, preserving any unrelated fragment state. This keeps the nonce out of the back/forward entry and out of reach of page scripts that run later. An absent fragment yields an empty nonce, which `/__dshd_bootstrap` rejects — a page opened without the shell gets no cookie rather than a partial session.

`navigate_to_sidecar` takes the nonce and builds that URL. Nonces come from `generate_desktop_token`, which is hex, so the fragment needs no percent-encoding. `is_internal_url` and `is_sidecar_origin` compare scheme, host, and port, so the added fragment does not affect the navigation fence, and the post-read `replaceState` leaves the origin check passing.

## Alternatives considered

**Bind the loopback listener to the user.** Correct in principle, but loopback TCP has no portable peer-UID check across macOS and Windows, and it would not remove a secret from a public response body.

**Require the `X-DSH-Bootstrap` header on `GET /`.** The first navigation is performed by the WebView, not by the shell's HTTP client, so the shell cannot attach a header to it.

**Shorten the nonce TTL further.** The window already spans the whole boot budget for a reason: first launch under real-time AV scanning can exceed 15s. Shrinking it trades a real startup-reliability failure for a smaller race window that still exists.

## Verification

`packages/bundle/web-app/tests/desktop-bootstrap.spec.ts` evaluates the injected script in a `vm` context with a stubbed `location`/`history`: the nonce comes from the fragment, unrelated fragment state survives while the key is removed, an absent fragment yields an empty nonce and no history rewrite, and hostile fragment content stays an opaque value instead of executing. One case asserts that two injections with no nonce argument produce identical HTML, pinning the property that the served bytes carry no secret.

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass in `apps/desktop/src-tauri`. Web-app package tests pass (18). `verify-translation-pairing` reports 956 consistent pairs after re-recording the desktop README pair.

Not yet covered: a packaged-app run proving the WebView boots through the fragment path end to end. The next signed build must confirm it before release, because the unit tests stub `location` rather than exercising a real WebView navigation.

## Consequences

The served index no longer varies with the launch secret, so an attacker who reaches the loopback port gains nothing from `GET /`. The remaining exposure is the navigation URL itself, which stays inside the WebView process.

Anything that opens the sidecar URL without the shell — a browser pointed at the port for debugging — now gets an unauthenticated page whose bootstrap POST fails. That is the intended behavior, and it is the same outcome as an expired nonce.
