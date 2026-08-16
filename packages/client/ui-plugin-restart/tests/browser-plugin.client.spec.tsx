// @vitest-environment jsdom
/**
 * The browser half on a real cordis Context with a fake slot registry: the
 * plugin contributes one sidebar-foot entry, the entry stays invisible until
 * the shell reports a pending change, the confirmation gates the restart, and
 * both the registration and the dictionary ride the plugin fiber (HMR safety).
 * The node half and the invariant companion are exercised over the same
 * Context.
 */
import { Context } from '@deepseek-ai/cordis'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import { SlotRegistry } from '@deepseek-ai/dsh-client-runtime/client'
import { LocaleRuntime } from '@deepseek-ai/dsh-client-locale/client'
import { apply, inject } from '../src/client/index.ts'
import { apply as nodeApply } from '../src/index.ts'
import { PluginRestartAction } from '../src/client/PluginRestartAction.tsx'
import { zh } from '../src/client/locales.ts'

type TauriHost = {
  __TAURI__?: { core?: { invoke?: (command: string) => Promise<unknown> } }
  __TAURI_INTERNALS__?: { invoke?: (command: string) => Promise<unknown> }
}

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  delete (globalThis as TauriHost).__TAURI__
  delete (globalThis as TauriHost).__TAURI_INTERNALS__
})

/** Install a shell whose answers the test controls, recording every command. */
function shell(pending: boolean, onRestart: () => Promise<unknown> = async () => undefined): string[] {
  const seen: string[] = []
  ;(globalThis as TauriHost).__TAURI__ = {
    core: {
      invoke: async (command) => {
        seen.push(command)
        if (command === 'plugins_pending_restart') return pending
        return onRestart()
      },
    },
  }
  return seen
}

/** Boot both halves over a Context carrying the slot and locale services. */
async function bench() {
  const ctx = new Context()
  await ctx.plugin(SlotRegistry).await()
  ctx.slots.register({
    name: 'root',
    children: { 'sidebar.footer.action': { kind: 'list', scope: 'root' } },
  } as never, (() => null) as never)
  ctx.provide('locale', new LocaleRuntime(ctx))
  const fiber = ctx.plugin({ inject: [...inject], apply })
  await fiber.await()
  return { ctx, fiber, entries: () => ctx.slots.entries('sidebar.footer.action') }
}

/**
 * Render the action with a translator returning the Chinese copy. The two
 * framework hooks are part of every root-scope slot's props; this action reads
 * neither, so they answer their own selector against an empty state.
 */
function renderAction(wide = true) {
  const t = ((key: keyof typeof zh) => zh[key]) as never
  const hook = ((selector: (state: unknown) => unknown) => selector({})) as never
  return render(
    <PluginRestartAction t={t} wide={wide} useSessions={hook} useWorkspaces={hook} />,
  )
}

describe('plugin-restart browser half', () => {
  it('contributes one sidebar-foot entry and withdraws it with the fiber', async () => {
    const { fiber, entries } = await bench()
    expect(entries().map(entry => entry.options.id)).toEqual(['plugin-restart'])
    await fiber.dispose()
    expect(entries()).toEqual([])
  })

  it('has an inert node half and a registrable invariant companion', async () => {
    expect(nodeApply).not.toThrow()
    // A bare Context: the bench one already carries an invariants registry
    // through the slot runtime, and the service seat admits one provider.
    const ctx = new Context()
    const registered: string[] = []
    ctx.provide('invariants', { register: (name: string) => { registered.push(name); return () => {} } } as never)
    const companion = await import('../src/invariant.ts')
    await companion.apply(ctx)
    expect(registered).toEqual(['@deepseek-ai/dsh-client-ui-plugin-restart'])
    await ctx.fiber.dispose()
  })

  it('renders the rail glyph when the sidebar is folded', async () => {
    shell(true)
    await act(async () => { renderAction(false) })
    expect(screen.getByRole('button', { name: zh['action.label'] }).textContent).toBe('\u21bb')
  })

  it('stays invisible while the shell reports nothing pending', async () => {
    shell(false)
    await act(async () => { renderAction() })
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('offers the restart, confirms first, and only then calls the shell', async () => {
    const seen = shell(true)
    await act(async () => { renderAction() })

    const action = screen.getByRole('button', { name: zh['action.label'] })
    expect(seen).toContain('plugins_pending_restart')
    // Clicking the entry must not restart on its own: a restart stops every
    // local session, so the dialog is the commit point.
    expect(seen).not.toContain('restart_for_plugins')

    await act(async () => { action.click() })
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(screen.getByText(zh['confirm.body'])).toBeTruthy()

    await act(async () => { screen.getByRole('button', { name: zh['confirm.cancel'] }).click() })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(seen).not.toContain('restart_for_plugins')

    await act(async () => { action.click() })
    await act(async () => { screen.getByRole('button', { name: zh['confirm.accept'] }).click() })
    expect(seen).toContain('restart_for_plugins')
  })

  it('reports a restart the shell never accepted, leaving the dialog open', async () => {
    shell(true, () => Promise.reject(new Error('denied')))
    await act(async () => { renderAction() })
    await act(async () => { screen.getByRole('button', { name: zh['action.label'] }).click() })
    await act(async () => { screen.getByRole('button', { name: zh['confirm.accept'] }).click() })
    expect(screen.getByText(zh['confirm.failed'])).toBeTruthy()
    expect(screen.getByRole('dialog')).toBeTruthy()
  })

  it('drops answers that arrive after unmount instead of setting state on a dead component', async () => {
    let releasePoll: (value: boolean) => void = () => {}
    let releaseRestart: (reason: Error) => void = () => {}
    ;(globalThis as TauriHost).__TAURI__ = {
      core: {
        invoke: async (command) => {
          if (command === 'plugins_pending_restart') {
            return new Promise<boolean>((resolve) => { releasePoll = resolve })
          }
          return new Promise<never>((_resolve, reject) => { releaseRestart = reject })
        },
      },
    }
    // First mount: unmount with the poll still in flight, then answer it.
    const idle = renderAction()
    idle.unmount()
    await act(async () => { releasePoll(true) })
    expect(screen.queryByRole('button')).toBeNull()

    // Second mount: reach the confirmation, then unmount with the restart
    // request still pending before rejecting it.
    const seen = shell(true, () => new Promise<never>((_resolve, reject) => { releaseRestart = reject }))
    const live = renderAction()
    await act(async () => { await Promise.resolve() })
    await act(async () => { screen.getByRole('button', { name: zh['action.label'] }).click() })
    await act(async () => { screen.getByRole('button', { name: zh['confirm.accept'] }).click() })
    live.unmount()
    await act(async () => { releaseRestart(new Error('too late')) })
    expect(seen).toContain('restart_for_plugins')
  })

  it('picks up a change that lands after mount, then stops polling on unmount', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    let pending = false
    ;(globalThis as TauriHost).__TAURI__ = {
      core: { invoke: async command => (command === 'plugins_pending_restart' ? pending : undefined) },
    }
    const view = renderAction()
    await act(async () => { await Promise.resolve() })
    expect(screen.queryByRole('button')).toBeNull()

    pending = true
    await act(async () => { await vi.advanceTimersByTimeAsync(5_000) })
    expect(screen.getByRole('button', { name: zh['action.label'] })).toBeTruthy()

    // The interval must die with the component: a detached poll would keep
    // asking the shell for the life of the page.
    view.unmount()
    const before = vi.getTimerCount()
    expect(before).toBe(0)
  })
})
