# Agent Note: Refuse a second JSONL writer instead of corrupting the log

Status: implemented

English | [中文](2026-08-15-jsonl-single-writer-containment.zh.md)

## Problem

The JSONL backend documented "one live writer per session" but nothing enforced it, and the desktop app made violating it ordinary: `dshd` and `npx @deepseek-ai/dsh` share `~/.dsh` by design, so a user with both open on the same session had two backend instances writing one file.

Corruption did not require any failure. `PersistenceCoordinator.append` takes the cursor from `this.states` — per-instance memory — and only checks contiguity against it (`coordinator.ts:694-704`). Two instances that adopt the same stored length both hold cursor `N`, and both append batches starting at `seq = N`. Both writes succeed. The reader then hits the duplicate: `format.ts:364-371` reports `corrupt session log: seq gap in committed region`, throws when the batch carries `turn/end`, and otherwise **drops every event after that line**. The user loses history with no error at write time.

Two further races existed on the same file. `appendLines` recorded the pre-write length and, on a write failure, truncated back to it — deleting anything a second writer had committed in between. `repair` truncated to an offset computed from the log this instance had read, which after a foreign append points into the other writer's data.

An earlier draft of this work claimed the damage required "two processes, the same session, and one of them failing to write". That was wrong, and it led to the fix being scheduled behind less urgent work.

## Decision

The backend records the on-disk state it believes it owns per session — `dev`, `ino`, and byte length — and refuses to write when the file is not in that state.

Ownership is taken where this instance has just seen or produced the whole file: in `readPrefix` (adoption) and after `materialize` (publication). Recording it at adoption rather than at the first append is what makes the check work: two instances that adopt the same length both record it, the first append moves the file, and the second instance's check fails instead of appending at a position already taken.

`appendLines` verifies before writing and re-records afterwards by re-stat'ing the file rather than adding the encoded length, since compression makes that length non-obvious. `repair` verifies on the same handle it truncates with, so a destructive offset cannot be applied to a file that changed after the offset was computed.

The refusal is one-way. Once a foreign write is observed, the coordinator's in-memory cursor no longer describes the file, so the session is marked poisoned and later writes are refused until it is reopened. Ownership is keyed per session, so a conflict on one session leaves the rest of the home writable.

This bounds damage; it is not a lock. A writer that does not run this code — an older CLI — can still interleave. Making that impossible needs an enforced lease, which is a larger change and is recorded as deferred work rather than pretended here.

## Alternatives considered

**A cross-process advisory lock (`proper-lockfile` or similar).** It also fails to stop an old CLI, since advisory locks bind only participants that take them, and it adds failure modes this check does not have: stale-lease misjudgement after a laptop sleep, leftover locks after a crash, and no fencing token. It would buy prevention over detection only for writers that already cooperate.

**Comparing only the byte length.** Length alone cannot tell a replaced file from an extended one. `dev`/`ino` cost nothing extra in the same `stat` and close that gap.

**Validating inside the coordinator instead.** The coordinator is backend-neutral, and the SQLite backend has its own transactional guarantees. Putting a file-identity check there would impose file semantics on every backend.

## Verification

`packages/session/session-persistence-jsonl/tests/jsonl.spec.ts` adds a `single-writer containment` suite that drives two backend instances over one root: the second writer's append is refused after the first extends the log while the first writer's events stay readable; a poisoned session keeps refusing with a distinct message; an unrelated session stays writable after a conflict; and a repair whose truncation offset predates a foreign append is refused with the committed events intact.

The suite was confirmed to fail for the right reason: with `assertWriteOwnership` stubbed to return immediately, all four cases fail; restoring it turns them green. Full package runs pass (508 tests across `session-persistence`, `session-persistence-jsonl`, and `web-app`), so the check does not disturb the single-writer path it now guards.

## Consequences

Concurrent use of the desktop app and the CLI on the same session now produces a refused write with a message naming the other writer, instead of a log that silently loses its tail. Sessions opened in only one place are unaffected: the check is one `stat` on a handle the append already opens.

A user who hits the refusal must reopen the session to continue writing it. That is deliberate — the alternative is trusting a cursor that is known to disagree with the file.
