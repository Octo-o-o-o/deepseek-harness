//! Loader-ready and host-ready probes.

use std::time::Duration;

use crate::http::{http_request, HttpError};

/// Health-check failure.
#[derive(Debug, thiserror::Error)]
pub enum HealthError {
    /// HTTP layer failed.
    #[error(transparent)]
    Http(#[from] HttpError),
    /// The index responded with a non-200 status.
    #[error("index health check returned HTTP {0}")]
    BadStatus(u16),
    /// The index body is not a dsh-hosted page.
    #[error("index response is missing the __DSH_BOOT__ marker")]
    MissingBootMarker,
    /// `host.describe` answered HTTP 200 but `result.ok` was not true.
    #[error("host.describe result is not ok")]
    DescribeNotOk,
    /// `/__dshd_status` never reported `ready: true`.
    #[error("desktop client did not become ready")]
    ClientNotReady,
}

/// GET `http://127.0.0.1:<port>/` and require `__DSH_BOOT__`.
///
/// # Parameters
/// - `port`: sidecar loopback port from the ready line.
///
/// # Returns
/// `Ok(())` when the page is a dsh-hosted index.
pub fn check_loader_ready(port: u16) -> Result<(), HealthError> {
    let response = http_request(
        "GET",
        "127.0.0.1",
        port,
        "/",
        &[],
        None,
        Duration::from_secs(5),
    )?;
    if response.status != 200 {
        return Err(HealthError::BadStatus(response.status));
    }
    if !response.body.contains("__DSH_BOOT__") {
        return Err(HealthError::MissingBootMarker);
    }
    Ok(())
}

/// JSON body for `POST /api/host.describe` (client-request envelope).
///
/// # Returns
/// A serialized client-request whose method matches the path.
pub fn host_describe_body() -> String {
    "{\"type\":\"client-request\",\"rpcId\":\"desktop-boot\",\"method\":\"host.describe\",\"payload\":{}}"
        .to_string()
}

/// POST `/api/host.describe` with the desktop token and require HTTP 200.
///
/// # Parameters
/// - `port`: sidecar loopback port.
/// - `token`: per-launch token; sent only as `X-DSH-Token`.
///
/// # Returns
/// `Ok(())` when the host handshake succeeds.
pub fn check_host_described(port: u16, token: &str) -> Result<(), HealthError> {
    let body = host_describe_body();
    let response = http_request(
        "POST",
        "127.0.0.1",
        port,
        "/api/host.describe",
        &[("Content-Type", "application/json"), ("X-DSH-Token", token)],
        Some(body.as_bytes()),
        Duration::from_secs(5),
    )?;
    if response.status != 200 {
        return Err(HealthError::BadStatus(response.status));
    }
    if !host_describe_ok(&response.body) {
        return Err(HealthError::DescribeNotOk);
    }
    Ok(())
}

/// Whether a `host.describe` JSON body reports business success.
///
/// # Parameters
/// - `body`: response body.
///
/// # Returns
/// `true` when `result.ok` is JSON `true`.
pub fn host_describe_ok(body: &str) -> bool {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return false,
    };
    parsed
        .get("result")
        .and_then(|result| result.get("ok"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

/// Poll `GET /__dshd_status` until the browser client reports ready.
///
/// # Parameters
/// - `port`: sidecar loopback port.
/// - `nonce`: bootstrap nonce sent as `X-DSH-Bootstrap`.
/// - `timeout`: exclusive wait budget.
///
/// # Returns
/// `Ok(())` when the status body is `{"ready":true}`.
pub fn wait_desktop_client_ready(
    port: u16,
    nonce: &str,
    timeout: Duration,
) -> Result<(), HealthError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let response = http_request(
            "GET",
            "127.0.0.1",
            port,
            "/__dshd_status",
            &[("X-DSH-Bootstrap", nonce)],
            None,
            Duration::from_secs(2),
        );
        if let Ok(response) = response {
            if response.status == 200 && status_ready(&response.body) {
                return Ok(());
            }
            if response.status != 200 && response.status != 401 {
                return Err(HealthError::BadStatus(response.status));
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(HealthError::ClientNotReady);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn status_ready(body: &str) -> bool {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return false,
    };
    parsed.get("ready").and_then(serde_json::Value::as_bool) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(body: &'static [u8], status: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        port
    }

    #[test]
    fn accepts_boot_marker() {
        let port = serve_once(b"<html>window.__DSH_BOOT__ = {}</html>", "200 OK");
        check_loader_ready(port).unwrap();
    }

    #[test]
    fn rejects_missing_marker() {
        let port = serve_once(b"<html>vite</html>", "200 OK");
        assert!(matches!(
            check_loader_ready(port),
            Err(HealthError::MissingBootMarker)
        ));
    }

    #[test]
    fn rejects_non_200() {
        let port = serve_once(b"nope", "503 Service Unavailable");
        assert!(matches!(
            check_loader_ready(port),
            Err(HealthError::BadStatus(503))
        ));
    }

    #[test]
    fn host_describe_requires_200() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.contains("POST /api/host.describe"));
            assert!(request.contains("X-DSH-Token: abc"));
            let body = br#"{"type":"server-response","rpcId":"desktop-boot","result":{"ok":true,"value":{}}}"#;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest);
        });
        check_host_described(port, "abc").unwrap();
        assert!(host_describe_body().contains("host.describe"));
        assert!(!host_describe_ok(r#"{"result":{"ok":false}}"#));
    }

    #[test]
    fn status_ready_requires_true() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.contains("GET /__dshd_status"));
            assert!(request.contains("X-DSH-Bootstrap: nonce"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"ready\":true}")
                .unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });
        wait_desktop_client_ready(port, "nonce", Duration::from_secs(2)).unwrap();
    }
}
