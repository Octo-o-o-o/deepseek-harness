# Agent Note: Authenticate desktop `/api` at the webserver, not inside one route

Status: implemented

English | [中文](2026-08-15-webserver-admission-guard.zh.md)

## Problem

Desktop mode checked its per-launch token inside the connection plugin's own handlers — once for the `/api` prefix route and once for each WebSocket upgrade. That is the only place the token was read, and it can only fence the routes connection owns.

`WebServer.match()` resolves the exact table before any prefix, so a plugin registering `exact /api/anything` answers ahead of connection's `prefix /api` and never passes a token check. The desktop server binds loopback, and loopback carries no user identity: any local process, including one running as another account, could read whatever such a route served.

The guard against that was `assertDesktopApiExclusive`, which threw when any `/api` registration named an owner other than `client-connection`. It ran after Loader settlement, so the throw failed the whole composition. A user who installed [`dsh-usage-stats`](https://github.com/omdsh-dev/dsh-usage-stats) — four exact routes under `/api/usage-stats/` serving token usage and DeepSeek account balance — got a sidecar that died before printing its ready line and a shell error reading `sidecar stdout closed before the ready line`, with the real diagnostic only in `$DSH_HOME/logs/sidecar.log`.

So the two properties were coupled the wrong way round: the harness refused a legitimate extension point in order to protect a namespace it could have protected directly, and it refused it by taking the application down.

## Decision

The webserver owns one **admission-guard seat**. `registerGuard(guard)` installs a `WebRequestGuard`, run on every HTTP request and every upgrade before any route table is consulted; it returns `undefined` to admit or the HTTP status refusing the request. A second registration throws, an empty seat admits everything, and a guard that throws is contained like any other per-request failure — 400 on HTTP, a destroyed socket on upgrade.

The seat is deliberately generic. It takes `(req, pathname)` and returns a status; it knows nothing about tokens, `/api`, or the desktop. It exists because a route can fence only itself, so a rule that must also cover routes registered by plugins the deployment does not control has nowhere else to live.

`dsh-web-app` claims the seat in its desktop branch with `desktopApiGuard(session)`: outside `/api` it admits, inside `/api` it requires the launch token through the same `hasValidDesktopToken` the connection handlers use. A plugin's own `/api` namespace is therefore authenticated without that plugin implementing anything, and the CLI and browser surfaces — which install no guard — behave exactly as before.

`assertDesktopApiExclusive` becomes `describeForeignApiRoutes`, a pure function returning a diagnostic instead of throwing. Foreign `/api` routes are no longer a security problem, but precedence still is: an exact route named after an RPC method replaces that method for the launch. The report names each route and owner and goes to stderr, which the desktop shell tees into `sidecar.log`. It is not `ctx.logger.warn` because the shipped composition mounts no logger exporter and cordis suppresses `warn` at the default level, so that call would print nowhere.

The connection plugin keeps its own two token checks. They are the CLI-facing behavior of that plugin's config and remain correct where no guard is installed.

The shell also stops reporting only the symptom. A ready-line failure now carries the last 20 lines of `sidecar.log` into the boot-failure message, so any fatal load failure — not just this one — reaches the error page instead of `$DSH_HOME/logs`.

## Alternatives considered

**Delete the assertion and rely on the trust fence.** The `isTrustedApiRequest` Host fence lives inside connection's handler too, so it protects nothing a third-party exact route serves. Deleting the assertion alone would have shipped the unauthenticated read the assertion existed to prevent.

**Keep the assertion but downgrade it to throw only when no guard is installed.** The desktop branch installs the guard unconditionally, three lines above the check, so the throwing arm would be unreachable by construction — a fence beside a gate that is welded open.

**Let the guard return a boolean and hardcode 401 in the webserver.** That puts a harness authorization decision inside a package whose whole point is knowing no harness concepts. Returning the status keeps the policy with its owner.

**Allow multiple guards.** No consumer needs a second one, and two seats could not both decide a binary admission. The single seat mirrors `registerFallback`, whose disposer-and-throw semantics it copies.

**Match only the connection plugin's RPC method names and reject collisions at registration.** Connection registers a prefix, so the webserver cannot know which sub-paths are methods without connection telling it — a coupling that would put the RPC vocabulary into the carrier.

## Verification

`pnpm vitest run packages/host/webserver packages/bundle/web-app packages/client/connection` — 16 files, 151 tests, all passing.

The new coverage: the webserver suite boots a real Loader composition and asserts that a guarded path refuses with the guard's status while its registered handler never runs, that an admitted request reaches exactly the route it would have without a guard, that upgrades follow both paths, that a status with no registered reason phrase still produces a parsable line, that a throwing guard is contained, and that the seat behaves as one seat (second registration throws, disposer restores routing and registrability). The web-app suite asserts the seat is claimed in desktop mode and left empty without it, and that a foreign `/api` route produces a stderr report rather than a failed launch.

Mutation check, run to prove the tests would catch the defect: with `registerGuard` altered not to store the guard and the web-app effect altered not to install one, exactly the two intended tests fail —

```
× runs the admission guard ahead of every route, on requests and on upgrades
× guards every /api path with the launch token, whoever registered the route
Test Files  2 failed | 3 passed (5)
      Tests  2 failed | 33 passed (35)
```

— and both pass again once the mutation is reverted.

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` (74 passing) cover the shell change, including the log-tail read past its byte window and the boot message with and without a readable log.

End to end on the reporting machine, `dsh-usage-stats` re-enabled in the profile patch and both desktop environment variables set. The composition that used to die before its ready line now prints the foreign-route report and then `dsh web: http://127.0.0.1:45999`, and every path behaves as designed:

| request | status |
|---|---|
| `GET /api/usage-stats/balance`, no credential | 401 |
| same, `X-DSH-Token` correct | 200 with the plugin's real balance payload |
| same, `X-DSH-Token` wrong | 401 |
| `GET /api/usage-stats/usage`, `dsh-token` cookie | 200 |
| `POST /api/host.describe`, no credential / with token | 401 / 200 |
| upgrade `/api/events.mux`, no credential | `HTTP/1.1 401 Unauthorized` + `Connection: close` |
| same, `dsh-token` cookie | `HTTP/1.1 101 Switching Protocols` |
| `GET /__dshd_status` with `X-DSH-Bootstrap` | 200 |
| `GET /` | 200 |

## Consequences

`/api` is now authenticated once, at the carrier, instead of once per route owner that remembers to. That is strictly stronger than what the assertion protected, because it also covers a route the connection plugin has never heard of.

The webserver gains a seat with security meaning, which its README and the [subsystem doc](../../../../docs/subsystems/web-server.md) now state: the package still implements no policy, but it is no longer true that authorization cannot be expressed there.

A plugin can still shadow an RPC method with an exact `/api/<method>` route. Nothing prevents it — the report is a diagnostic — and preventing it would require the carrier to know connection's method vocabulary.
