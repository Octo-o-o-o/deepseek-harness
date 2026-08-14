# Agent Note: ArrowUp recall of a session's own prompts

Status: implemented

English | [中文](2026-08-14-composer-prompt-recall.zh.md)

## Problem

The composer had no way back to something already sent. Re-running a prompt with one word changed meant scrolling the transcript, selecting the text, and copying it — a gesture every shell and every REPL solves with ArrowUp. The keys were already spoken for in one narrow case (an open trigger menu moves its highlight with them) and otherwise fell through to the native caret.

## Decision

ArrowUp from a collapsed caret at offset 0 begins a recall walk over this session's own prompts, newest first; ArrowDown walks toward newer ones and, past the newest, restores the draft stashed when the walk began. The trigger menu keeps first claim on both keys — recall is what they mean only after `arbitrate` passes them back.

Once a walk has begun the caret stops deciding. The alternative — re-testing offset 0 on every key — stalls on the recalled text itself, because a recall places the caret at the end so the text is immediately editable, and a multi-line prompt then absorbs the next ArrowUp as an ordinary line move. A walk therefore ends on an explicit act instead: editing the draft, sending, or switching session. ArrowUp at the oldest prompt holds rather than wrapping, which is what a shell does and what makes holding the key safe.

`prompt-history.ts` is pure — a derivation from conversation nodes and a cursor transition — and the composer owns the cursor in a ref, reads the walk's draft, and writes it through the ordinary `setDraft` path plus a `track` call, exactly as the space-claim gesture does. The frozen input contract is untouched: recall composes existing verbs rather than adding machine state.

History comes from the loaded conversation nodes, not a separate list. Steering messages are included because the user typed them; whitespace-only and image-only prompts are dropped because they recall nothing, and a prompt repeated back to back collapses so holding ArrowUp through a retry keeps moving.

## Alternatives considered

**A ring buffer of drafts submitted in this browser tab.** It survives no reload, forgets everything sent before the tab opened, and would be a second source of truth beside the transcript already on screen.

**A durable cross-session prompt history under `$DSH_HOME`.** A larger feature with its own privacy question (prompts are the most sensitive text in the product) and no current consumer asking for it. The loaded window is the honest boundary until one does.

**Machine state instead of a component ref.** The cursor is read and written inside a single keydown and never renders; adding it to `InputState` would widen a frozen contract for a value no other consumer observes.

## Consequences

The loaded window bounds how far back a walk reaches — an old prompt scrolled out of the loaded range is not recallable, which matches what the user can see. Recall is refused while the composer is read-only or holds the pending lock, the same span that refuses every other draft write. Coverage is `prompt-history.client.spec.ts` for the pure derivation and cursor, plus the `prompt recall` cases in `input-bar.client.spec.tsx` for the key path, the trigger-menu precedence, the IME guard, and both refusals.
