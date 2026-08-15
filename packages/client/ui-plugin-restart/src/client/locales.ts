/** `pluginRestart` namespace dictionaries. */

/** Simplified Chinese dictionary (the key-set source of truth). */
export const zh = {
  'action.label': '重启以启用插件',
  'action.hint': '检测到插件变更，重启后生效',
  'confirm.title': '重启应用？',
  'confirm.body': '新装的插件要在应用重新组合后才会加载。重启会关闭本机的会话进程，正在进行的回答会被中断，已保存的会话记录不受影响。',
  'confirm.accept': '重启',
  'confirm.cancel': '取消',
  'confirm.failed': '重启请求没有送达应用外壳',
} satisfies Record<string, string>

/** English dictionary. */
export const en = {
  'action.label': 'Restart to enable plugins',
  'action.hint': 'Plugin changes detected; a restart applies them',
  'confirm.title': 'Restart the application?',
  'confirm.body': 'Newly installed plugins load only after the application composes again. Restarting stops the local session process, so an answer in progress is interrupted; saved session history is unaffected.',
  'confirm.accept': 'Restart',
  'confirm.cancel': 'Cancel',
  'confirm.failed': 'The restart request did not reach the application shell',
} satisfies Record<keyof typeof zh, string>

/** The pluginRestart namespace key union. */
export type PluginRestartKey = keyof typeof zh

declare module '@deepseek-ai/dsh-client-ui-slots' {
  interface LocaleNamespaceMap {
    /** The desktop restart action's copy. */
    pluginRestart: PluginRestartKey
  }
}
