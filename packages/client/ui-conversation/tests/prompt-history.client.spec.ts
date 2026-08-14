import { describe, expect, it } from 'vitest'
import type { ConversationNode } from '@deepseek-ai/dsh-client-runtime/client'
import { promptHistory, recallStep, shouldRecall } from '../src/client/input/prompt-history.ts'

function user(text: string, seq = 1): ConversationNode {
  return { kind: 'user', seq, time: 0, content: [{ type: 'text', text }], source: null }
}

function steering(text: string, seq = 1): ConversationNode {
  // messageId is branded; a literal cannot carry the brand.
  return { kind: 'steering', messageId: `m${String(seq)}`, seq, time: 0, content: [{ type: 'text', text }], source: null } as unknown as ConversationNode
}

function assistant(text: string, seq = 1): ConversationNode {
  return { kind: 'assistant', seq, time: 0, content: [{ type: 'text', text }] } as unknown as ConversationNode
}

describe('promptHistory', () => {
  it('lists the session\'s own prompts newest first and ignores other nodes', () => {
    const nodes = [user('first', 1), assistant('reply', 2), user('second', 3)]
    expect(promptHistory(nodes)).toEqual(['second', 'first'])
  })

  it('counts steering messages, which the user also typed', () => {
    expect(promptHistory([user('a', 1), steering('interrupt', 2)])).toEqual(['interrupt', 'a'])
  })

  it('joins multi-block text and drops non-text blocks', () => {
    const node = {
      kind: 'user',
      seq: 1,
      time: 0,
      content: [{ type: 'text', text: 'look ' }, { type: 'image', attachment: {} }, { type: 'text', text: 'here' }],
      source: null,
    } as unknown as ConversationNode
    expect(promptHistory([node])).toEqual(['look here'])
  })

  it('drops image-only and whitespace-only prompts, which recall nothing', () => {
    const imageOnly = { kind: 'user', seq: 1, time: 0, content: [{ type: 'image', attachment: {} }], source: null } as unknown as ConversationNode
    expect(promptHistory([imageOnly, user('   \n ', 2)])).toEqual([])
  })

  it('collapses a prompt repeated back to back so holding ArrowUp keeps moving', () => {
    expect(promptHistory([user('again', 1), user('again', 2), user('other', 3)])).toEqual(['other', 'again'])
  })

  it('keeps a repeat that is not adjacent', () => {
    expect(promptHistory([user('a', 1), user('b', 2), user('a', 3)])).toEqual(['a', 'b', 'a'])
  })
})

describe('recallStep', () => {
  const history = ['newest', 'middle', 'oldest']

  it('walks back from the live draft through every entry', () => {
    const first = recallStep(null, 'back', history, 'live')
    expect(first).toEqual({ draft: 'newest', cursor: 0 })
    const second = recallStep(first.cursor, 'back', history, 'live')
    expect(second).toEqual({ draft: 'middle', cursor: 1 })
    expect(recallStep(second.cursor, 'back', history, 'live')).toEqual({ draft: 'oldest', cursor: 2 })
  })

  it('stops at the oldest entry instead of wrapping', () => {
    expect(recallStep(2, 'back', history, 'live')).toEqual({ draft: undefined, cursor: null })
  })

  it('does not move back when there is no history', () => {
    expect(recallStep(null, 'back', [], 'live')).toEqual({ draft: undefined, cursor: null })
  })

  it('walks forward and restores the stashed draft past the newest entry', () => {
    expect(recallStep(1, 'forward', history, 'live')).toEqual({ draft: 'newest', cursor: 0 })
    expect(recallStep(0, 'forward', history, 'live')).toEqual({ draft: 'live', cursor: null })
  })

  it('leaves the draft alone when moving forward without an active recall', () => {
    expect(recallStep(null, 'forward', history, 'live')).toEqual({ draft: undefined, cursor: null })
  })

  it('returns to the live draft when the history shrank under the cursor', () => {
    expect(recallStep(5, 'forward', ['only'], 'live')).toEqual({ draft: 'live', cursor: null })
  })
})

describe('shouldRecall', () => {
  it('requires a collapsed caret at the very start before recall begins', () => {
    expect(shouldRecall(null, 0, 0)).toBe(true)
    expect(shouldRecall(null, 3, 3)).toBe(false)
    expect(shouldRecall(null, 0, 4)).toBe(false)
  })

  it('ignores the caret once recall is under way', () => {
    expect(shouldRecall(0, 12, 12)).toBe(true)
  })
})
