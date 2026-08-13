# Agent Note: Desktop per-launch token travels on env and an HttpOnly cookie

Status: implemented

English | [中文](2026-08-14-desktop-per-launch-token.zh.md)

## Problem

Loopback `/api` is a reachability fence, not authentication. Putting the per-launch token in argv (`ps` / `/proc/cmdline`) or in the unauthenticated index (`window.__DSH_TOKEN__`) lets any local process steal it and call `/api`.

## Decision

The shell injects paired `DSH_DESKTOP_TOKEN` and `DSH_DESKTOP_BOOTSTRAP_NONCE` env. A lone variable fails the web-startup load. The index receives only the nonce (JSON.stringify plus angle-bracket escapes). `POST /__dshd_bootstrap` consumes that nonce once within 30s and sets `Set-Cookie: dsh-token=…; Path=/api; HttpOnly; SameSite=Strict` (and the same cookie for `/__dshd_ready`). The connection node half still accepts `X-DSH-Token` or the cookie so the shell can self-check without bootstrap. After both downlinks are up the browser posts `/__dshd_ready`; the shell polls `/__dshd_status` with `X-DSH-Bootstrap` before `Visible`. Absent env leaves the unauthenticated CLI default.

## Alternatives considered

**Keep `--desktop-token` and only stop logging it.** Argv remains readable to every process of the same user.

**Give the renderer the token over Tauri IPC.** That still puts a bearer secret in JS. HttpOnly cookie plus header-only shell checks keep the secret out of the page.

## Consequences

`dsh web` without the paired env is unchanged. A consumed or expired nonce cannot mint another cookie. Desktop Loader settlement refuses any `/api` registration whose owner is not `client-connection`.
