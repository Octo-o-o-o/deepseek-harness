/**
 * Sidebar-foot action offering a restart once the profile's plugin list has
 * moved away from what the running composition read.
 *
 * The confirmation is unconditional rather than gated on a busy session: a
 * restart stops the whole local session process, and the browser has no
 * cross-session view of which ones hold an answer in flight — the sidebar's
 * session list carries no running flag. Asking once is the honest default; a
 * conditional prompt would have to guess.
 * @module @deepseek-ai/dsh-client-ui-plugin-restart/client/PluginRestartAction
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { pluginsPendingRestart, requestRestart } from './shell.ts'
import type { PluginRestartActionProps } from './slots.ts'
import css from './PluginRestartAction.module.css'

/** How often the page re-asks the shell whether a restart would change anything. */
const POLL_MS = 5_000

/**
 * The restart entry, rendered only while the shell reports pending changes.
 * @param props - the sidebar's fold state and the slot's copy accessor.
 * @returns the action, plus the confirmation dialog while it is open.
 */
export function PluginRestartAction({ t, wide }: PluginRestartActionProps) {
  const [pending, setPending] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const alive = useRef(true)

  useEffect(() => {
    alive.current = true
    const poll = (): void => {
      void pluginsPendingRestart().then((value) => {
        if (alive.current) setPending(value)
      })
    }
    poll()
    const timer = setInterval(poll, POLL_MS)
    return () => {
      alive.current = false
      clearInterval(timer)
    }
  }, [])

  const accept = useCallback(() => {
    setFailure(null)
    // A successful restart replaces the process, so this promise settling at
    // all means the request failed to take effect.
    void requestRestart().catch(() => {
      if (alive.current) setFailure(t('confirm.failed'))
    })
  }, [t])

  if (!pending) return null

  return (
    <>
      <button
        type="button"
        className={wide ? css.action : css.rail}
        title={t('action.hint')}
        aria-label={t('action.label')}
        aria-haspopup="dialog"
        onClick={() => { setConfirming(true) }}
      >
        {wide ? t('action.label') : '\u21bb'}
      </button>
      {confirming && (
        <div className={css.overlay} role="presentation">
          <div className={css.panel} role="dialog" aria-modal="true" aria-label={t('confirm.title')}>
            <p className={css.title}>{t('confirm.title')}</p>
            <p className={css.body}>{t('confirm.body')}</p>
            {failure !== null && <p className={css.failure}>{failure}</p>}
            <div className={css.buttons}>
              <button type="button" className={css.cancel} onClick={() => { setConfirming(false) }}>
                {t('confirm.cancel')}
              </button>
              <button type="button" className={css.accept} autoFocus onClick={accept}>
                {t('confirm.accept')}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  )
}
