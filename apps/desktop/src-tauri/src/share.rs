//! Share sheet: loopback browser pairing, nearby QR, and Tailscale Serve.
//!
//! Commands here are invoked from the shell's own `share` window. The sidecar
//! origin is never granted them. Menu and tray call the same functions in-process.

use std::path::Path;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(not(target_os = "macos"))]
use tauri::menu::Submenu;
use tauri::menu::{Menu, MenuItem};
#[cfg(target_os = "macos")]
use tauri::menu::{MenuItemKind, PredefinedMenuItem};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Theme, WebviewUrl, WebviewWindowBuilder};

use crate::http::http_request;
use crate::navigation::is_internal_url;
use crate::opener::open_external_url;
use crate::paths::{self, SharePrefs};
use crate::tailscale::{self, ServeProcess};
use crate::AppState;

/// Tailscale download page opened when the CLI is missing.
pub const TAILSCALE_DOWNLOAD_URL: &str = "https://tailscale.com/download";

/// Live share handle filled after the sidecar's gateway answers.
pub struct ShareRuntime {
    sidecar_port: u16,
    token: String,
    loopback_port: u16,
    selected_address: Option<String>,
    nearby_on: bool,
    serve: Option<ServeProcess>,
}

impl ShareRuntime {
    /// Stop the foreground Serve child if one is running.
    pub fn stop_tailscale(&mut self) {
        if let Some(mut serve) = self.serve.take() {
            serve.stop();
        }
    }
}

/// Snapshot the share window renders.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSnapshot {
    /// Whether the gateway has answered since boot.
    pub ready: bool,
    /// Nearby panel.
    pub nearby: NearbySnapshot,
    /// Tailscale panel.
    pub tailscale: TailscaleSnapshot,
    /// Last command failure, when the window should keep showing it.
    pub error: Option<String>,
    /// Appearance preference from `ui-theme.preference` (`light` / `dark` / `system`).
    pub theme_preference: String,
}

/// Nearby listen and pairing URL.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbySnapshot {
    /// Whether the nearby listen is up.
    pub enabled: bool,
    /// Candidate LAN addresses.
    pub addresses: Vec<ShareAddress>,
    /// Address the QR currently advertises.
    pub selected_address: Option<String>,
    /// Pairing URL including the live ticket.
    pub ticket_url: Option<String>,
    /// SVG QR for `ticket_url`.
    pub qr_svg: Option<String>,
}

/// Tailscale install / Serve / pairing URL.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleSnapshot {
    /// Whether a Tailscale CLI was found.
    pub installed: bool,
    /// Whether `BackendState` is `Running`.
    pub running: bool,
    /// Whether this app's foreground Serve is up.
    pub enabled: bool,
    /// MagicDNS name.
    pub machine: Option<String>,
    /// HTTPS port passed to `tailscale serve`.
    pub https_port: Option<u16>,
    /// Pairing URL including the live ticket.
    pub ticket_url: Option<String>,
    /// SVG QR for `ticket_url`.
    pub qr_svg: Option<String>,
    /// Download page for a missing install.
    pub download_url: &'static str,
}

/// One LAN IPv4 the nearby QR may use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareAddress {
    /// IPv4 literal.
    pub address: String,
    /// OS interface name.
    pub iface: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayStatus {
    loopback_port: Option<u16>,
    nearby: Option<NearbyListen>,
    addresses: Vec<ShareAddress>,
    nearby_ticket_url: Option<String>,
    tailscale_ticket_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NearbyListen {
    bind_address: String,
}

/// Install the native menu items for Open in Browser and the share sheet.
///
/// # Parameters
/// - `app`: Tauri app handle.
///
/// # Returns
/// `Ok(())` after the menu is installed and the handler is registered.
pub fn install_app_menu(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        dispatch_menu(app, event.id().as_ref());
    });
    Ok(())
}

/// Handle a menu or tray id for share actions.
///
/// # Parameters
/// - `app`: Tauri app handle.
/// - `id`: menu item id.
///
/// # Returns
/// `true` when the id was a share action.
pub fn dispatch_menu(app: &AppHandle, id: &str) -> bool {
    match id {
        "open-in-browser" => {
            if let Err(err) = open_in_browser_now(app) {
                eprintln!("desktop: open in browser failed: {err}");
            }
            true
        }
        "open-share-window" => {
            if let Err(err) = open_share_window_now(app) {
                eprintln!("desktop: share window failed: {err}");
            }
            true
        }
        _ => false,
    }
}

/// Query the gateway and remember its loopback port. Failures are logged; boot
/// still shows the WebView.
///
/// # Parameters
/// - `app`: Tauri app handle.
/// - `sidecar_port`: sidecar loopback port.
/// - `token`: per-launch token.
pub fn attach_after_boot(app: &AppHandle, sidecar_port: u16, token: &str) {
    let mut last_err: Option<String> = None;
    let mut status = None;
    for _ in 0..20 {
        match post_control(sidecar_port, token, &json!({ "op": "status" })) {
            Ok(current) if current.loopback_port.is_some() => {
                status = Some(current);
                break;
            }
            Ok(_) => last_err = Some("share gateway loopback port is not bound".into()),
            Err(err) => last_err = Some(err),
        }
        thread::sleep(Duration::from_millis(50));
    }
    let Some(status) = status else {
        eprintln!(
            "desktop: share gateway unavailable: {}",
            last_err.unwrap_or_else(|| "unknown".into())
        );
        return;
    };
    let Some(loopback_port) = status.loopback_port else {
        return;
    };
    let runtime = ShareRuntime {
        sidecar_port,
        token: token.to_string(),
        loopback_port,
        selected_address: status.addresses.first().map(|entry| entry.address.clone()),
        nearby_on: false,
        serve: None,
    };
    match app.state::<AppState>().share.lock() {
        Ok(mut guard) => *guard = Some(runtime),
        Err(err) => {
            eprintln!("desktop: share state lock poisoned: {err}");
            return;
        }
    }
    // Restore after the handle is visible to request_stop, so a quit during
    // Tailscale spawn still finds the child.
    if let Err(err) = with_runtime(app, |live, home_dir| {
        restore_prefs(live, home_dir);
        Ok(())
    }) {
        eprintln!("desktop: could not restore share prefs: {err}");
    }
}

/// POST `/__dshd_share` on the sidecar. Cookie is not sent.
///
/// # Parameters
/// - `sidecar_port`: loopback sidecar port.
/// - `token`: per-launch token, sent as `X-DSH-Token`.
/// - `body`: control JSON.
///
/// # Returns
/// The gateway status snapshot.
pub(crate) fn post_control(
    sidecar_port: u16,
    token: &str,
    body: &Value,
) -> Result<GatewayStatus, String> {
    let payload = serde_json::to_vec(body).map_err(|err| err.to_string())?;
    let response = http_request(
        "POST",
        "127.0.0.1",
        sidecar_port,
        "/__dshd_share",
        &[("Content-Type", "application/json"), ("X-DSH-Token", token)],
        Some(&payload),
        Duration::from_secs(8),
    )
    .map_err(|err| err.to_string())?;
    if response.status == 409 {
        return Err(conflict_message(&response.body));
    }
    if response.status != 200 {
        return Err(format!("share control HTTP {}", response.status));
    }
    serde_json::from_str(&response.body).map_err(|err| err.to_string())
}

/// Render a QR as an SVG string.
///
/// # Parameters
/// - `payload`: URL to encode.
///
/// # Returns
/// SVG markup, or an error when the payload cannot be encoded.
pub fn qr_svg(payload: &str) -> Result<String, String> {
    use qrcode::render::svg;
    use qrcode::QrCode;
    let code = QrCode::new(payload.as_bytes()).map_err(|err| err.to_string())?;
    Ok(code
        .render::<svg::Color<'_>>()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#111111"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

/// Open the loopback gateway in the default browser during a pairing window.
///
/// # Parameters
/// - `app`: Tauri app handle.
///
/// # Returns
/// `Ok(())` after the launcher process starts.
#[tauri::command]
pub fn open_in_browser(app: AppHandle) -> Result<(), String> {
    open_in_browser_now(&app)
}

/// Show or focus the share sheet.
///
/// # Parameters
/// - `app`: Tauri app handle.
///
/// # Returns
/// `Ok(())` when the window is visible.
#[tauri::command]
pub fn open_share_window(app: AppHandle) -> Result<(), String> {
    open_share_window_now(&app)
}

/// Snapshot for the share sheet.
///
/// # Parameters
/// - `app`: Tauri app handle.
///
/// # Returns
/// Current listens, pairing URLs, and Tailscale presence.
#[tauri::command]
pub fn share_snapshot(app: AppHandle) -> Result<ShareSnapshot, String> {
    Ok(build_snapshot(&app, None))
}

/// Enable or disable the nearby listen.
///
/// # Parameters
/// - `app`: Tauri app handle.
/// - `enabled`: whether nearby devices may connect.
/// - `bind_address`: IPv4 to bind; omitted uses the last selection.
///
/// # Returns
/// A fresh snapshot after the listen change.
#[tauri::command]
pub fn set_share_nearby(
    app: AppHandle,
    enabled: bool,
    bind_address: Option<String>,
) -> Result<ShareSnapshot, String> {
    let result = with_runtime(&app, |runtime, home| {
        let status = set_nearby_inner(runtime, enabled, bind_address)?;
        runtime.nearby_on = status.nearby.is_some();
        persist(home, runtime);
        Ok(())
    });
    Ok(build_snapshot(&app, result.err()))
}

/// Enable or disable this app's foreground Tailscale Serve.
///
/// # Parameters
/// - `app`: Tauri app handle.
/// - `enabled`: whether the tailnet may connect.
///
/// # Returns
/// A fresh snapshot after the Serve change.
#[tauri::command]
pub fn set_share_tailscale(app: AppHandle, enabled: bool) -> Result<ShareSnapshot, String> {
    let result = with_runtime(&app, |runtime, home| {
        if enabled {
            enable_tailscale(runtime, home)?;
        } else {
            disable_tailscale(runtime)?;
        }
        persist(home, runtime);
        Ok(())
    });
    Ok(build_snapshot(&app, result.err()))
}

/// Rebind nearby to `address`, or remember it for the next enable.
///
/// # Parameters
/// - `app`: Tauri app handle.
/// - `address`: IPv4 literal from the gateway's address list.
///
/// # Returns
/// A fresh snapshot.
#[tauri::command]
pub fn select_share_address(app: AppHandle, address: String) -> Result<ShareSnapshot, String> {
    let result = with_runtime(&app, |runtime, home| {
        runtime.selected_address = Some(address.clone());
        if runtime.nearby_on {
            set_nearby_inner(runtime, true, Some(address))?;
            persist(home, runtime);
        }
        Ok(())
    });
    Ok(build_snapshot(&app, result.err()))
}

/// Open the Tailscale download page in the default browser.
///
/// # Returns
/// `Ok(())` after the launcher process starts.
#[tauri::command]
pub fn open_tailscale_download() -> Result<(), String> {
    let url = tauri::Url::parse(TAILSCALE_DOWNLOAD_URL).map_err(|err| err.to_string())?;
    open_external_url(&url).map_err(|err| err.to_string())
}

fn open_in_browser_now(app: &AppHandle) -> Result<(), String> {
    let loopback_port = {
        let state = app.state::<AppState>();
        let mut share = state.share.lock().map_err(|err| err.to_string())?;
        let runtime = share
            .as_mut()
            .ok_or_else(|| "local host is not ready".to_string())?;
        post_control(
            runtime.sidecar_port,
            &runtime.token,
            &json!({ "op": "openLoopback" }),
        )?;
        runtime.loopback_port
    };
    let url = tauri::Url::parse(&format!("http://127.0.0.1:{loopback_port}/"))
        .map_err(|err| err.to_string())?;
    open_external_url(&url).map_err(|err| err.to_string())
}

fn open_share_window_now(app: &AppHandle) -> Result<(), String> {
    let preference = snapshot_theme(app);
    if let Some(window) = app.get_webview_window("share") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        apply_share_window_theme(app, &preference);
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "share", WebviewUrl::App("share.html".into()))
        .title("在其他设备上使用")
        .inner_size(460.0, 760.0)
        .min_inner_size(400.0, 600.0)
        .resizable(true)
        .visible(true)
        .center()
        .theme(share_window_theme(&preference))
        .on_navigation(is_internal_url)
        .on_new_window(|url, _features| {
            let _ = open_external_url(&url);
            NewWindowResponse::Deny
        })
        .build()
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open_browser = MenuItem::with_id(
        app,
        "open-in-browser",
        "在浏览器中打开",
        true,
        Some("CmdOrCtrl+Shift+B"),
    )?;
    let share = MenuItem::with_id(
        app,
        "open-share-window",
        "在其他设备上使用…",
        true,
        None::<&str>,
    )?;
    #[cfg(target_os = "macos")]
    {
        let menu = Menu::default(app)?;
        if let Some(MenuItemKind::Submenu(submenu)) = menu.items()?.into_iter().next() {
            let sep = PredefinedMenuItem::separator(app)?;
            let at = if submenu.items()?.is_empty() { 0 } else { 1 };
            submenu.insert(&open_browser, at)?;
            submenu.insert(&share, at + 1)?;
            submenu.insert(&sep, at + 2)?;
        }
        Ok(menu)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let menu = Menu::default(app)?;
        let extra = Submenu::with_items(app, "dshd", true, &[&open_browser, &share])?;
        let _ = menu.append(&extra);
        Ok(menu)
    }
}

fn with_runtime<T>(
    app: &AppHandle,
    f: impl FnOnce(&mut ShareRuntime, &Path) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<AppState>();
    let home = {
        let guard = state.home.lock().map_err(|err| err.to_string())?;
        guard
            .clone()
            .ok_or_else(|| "local host is not ready".to_string())?
    };
    let mut share = state.share.lock().map_err(|err| err.to_string())?;
    let runtime = share
        .as_mut()
        .ok_or_else(|| "local host is not ready".to_string())?;
    f(runtime, &home)
}

fn restore_prefs(runtime: &mut ShareRuntime, home: &Path) {
    let prefs = paths::read_share_prefs(home);
    if prefs.nearby {
        match set_nearby_inner(runtime, true, None) {
            Ok(status) => runtime.nearby_on = status.nearby.is_some(),
            Err(err) => eprintln!("desktop: could not restore nearby share: {err}"),
        }
    }
    if prefs.tailscale {
        if let Err(err) = enable_tailscale(runtime, home) {
            eprintln!("desktop: could not restore Tailscale share: {err}");
        }
    }
}

fn set_nearby_inner(
    runtime: &mut ShareRuntime,
    enabled: bool,
    bind_address: Option<String>,
) -> Result<GatewayStatus, String> {
    let bind = bind_address.or_else(|| runtime.selected_address.clone());
    let mut body = json!({ "op": "setNearby", "enabled": enabled });
    if enabled {
        if let Some(address) = bind {
            body["bindAddress"] = json!(address);
        }
    }
    let status = post_control(runtime.sidecar_port, &runtime.token, &body)?;
    if let Some(nearby) = &status.nearby {
        runtime.selected_address = Some(nearby.bind_address.clone());
    }
    runtime.nearby_on = status.nearby.is_some();
    Ok(status)
}

fn enable_tailscale(runtime: &mut ShareRuntime, home: &Path) -> Result<(), String> {
    if runtime.serve.is_some() {
        return Ok(());
    }
    let bin = tailscale::discover_tailscale()
        .ok_or_else(|| "未安装 Tailscale。打开下载页安装后，两边都连上再开启。".to_string())?;
    let status = tailscale::run_json(&bin, &["status", "--json"])?;
    if !tailscale::backend_running(&status) {
        return Err("Tailscale 已安装但未连上。请先在 Tailscale 里连上，再打开这个开关。".into());
    }
    let machine =
        tailscale::dns_name(&status).ok_or_else(|| "Tailscale 没有报告机器名。".to_string())?;
    let serve = tailscale::run_json(&bin, &["serve", "status", "--json"])?;
    let funnel = tailscale::read_funnel(&bin);
    let occupied = tailscale::occupied_https_ports(&serve, &funnel);
    let https_port = tailscale::pick_https_port(&occupied).ok_or_else(|| {
        "本功能要用的 HTTPS 端口都被现有 Serve/Funnel 占用了。请在 Tailscale 里关掉一条后再试。不会自动关闭 443。".to_string()
    })?;
    let log_path = home.join("logs/tailscale-serve.log");
    let child = tailscale::spawn_serve(&bin, https_port, runtime.loopback_port, &log_path)
        .map_err(|err| format!("无法启动 tailscale serve: {err}"))?;
    let audience = tailscale::serve_audience(&machine, https_port);
    let mut process = ServeProcess::new(child, https_port, machine);
    thread::sleep(Duration::from_millis(250));
    if process.exited() {
        process.stop();
        return Err("tailscale serve 立刻退出了。请看 logs/tailscale-serve.log。".into());
    }
    if let Err(err) = tailscale::wait_https_listed(&bin, https_port, Duration::from_secs(8)) {
        process.stop();
        return Err(format!(
            "Tailscale Serve 还没把 HTTPS 端口发出来。请稍后再开，或看 logs/tailscale-serve.log。({err})"
        ));
    }
    post_control(
        runtime.sidecar_port,
        &runtime.token,
        &json!({ "op": "setTailscaleAudience", "audience": audience }),
    )?;
    runtime.serve = Some(process);
    Ok(())
}

fn disable_tailscale(runtime: &mut ShareRuntime) -> Result<(), String> {
    runtime.stop_tailscale();
    post_control(
        runtime.sidecar_port,
        &runtime.token,
        &json!({ "op": "setTailscaleAudience", "audience": Value::Null }),
    )?;
    Ok(())
}

fn persist(home: &Path, runtime: &ShareRuntime) {
    if let Err(err) = paths::merge_share_prefs(
        home,
        SharePrefs {
            nearby: runtime.nearby_on,
            tailscale: runtime.serve.is_some(),
        },
    ) {
        eprintln!("desktop: could not write share prefs: {err}");
    }
}

fn snapshot_theme(app: &AppHandle) -> String {
    let Some(state) = app.try_state::<AppState>() else {
        return "system".into();
    };
    let Ok(home) = state.home.lock() else {
        return "system".into();
    };
    home.as_deref()
        .map(paths::read_theme_preference)
        .unwrap_or("system")
        .to_string()
}

fn share_window_theme(preference: &str) -> Option<Theme> {
    match preference {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

fn apply_share_window_theme(app: &AppHandle, preference: &str) {
    let Some(window) = app.get_webview_window("share") else {
        return;
    };
    let _ = window.set_theme(share_window_theme(preference));
}

fn build_snapshot(app: &AppHandle, error: Option<String>) -> ShareSnapshot {
    let theme_preference = snapshot_theme(app);
    apply_share_window_theme(app, &theme_preference);
    let empty = ShareSnapshot {
        ready: false,
        nearby: NearbySnapshot {
            enabled: false,
            addresses: Vec::new(),
            selected_address: None,
            ticket_url: None,
            qr_svg: None,
        },
        tailscale: TailscaleSnapshot {
            installed: false,
            running: false,
            enabled: false,
            machine: None,
            https_port: None,
            ticket_url: None,
            qr_svg: None,
            download_url: TAILSCALE_DOWNLOAD_URL,
        },
        error,
        theme_preference,
    };
    let Some(state) = app.try_state::<AppState>() else {
        return empty;
    };
    let Ok(share) = state.share.lock() else {
        return empty;
    };
    let Some(runtime) = share.as_ref() else {
        return empty;
    };
    let status = match post_control(
        runtime.sidecar_port,
        &runtime.token,
        &json!({ "op": "status" }),
    ) {
        Ok(status) => status,
        Err(err) => {
            return ShareSnapshot {
                error: Some(empty.error.unwrap_or(err)),
                ..empty
            }
        }
    };
    let selected = runtime
        .selected_address
        .clone()
        .or_else(|| status.nearby.as_ref().map(|n| n.bind_address.clone()))
        .or_else(|| status.addresses.first().map(|a| a.address.clone()));
    let nearby_qr = status
        .nearby_ticket_url
        .as_deref()
        .and_then(|url| qr_svg(url).ok());
    let tailscale_qr = status
        .tailscale_ticket_url
        .as_deref()
        .and_then(|url| qr_svg(url).ok());
    let (installed, running, machine) = tailscale_presence();
    ShareSnapshot {
        ready: true,
        nearby: NearbySnapshot {
            enabled: status.nearby.is_some(),
            addresses: status.addresses,
            selected_address: selected,
            ticket_url: status.nearby_ticket_url,
            qr_svg: nearby_qr,
        },
        tailscale: TailscaleSnapshot {
            installed,
            running,
            enabled: runtime.serve.is_some(),
            machine: runtime
                .serve
                .as_ref()
                .map(|serve| serve.machine.clone())
                .or(machine),
            https_port: runtime.serve.as_ref().map(|serve| serve.https_port),
            ticket_url: status.tailscale_ticket_url,
            qr_svg: tailscale_qr,
            download_url: TAILSCALE_DOWNLOAD_URL,
        },
        error: empty.error,
        theme_preference: empty.theme_preference,
    }
}

fn tailscale_presence() -> (bool, bool, Option<String>) {
    let Some(bin) = tailscale::discover_tailscale() else {
        return (false, false, None);
    };
    match tailscale::run_json(&bin, &["status", "--json"]) {
        Ok(status) => (
            true,
            tailscale::backend_running(&status),
            tailscale::dns_name(&status),
        ),
        Err(_) => (true, false, None),
    }
}

fn conflict_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0_u8; 256];
        loop {
            let n = stream.read(&mut tmp).expect("read request");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            let Some(split) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let header_end = split + 4;
            let headers = std::str::from_utf8(&buf[..header_end]).unwrap_or("");
            let len = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                })
                .unwrap_or(0);
            if buf.len() >= header_end + len {
                break;
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    #[test]
    fn qr_svg_encodes_a_pairing_url() {
        let svg = qr_svg("http://192.168.1.8:9/p/abc").expect("qr");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("#111111"));
    }

    #[test]
    fn share_window_theme_maps_preference() {
        assert!(matches!(share_window_theme("light"), Some(Theme::Light)));
        assert!(matches!(share_window_theme("dark"), Some(Theme::Dark)));
        assert!(share_window_theme("system").is_none());
        assert!(share_window_theme("sepia").is_none());
    }

    #[test]
    fn post_control_injects_the_token_and_parses_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_http_request(&mut stream);
            assert!(req.contains("X-DSH-Token: secret"));
            assert!(req.contains("POST /__dshd_share"));
            let payload = r#"{"loopbackPort":9,"nearby":null,"tailscaleAudience":null,"addresses":[],"nearbyTicketUrl":null,"tailscaleTicketUrl":null}"#;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{payload}",
                payload.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        });
        let status = post_control(port, "secret", &json!({ "op": "status" })).unwrap();
        assert_eq!(status.loopback_port, Some(9));
        server.join().unwrap();
    }

    #[test]
    fn post_control_surfaces_a_409_error_field() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut stream);
            let payload = r#"{"error":"share gateway: no LAN address for nearby listen"}"#;
            let reply = format!(
                "HTTP/1.1 409 Conflict\r\nContent-Length: {}\r\n\r\n{payload}",
                payload.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        });
        let err = post_control(
            port,
            "secret",
            &json!({ "op": "setNearby", "enabled": true }),
        )
        .unwrap_err();
        assert!(err.contains("no LAN address"));
        server.join().unwrap();
    }

    #[test]
    fn conflict_message_falls_back_to_the_raw_body() {
        assert_eq!(conflict_message("nope"), "nope");
        assert_eq!(conflict_message(r#"{"error":"boom"}"#), "boom");
    }
}
