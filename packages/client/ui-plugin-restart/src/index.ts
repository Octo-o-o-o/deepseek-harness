/**
 * Plugin-restart surface plugin, node half. Pure UI plugin: the empty apply
 * exists so the plugin appears in the host cordis.yml / Loader; the browser
 * half ships via exports["./client"], discovered through the package.json
 * dsh.client declaration.
 *
 * The state it surfaces belongs to the desktop shell, not to this process: the
 * shell stamped the profile manifest when it launched this sidecar, and only
 * it can restart the application.
 */

/** Host plugin body — no host-side behavior for this surface plugin. */
export function apply(): void {}
