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

## The bootstrap cookie is scoped to the origin

Guarding `/api` fixed the namespace the guard can see. It did not fix the other half of the plugin HTTP surface, which the same investigation then exposed.

`connection.rpc.handle(channel, …)` is the supported way for a plugin to serve its own HTTP endpoints: connection registers the channel as a prefix route and guards it with this same launch token (`rpc-host.ts`). But `CHANNEL_PATTERN` admits exactly one path segment and `assertChannel` reserves `/api`, so **every** channel necessarily sits at a top-level path outside `/api` — `/dsh-mnemon-read`, `/dsh-context`.

The bootstrap response used to scope its cookie to `Path=/api` plus `Path=/__dshd_ready`. A cookie is only sent to paths its scope covers, so the token never reached a single channel, and no plugin could do anything about it: the browser cannot attach `X-DSH-Token` to the client RPC's fetches either. Desktop mode therefore answered 401 to every plugin channel for the life of the launch, while the same plugins worked under `dsh web`, where no token is configured and the check is a no-op.

The cookie is now issued once with `Path=/`. The token authenticates this WebView's origin, not one namespace. Scoping per path cannot work even in principle: channels are registered and disposed while the page runs, long after the single bootstrap response that would have had to enumerate them.

The change is one line in the bootstrap response, and connection is untouched. That is deliberate: under `dsh web` the same plugins already work, because no token is configured and connection's check is a no-op, so the defect and its fix belong entirely to the desktop composition.

Two unrelated gaps surfaced during the investigation and are **not** addressed here, because neither causes this failure and both would change CLI behavior: a dedicated channel bridges its request body with `bridge(req, res, handler)` and so ignores the deployment's `maxRequestBodyBytes`, keeping the 160 MiB default while `/api` honors the configured ceiling; and the client RPC's `fetch` leaves credentials to the `same-origin` default while the two neighbouring desktop fetches state it. Each is worth its own change.

`/plugins` (client bundles) and the HMR event stream stay unauthenticated, which the guard does not change. They are page-load infrastructure fetched before any handshake can be awaited, and they carry no user data — bundle code and a module graph. Gating them would risk a blank window to hide nothing.

## Alternatives considered

**Delete the assertion and rely on the trust fence.** The `isTrustedApiRequest` Host fence lives inside connection's handler too, so it protects nothing a third-party exact route serves. Deleting the assertion alone would have shipped the unauthenticated read the assertion existed to prevent.

**Keep the assertion but downgrade it to throw only when no guard is installed.** The desktop branch installs the guard unconditionally, three lines above the check, so the throwing arm would be unreachable by construction — a fence beside a gate that is welded open.

**Let the guard return a boolean and hardcode 401 in the webserver.** That puts a harness authorization decision inside a package whose whole point is knowing no harness concepts. Returning the status keeps the policy with its owner.

**Allow multiple guards.** No consumer needs a second one, and two seats could not both decide a binary admission. The single seat mirrors `registerFallback`, whose disposer-and-throw semantics it copies.

**Match only the connection plugin's RPC method names and reject collisions at registration.** Connection registers a prefix, so the webserver cannot know which sub-paths are methods without connection telling it — a coupling that would put the RPC vocabulary into the carrier.

**Issue one cookie per registered channel instead of scoping to the origin.** The bootstrap response is written once, before the page runs; channels are registered and disposed throughout the session, so any channel mounted after that response — or re-mounted after an HMR reload — would still be unreachable. The two `Set-Cookie` lines this replaced were already the beginning of that dead end.

**Move the channels under `/api` so the existing `/api` scope covers them.** That changes the public path of every channel and breaks the plugins that already ship, to save a cookie attribute.

**Have the client send `X-DSH-Token` on channel fetches instead of relying on the cookie.** The page never receives the token — that is the point of the nonce exchange. Handing it to page JavaScript to put in a header would undo the HttpOnly property the whole bootstrap exists to establish.

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

Cookie scope, verified the same way with the two plugins that mount channels installed (`dsh-mnemon`, `dsh-context`), using a client that applies the RFC 6265 path rule as a browser does — bootstrap, then the jar:

| request | before | after |
|---|---|---|
| cookie issued by `/__dshd_bootstrap` | `Path=/api`, `Path=/__dshd_ready` | one cookie, `Path=/` |
| `/dsh-mnemon-read/status` with the jar | 401 | 200 |
| `/dsh-context/status` with the jar | 401 | 200 |
| `/api/host.describe` with the jar | 200 | 200 |
| `/__dshd_ready` with the jar | 204 | 204 |
| `/dsh-mnemon-read/status`, no credential | 401 | 401 |
| `/api/host.describe`, no credential | 401 | 401 |

Mutation-checked: reverting the cookie to `Path=/api` fails the bootstrap test and nothing else moves. The same probe run without the desktop environment variables — the `dsh web` posture — reaches `/dsh-mnemon-read/status` and `/dsh-context/status` with 200 both before and after, confirming the CLI surface never had this defect and is not touched by the fix.

## Consequences

`/api` is now authenticated once, at the carrier, instead of once per route owner that remembers to. That is strictly stronger than what the assertion protected, because it also covers a route the connection plugin has never heard of.

The webserver gains a seat with security meaning, which its README and the [subsystem doc](../../../../docs/subsystems/web-server.md) now state: the package still implements no policy, but it is no longer true that authorization cannot be expressed there.

A plugin can still shadow an RPC method with an exact `/api/<method>` route. Nothing prevents it — the report is a diagnostic — and preventing it would require the carrier to know connection's method vocabulary.
