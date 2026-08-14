/**
 * Shell-style recall of this session's own prompts. Pure derivation and a
 * pure cursor transition; the composer owns the cursor value and applies the
 * returned draft through the ordinary `setDraft` write path.
 */

import type { ConversationNode, SteeringMessageNode, UserMessageNode } from '@deepseek-ai/dsh-client-runtime/client'

/** Where the recall cursor sits: `null` = editing the live draft, `0` = newest recalled entry. */
export type PromptHistoryCursor = number | null

/** One recall step's result. */
export interface PromptRecall {
  /** Draft text to install, or `undefined` when the step does not move. */
  readonly draft: string | undefined
  /** Cursor after the step. */
  readonly cursor: PromptHistoryCursor
}

/** A step that changes nothing, reused so callers can compare by identity. */
const NO_MOVE: PromptRecall = { draft: undefined, cursor: null }

/**
 * The prompts this session has sent, newest first.
 *
 * Both finalized user messages and steering messages count — the user typed
 * both. Image-only and whitespace-only prompts are dropped (nothing to
 * recall), and a prompt identical to its predecessor collapses, so holding
 * ArrowUp through a retried prompt does not stall.
 * @param nodes - the session's conversation nodes in log order.
 * @returns recallable prompt texts, newest first.
 */
export function promptHistory(nodes: readonly ConversationNode[]): readonly string[] {
  const out: string[] = []
  for (const node of nodes) {
    if (node.kind !== 'user' && node.kind !== 'steering') continue
    const text = textOf(node.content)
    if (text.trim() === '' || text === out[0]) continue
    out.unshift(text)
  }
  return out
}

/**
 * Move the recall cursor one entry and report the draft to install.
 *
 * `back` walks toward older prompts and stops at the oldest. `forward` walks
 * toward newer ones and, past the newest, restores `liveDraft` — the text the
 * user was composing when recall began, which the caller stashed.
 * @param cursor - current cursor.
 * @param direction - `back` for older, `forward` for newer.
 * @param history - result of {@link promptHistory}.
 * @param liveDraft - draft stashed when recall began; restored past the newest entry.
 * @returns the next cursor and the draft to install, or {@link PromptRecall.draft} `undefined` to leave the draft alone.
 */
export function recallStep(
  cursor: PromptHistoryCursor,
  direction: 'back' | 'forward',
  history: readonly string[],
  liveDraft: string,
): PromptRecall {
  if (direction === 'back') {
    const next = cursor === null ? 0 : cursor + 1
    const draft = history[next]
    if (draft === undefined) return NO_MOVE
    return { draft, cursor: next }
  }
  if (cursor === null) return NO_MOVE
  if (cursor === 0) return { draft: liveDraft, cursor: null }
  const next = cursor - 1
  // A shrinking history (older nodes evicted) can leave the cursor past the
  // end; walking forward off that edge returns to the live draft.
  const draft = history[next]
  if (draft === undefined) return { draft: liveDraft, cursor: null }
  return { draft, cursor: next }
}

/**
 * Whether ArrowUp should begin recall rather than move the caret.
 *
 * Recall starts only from the very beginning of the draft with nothing
 * selected, so the key keeps its native meaning everywhere else in a
 * multi-line draft. Once recall is under way the cursor is non-null and the
 * caret no longer decides, which is what makes repeated ArrowUp walk back
 * instead of stalling on the recalled text's own first line.
 * @param cursor - current cursor.
 * @param selectionStart - textarea selection start.
 * @param selectionEnd - textarea selection end.
 * @returns true when the composer should recall.
 */
export function shouldRecall(
  cursor: PromptHistoryCursor,
  selectionStart: number,
  selectionEnd: number,
): boolean {
  if (cursor !== null) return true
  return selectionStart === 0 && selectionEnd === 0
}

function textOf(content: (UserMessageNode | SteeringMessageNode)['content']): string {
  return content
    .map(block => (block.type === 'text' ? block.text : ''))
    .join('')
}
