//! Locate the Tailscale CLI and run a foreground `serve` at the share gateway.
//!
//! The shell never passes `--bg`: a background Serve survives quit and would
//! keep the gateway port published. HTTPS ports already claimed by the user's
//! Serve or Funnel are skipped; 443 is never turned off.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::process::hide_child_console;

/// HTTPS ports this feature will consider, in preference order.
pub const HTTPS_CANDIDATES: &[u16] = &[443, 8443, 8444, 9443, 10443, 11443];

/// A live foreground `tailscale serve` child.
pub struct ServeProcess {
    child: Option<Child>,
    /// HTTPS port passed to `--https`.
    pub https_port: u16,
    /// MagicDNS name without a trailing dot.
    pub machine: String,
}

impl ServeProcess {
    /// Wrap a spawned serve child.
    ///
    /// # Parameters
    /// - `child`: the foreground `tailscale serve` process.
    /// - `https_port`: port passed as `--https`.
    /// - `machine`: MagicDNS name.
    ///
    /// # Returns
    /// A handle that SIGTERMs the child on [`ServeProcess::stop`] or drop.
    pub fn new(child: Child, https_port: u16, machine: String) -> Self {
        Self {
            child: Some(child),
            https_port,
            machine,
        }
    }

    /// Whether the child has already exited.
    ///
    /// # Returns
    /// `true` when there is no live process.
    pub fn exited(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        match child.try_wait() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => true,
        }
    }

    /// SIGTERM the child, wait briefly, then SIGKILL. A second call is a no-op.
    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        terminate_child(&mut child);
    }
}

impl Drop for ServeProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Locate `tailscale` on PATH, then the platform app / Program Files binary.
///
/// # Returns
/// The first existing executable, if any.
pub fn discover_tailscale() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    discover_tailscale_in(std::env::split_paths(&path), &platform_fallbacks())
}

/// Search `path_dirs` then `extra` for a Tailscale CLI.
///
/// # Parameters
/// - `path_dirs`: directories from `PATH`.
/// - `extra`: platform fallbacks such as the macOS app binary.
///
/// # Returns
/// The first existing executable.
pub fn discover_tailscale_in(
    path_dirs: impl IntoIterator<Item = PathBuf>,
    extra: &[PathBuf],
) -> Option<PathBuf> {
    for dir in path_dirs {
        if let Some(found) = executable_in(&dir, "tailscale") {
            return Some(found);
        }
    }
    extra.iter().find(|path| path.is_file()).cloned()
}

/// argv for a foreground Serve targeting the gateway loopback port.
///
/// # Parameters
/// - `https_port`: `--https` value.
/// - `gateway_port`: gateway listen on `127.0.0.1`.
///
/// # Returns
/// Arguments after the binary. Never includes `--bg`.
pub fn serve_args(https_port: u16, gateway_port: u16) -> Vec<String> {
    vec![
        "serve".into(),
        format!("--https={https_port}"),
        format!("http://127.0.0.1:{gateway_port}"),
    ]
}

/// Host the share gateway should expect for this HTTPS port.
///
/// # Parameters
/// - `dns_name`: MagicDNS name without a trailing dot.
/// - `https_port`: Serve HTTPS port.
///
/// # Returns
/// `machine.ts.net` for 443, otherwise `machine.ts.net:port`.
pub fn serve_audience(dns_name: &str, https_port: u16) -> String {
    if https_port == 443 {
        dns_name.to_string()
    } else {
        format!("{dns_name}:{https_port}")
    }
}

/// Spawn foreground `tailscale serve`. The caller owns teardown.
///
/// # Parameters
/// - `bin`: Tailscale CLI.
/// - `https_port`: `--https` value.
/// - `gateway_port`: gateway loopback port.
/// - `log_path`: appended stdout/stderr.
///
/// # Returns
/// The child process.
pub fn spawn_serve(
    bin: &Path,
    https_port: u16,
    gateway_port: u16,
    log_path: &Path,
) -> io::Result<Child> {
    if let Some(parent) = log_path.parent() {
        fs_create_dir_all(parent)?;
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let stderr = log.try_clone()?;
    let mut cmd = Command::new(bin);
    hide_child_console(&mut cmd);
    cmd.env("TAILSCALE_BE_CLI", "1")
        .args(serve_args(https_port, gateway_port))
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(stderr);
    cmd.spawn()
}

/// Run the CLI and parse JSON stdout. Empty or non-JSON stdout is `{}`.
///
/// # Parameters
/// - `bin`: Tailscale CLI.
/// - `args`: arguments after the binary.
///
/// # Returns
/// The parsed object, or an error when the process fails.
pub fn run_json(bin: &Path, args: &[&str]) -> Result<Value, String> {
    let output = Command::new(bin)
        .env("TAILSCALE_BE_CLI", "1")
        .args(args)
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let name = args.first().copied().unwrap_or("command");
        return Err(format!("tailscale {name} failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    Ok(serde_json::from_str(trimmed).unwrap_or_else(|_| Value::Object(serde_json::Map::new())))
}

/// Funnel status, or an empty object when the command is unavailable.
///
/// # Parameters
/// - `bin`: Tailscale CLI.
///
/// # Returns
/// Parsed JSON, or `{}`.
pub fn read_funnel(bin: &Path) -> Value {
    run_json(bin, &["funnel", "status", "--json"])
        .unwrap_or_else(|_| Value::Object(serde_json::Map::new()))
}

/// HTTPS ports claimed by Serve / Funnel TCP and Web maps.
///
/// # Parameters
/// - `serve`: `tailscale serve status --json`.
/// - `funnel`: `tailscale funnel status --json`.
///
/// # Returns
/// Ports this feature must not `--https`.
pub fn occupied_https_ports(serve: &Value, funnel: &Value) -> HashSet<u16> {
    let mut out = HashSet::new();
    collect_tcp_ports(serve.get("TCP"), &mut out);
    collect_host_ports(serve.get("Web"), &mut out);
    collect_tcp_ports(funnel.get("TCP"), &mut out);
    collect_host_ports(funnel.get("Web"), &mut out);
    collect_host_ports(funnel.get("AllowFunnel"), &mut out);
    out
}

/// First candidate port not in `occupied`.
///
/// # Parameters
/// - `occupied`: ports already published.
///
/// # Returns
/// A port from [`HTTPS_CANDIDATES`], or `None` when all are taken.
pub fn pick_https_port(occupied: &HashSet<u16>) -> Option<u16> {
    HTTPS_CANDIDATES
        .iter()
        .copied()
        .find(|port| !occupied.contains(port))
}

/// Whether `status --json` reports a connected backend.
///
/// # Parameters
/// - `status`: `tailscale status --json`.
///
/// # Returns
/// `true` when `BackendState` is `Running`.
pub fn backend_running(status: &Value) -> bool {
    status.get("BackendState").and_then(Value::as_str) == Some("Running")
}

/// MagicDNS name with trailing dots stripped.
///
/// # Parameters
/// - `status`: `tailscale status --json`.
///
/// # Returns
/// `Self.DNSName` without a trailing dot.
pub fn dns_name(status: &Value) -> Option<String> {
    status
        .get("Self")
        .and_then(|node| node.get("DNSName"))
        .and_then(Value::as_str)
        .map(|name| name.trim_end_matches('.').to_string())
        .filter(|name| !name.is_empty())
}

/// Wait until `serve status` lists `port`, or `timeout` elapses.
///
/// # Parameters
/// - `bin`: Tailscale CLI.
/// - `port`: HTTPS port that should appear.
/// - `timeout`: how long to poll.
///
/// # Returns
/// `Ok(())` once the port is listed.
pub fn wait_https_listed(bin: &Path, port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let serve = run_json(bin, &["serve", "status", "--json"])?;
        let funnel = read_funnel(bin);
        if occupied_https_ports(&serve, &funnel).contains(&port) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("tailscale serve did not publish HTTPS port {port}"));
        }
        thread::sleep(Duration::from_millis(150));
    }
}

fn collect_tcp_ports(value: Option<&Value>, out: &mut HashSet<u16>) {
    let Some(map) = value.and_then(Value::as_object) else {
        return;
    };
    for key in map.keys() {
        if let Ok(port) = key.parse() {
            out.insert(port);
        }
    }
}

fn collect_host_ports(value: Option<&Value>, out: &mut HashSet<u16>) {
    let Some(map) = value.and_then(Value::as_object) else {
        return;
    };
    for key in map.keys() {
        if let Some((_, port)) = key.rsplit_once(':') {
            if let Ok(parsed) = port.parse::<u16>() {
                out.insert(parsed);
                continue;
            }
        }
        out.insert(443);
    }
}

fn executable_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    #[cfg(windows)]
    {
        let exe = dir.join(format!("{name}.exe"));
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

fn platform_fallbacks() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from(
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        ));
    }
    #[cfg(windows)]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            out.push(PathBuf::from(pf).join("Tailscale").join("tailscale.exe"));
        }
        if let Some(pf) = std::env::var_os("ProgramFiles(x86)") {
            out.push(PathBuf::from(pf).join("Tailscale").join("tailscale.exe"));
        }
    }
    out
}

fn fs_create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // SAFETY: the pid is this process's child.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "dsh-desktop-tailscale-{nanos}-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn serve_args_are_foreground_and_loopback() {
        let args = serve_args(8443, 9_001);
        assert_eq!(args, vec!["serve", "--https=8443", "http://127.0.0.1:9001"]);
        assert!(!args.iter().any(|arg| arg.contains("--bg")));
    }

    #[test]
    fn audience_omits_the_default_https_port() {
        assert_eq!(
            serve_audience("box.tailnet.ts.net", 443),
            "box.tailnet.ts.net"
        );
        assert_eq!(
            serve_audience("box.tailnet.ts.net", 8443),
            "box.tailnet.ts.net:8443"
        );
    }

    #[test]
    fn occupied_ports_read_tcp_web_and_funnel() {
        let serve = json!({
            "TCP": { "443": { "HTTPS": true } },
            "Web": { "box.tailnet.ts.net:443": { "Handlers": {} } }
        });
        let funnel = json!({
            "AllowFunnel": { "box.tailnet.ts.net:8443": true }
        });
        let occupied = occupied_https_ports(&serve, &funnel);
        assert!(occupied.contains(&443));
        assert!(occupied.contains(&8443));
        assert_eq!(pick_https_port(&occupied), Some(8444));
    }

    #[test]
    fn a_bare_web_host_counts_as_443() {
        let serve = json!({ "Web": { "box.tailnet.ts.net": {} } });
        let occupied = occupied_https_ports(&serve, &json!({}));
        assert!(occupied.contains(&443));
        assert_eq!(pick_https_port(&occupied), Some(8443));
    }

    #[test]
    fn pick_https_port_returns_none_when_every_candidate_is_taken() {
        let occupied = HTTPS_CANDIDATES.iter().copied().collect();
        assert_eq!(pick_https_port(&occupied), None);
    }

    #[test]
    fn dns_name_strips_trailing_dots_and_requires_running() {
        let status = json!({
            "BackendState": "Running",
            "Self": { "DNSName": "box.tailnet.ts.net." }
        });
        assert!(backend_running(&status));
        assert_eq!(dns_name(&status).as_deref(), Some("box.tailnet.ts.net"));
        assert!(!backend_running(&json!({ "BackendState": "Stopped" })));
        assert_eq!(dns_name(&json!({ "Self": { "DNSName": "" } })), None);
    }

    #[test]
    fn discovers_a_path_binary_before_fallbacks() {
        let dir = temp_dir();
        let bin = dir.join("tailscale");
        fs::write(&bin, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let missing = dir.join("missing-fallback");
        let found = discover_tailscale_in([dir.clone()], &[missing]);
        assert_eq!(found, Some(bin));
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn stop_terminates_a_sleeping_child() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let mut serve = ServeProcess::new(child, 443, "x".into());
        serve.stop();
        assert!(serve.exited());
    }
}
