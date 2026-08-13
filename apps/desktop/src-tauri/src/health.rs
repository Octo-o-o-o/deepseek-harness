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
    Ok(())
}

const MUX_EVENTS_PATH: &str = "/api/events.mux";
const HOST_EVENTS_PATH: &str = "/api/events.host";

/// Upgrade both downlink WebSockets with the desktop token cookie.
///
/// # Parameters
/// - `port`: sidecar loopback port.
/// - `token`: per-launch token; sent only as `dsh-token`.
///
/// # Returns
/// `Ok(())` when both upgrades return HTTP 101.
pub fn check_websockets_ready(port: u16, token: &str) -> Result<(), HealthError> {
    for path in [MUX_EVENTS_PATH, HOST_EVENTS_PATH] {
        let response = websocket_upgrade(port, path, token)?;
        if response.status != 101 {
            return Err(HealthError::BadStatus(response.status));
        }
    }
    Ok(())
}

fn websocket_upgrade(
    port: u16,
    path: &str,
    token: &str,
) -> Result<crate::http::HttpResponse, HealthError> {
    let cookie = format!("dsh-token={token}");
    Ok(http_request(
        "GET",
        "127.0.0.1",
        port,
        path,
        &[
            ("Connection", "Upgrade"),
            ("Upgrade", "websocket"),
            ("Sec-WebSocket-Version", "13"),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ("Cookie", cookie.as_str()),
        ],
        None,
        Duration::from_secs(5),
    )?)
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
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest);
        });
        check_host_described(port, "abc").unwrap();
        assert!(host_describe_body().contains("host.describe"));
    }

    #[test]
    fn websocket_upgrade_requires_101() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0_u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                assert!(request.contains("Upgrade: websocket"));
                assert!(request.contains("Cookie: dsh-token=abc"));
                stream
                    .write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n")
                    .unwrap();
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        });
        check_websockets_ready(port, "abc").unwrap();
    }
}
