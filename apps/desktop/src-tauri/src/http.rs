//! Tiny loopback-only HTTP/1.1 client used by health checks.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// One HTTP response (status + undecoded body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Numeric status code.
    pub status: u16,
    /// Response body as lossy UTF-8.
    pub body: String,
}

/// HTTP exchange failure.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// The overlay refuses any non-loopback target.
    #[error("desktop overlay refused a non-loopback HTTP target")]
    RefusedHost,
    /// The peer did not return a parseable HTTP/1.1 response.
    #[error("invalid HTTP response")]
    InvalidResponse,
    /// Transport failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The response exceeded the 4MB desktop cap.
    #[error("HTTP response exceeded 4MB")]
    TooLarge,
}

/// Maximum response body the desktop health client will buffer.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Perform one HTTP/1.1 request against `127.0.0.1`.
///
/// # Parameters
/// - `method`: request method token.
/// - `host`: must be `127.0.0.1`.
/// - `port`: loopback port.
/// - `path`: request target, including a leading `/`.
/// - `extra_headers`: additional `Name: value` pairs.
/// - `body`: optional request body.
/// - `timeout`: read/write timeout.
///
/// # Returns
/// The parsed status and body.
pub fn http_request(
    method: &str,
    host: &str,
    port: u16,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<HttpResponse, HttpError> {
    if host != "127.0.0.1" {
        return Err(HttpError::RefusedHost);
    }
    let mut stream = TcpStream::connect((host, port))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n");
    if let Some(bytes) = body {
        req.push_str(&format!("Content-Length: {}\r\n", bytes.len()));
    }
    for (name, value) in extra_headers {
        req.push_str(name);
        req.push_str(": ");
        req.push_str(value);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes())?;
    if let Some(bytes) = body {
        stream.write_all(bytes)?;
    }

    let buf = read_limited(&mut stream, MAX_RESPONSE_BYTES)?;
    parse_http_response(&buf)
}

/// Read at most `max` bytes. Used so a runaway sidecar cannot fill RAM.
///
/// # Parameters
/// - `reader`: response stream.
/// - `max`: inclusive byte cap.
///
/// # Returns
/// The buffered bytes, or [`HttpError::TooLarge`].
pub fn read_limited<R: Read>(reader: &mut R, max: usize) -> Result<Vec<u8>, HttpError> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Ok(buf);
        }
        if buf.len().saturating_add(n) > max {
            return Err(HttpError::TooLarge);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, HttpError> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or(HttpError::InvalidResponse)?;
    let status_line = head.lines().next().ok_or(HttpError::InvalidResponse)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or(HttpError::InvalidResponse)?
        .parse()
        .map_err(|_| HttpError::InvalidResponse)?;
    Ok(HttpResponse {
        status,
        body: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn refuses_non_loopback_host() {
        let err =
            http_request("GET", "0.0.0.0", 80, "/", &[], None, Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, HttpError::RefusedHost));
    }

    #[test]
    fn round_trips_a_local_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nping")
                .unwrap();
        });
        let resp = http_request(
            "GET",
            "127.0.0.1",
            port,
            "/",
            &[],
            None,
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "ping");
        server.join().unwrap();
    }

    #[test]
    fn rejects_oversized_response() {
        let err = read_limited(&mut &b"abcdef"[..], 4).unwrap_err();
        assert!(matches!(err, HttpError::TooLarge));
    }
}
