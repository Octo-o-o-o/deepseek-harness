# Agent Note: Desktop per-launch token is opt-in on the CLI and required in the shell

Status: implemented

English | [中文](2026-08-14-desktop-per-launch-token.zh.md)

## Problem

Loopback `/api` is a reachability fence, not authentication. Any local process can POST and open the two downlink WebSockets. A packaged desktop app must stop that without changing `dsh web` for CLI users.

## Decision

`--desktop-token <tok>` is empty-default: no flag means no check and no page injection. When the token is set, `dsh-web-app` taps the index to set `window.__DSH_TOKEN__` (escaped, never logged). The connection host compares `X-DSH-Token` or cookie `dsh-token` in constant time and returns 401 / rejects the upgrade on mismatch. The desktop shell always generates a hex token, passes the flag, probes `POST /api/host.describe` and both `/api/events.mux` and `/api/events.host` upgrades before `Visible`.

## Alternatives considered

**Unix domain socket / named pipe with OS ACL.** Stronger, deferred to a later M1 evaluation.

**Put the token in the ready-line URL.** It would land in logs and process listings.

## Consequences

Without the flag, existing CLI and `test:gui` behavior is unchanged. With the flag, a header-less `/api` call is 401. The token is not written to the URL or sidecar logs.
