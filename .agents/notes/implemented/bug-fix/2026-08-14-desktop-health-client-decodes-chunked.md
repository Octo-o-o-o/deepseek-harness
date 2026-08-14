# Agent Note: Decode chunked replies in the desktop health client

Status: implemented

English | [中文](2026-08-14-desktop-health-client-decodes-chunked.zh.md)

## Problem

`apps/desktop/src-tauri/src/http.rs` returned everything after the header terminator as the response body. The sidecar's Node server sets no `Content-Length` and answers every index and `/api` request with `Transfer-Encoding: chunked`, so the body reached the health checks still carrying chunk framing (`e7\r\n{…}\r\n0\r\n\r\n`).

`check_loader_ready` only substring-matches `__DSH_BOOT__`, which survives the framing, so gate two passed. `host_describe_ok` and `status_ready` parse JSON, which the framed text is not, so `check_host_described` failed on every launch with `host.describe result is not ok` and `wait_desktop_client_ready` could never observe `ready: true`. The desktop shell could not boot at all.

Neither existing gate could see it. The `health.rs` and `http.rs` unit tests served `Content-Length` bodies, and both real-server checks — `pack-sidecar.mjs` `selfCheck` and `smoke-app.sh` — probe with `fetch` and `curl`, which decode chunked transparently. The one client that does not decode chunked was the only client never pointed at the real server.

## Decision

`parse_http_response` splits head from body on the raw bytes, reads `Transfer-Encoding` from the head, and concatenates chunk payloads through `decode_chunked` when it declares `chunked`. Identity bodies are unchanged. Byte-level splitting keeps chunk sizes aligned with the wire for multi-byte UTF-8, which a `String`-level split cannot guarantee. Malformed or truncated framing is `HttpError::InvalidChunkedBody` rather than a body that silently parses as not-ok.

Chunk extensions after the size are skipped and a trailer section is not read: the reply is complete at the terminating zero-size chunk, and no health check consumes trailers.

The regression lives in `health.rs`, where `serve_once_chunked` frames its reply the way the sidecar does, so both JSON gates are now pinned against real framing instead of a fixture only this client would accept.

## Alternatives considered

**Have the sidecar send `Content-Length` on health routes.** It would fix these two gates by changing the product's HTTP surface for the benefit of one internal client, and any future route would reintroduce the bug.

**Substring-match the JSON instead of parsing it.** `body.contains("\"ok\":true")` would pass through framing, but it would also accept a nested `ok` from an unrelated field and drops the guarantee that the reply is a well-formed response.

**Use a Rust HTTP client crate.** The shell deliberately carries no HTTP dependency for three loopback probes; the [dependencies-over-hand-rolling policy](../process/2026-07-26-dependencies-over-hand-rolling.md) trades owned code for a dependency, and forty lines of chunk decoding with its own tests does not reach that threshold.

## Consequences

The four-gate boot completes: a packaged `dshd.app` spawns its sidecar, passes `host.describe`, navigates the WebView, and observes `/__dshd_ready`. Any future health route added to this client decodes correctly whichever framing the server picks.
