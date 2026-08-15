/**
 * The restart entry's props. The target 'sidebar.footer.action' slot is
 * declared and typed by ui-sidebar; this package only contributes the entry,
 * so no SlotMap merge lives here. The action injects no business face — the
 * state it shows belongs to the desktop shell and is read over IPC.
 * @module @deepseek-ai/dsh-client-ui-plugin-restart/client/slots
 */

import type { PropsLocale, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
// Type-only: pulls this package's LocaleNamespaceMap merge (the 'pluginRestart' seat).
import type {} from './locales.ts'

/** Full props of the sidebar-foot restart entry. */
export type PluginRestartActionProps =
  PropsRuntime<'sidebar.footer.action'>
  & PropsLocale<'pluginRestart'>
