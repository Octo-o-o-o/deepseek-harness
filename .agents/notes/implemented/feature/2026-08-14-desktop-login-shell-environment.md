# Agent Note: The desktop sidecar takes its PATH from the login shell

Status: implemented

English | [中文](2026-08-14-desktop-login-shell-environment.zh.md)

## Problem

An application opened from the Dock, Finder, or Spotlight inherits the launch daemon's environment. Reading the environment of a sidecar under the installed `dshd.app` showed `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, no `SSH_AUTH_SOCK`, and no `LANG`. The whitelist in `env.rs` copied that faithfully, and `subprocess-local` builds each tool child from the sidecar's own environment, so the agent's `bash` tool could not see Homebrew, nvm, `cargo`, or anything else the user installed, `git push` over SSH had no agent socket, and a `DEEPSEEK_API_KEY` exported in a shell profile was absent — while the same harness started from a terminal had all of them. The packaged `rg` and `/usr/bin/git` kept search and most version control working, which hid how large the difference was.

## Decision

`shell_env.rs` runs `$SHELL -ilc` once per launch and reads the `env -0` block between its own markers, with `stdin` closed, a 5s deadline, and `DSH_RESOLVING_ENVIRONMENT=1` set so a profile can skip interactive-only work. A shell that is missing, fails, or times out yields nothing and the launch environment stands.

The shell's answer is a value source, not a permission. `INHERITED_ENV` remains the only gate, so a credential exported in `.zshrc` for an unrelated service still never reaches the agent. Within that whitelist, `PATH` takes the login shell's value and every other name keeps the launch value when it has one: `PATH` from the launch daemon is not a choice anyone made, while a variable set on the application process was set by whoever started it. The whitelist gains `SSH_AUTH_SOCK`, `SHELL`, `USER`, `LOGNAME`, and the certificate names (`NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`, `SSL_CERT_DIR`) that a network with its own certificate authority requires.

`DSH_DESKTOP_SHELL_ENV=0` skips the probe.

## Alternatives considered

**Run the tools through a login shell instead** (`bash -lc` in `bash-local`). That changes every harness deployment to fix a desktop-launch problem, and it re-runs the user's profile on every tool call.

**Inherit the login shell wholesale.** It restores the leak the whitelist exists to prevent: an agent that can read its own environment can read every credential the user exports.

**Resolve only when `PATH` equals the launch daemon default.** It skips the probe for terminal launches, but makes the environment depend on a value comparison that a user with a short `PATH` would fail unpredictably.

## Consequences

Boot pays one shell startup, bounded at 5s. Unit tests cover marker parsing, values containing `=` and newlines, an unterminated block, the opt-out, a missing shell, and the merge rules including that a name outside the whitelist cannot enter. A launch under `env -i` with the launch daemon `PATH` was verified to hand the sidecar the user's full login `PATH`.
