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
    /// The peer announced `Transfer-Encoding: chunked` but the framing is malformed or truncated.
    #[error("invalid chunked response body")]
    InvalidChunkedBody,
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

/// Split the head from the body on the raw bytes, so chunk sizes stay aligned
/// with the wire even when the body carries multi-byte UTF-8.
fn split_head(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| (&raw[..at], &raw[at + 4..]))
}

/// Whether the head declares `Transfer-Encoding: chunked`.
///
/// The sidecar's Node server omits `Content-Length` and chunks every `/api`
/// and index response, so this is the normal case, not an edge case.
fn is_chunked(head: &str) -> bool {
    head.lines().skip(1).any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.trim().eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("chunked"))
    })
}

/// Concatenate the chunk payloads of an RFC 9112 chunked body.
///
/// Chunk extensions after the size are ignored; a trailer section is not
/// parsed because the reply is complete at the terminating zero-size chunk.
fn decode_chunked(body: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let line_end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or(HttpError::InvalidChunkedBody)?;
        let header =
            std::str::from_utf8(&rest[..line_end]).map_err(|_| HttpError::InvalidChunkedBody)?;
        let size_text = header.split(';').next().unwrap_or(header).trim();
        let size =
            usize::from_str_radix(size_text, 16).map_err(|_| HttpError::InvalidChunkedBody)?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size + 2 {
            return Err(HttpError::InvalidChunkedBody);
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, HttpError> {
    let (head_bytes, body_bytes) = split_head(raw).ok_or(HttpError::InvalidResponse)?;
    let head = String::from_utf8_lossy(head_bytes);
    let status_line = head.lines().next().ok_or(HttpError::InvalidResponse)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or(HttpError::InvalidResponse)?
        .parse()
        .map_err(|_| HttpError::InvalidResponse)?;
    let body = if is_chunked(&head) {
        decode_chunked(body_bytes)?
    } else {
        body_bytes.to_vec()
    };
    Ok(HttpResponse {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
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

    /// The sidecar chunks every reply; an undecoded body is not parseable JSON.
    #[test]
    fn decodes_a_chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"a\":\r\n3\r\n1}\x20\r\n0\r\n\r\n";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "{\"a\":1} ");
    }

    #[test]
    fn chunk_sizes_count_bytes_not_characters() {
        // "中" is three bytes; a char-indexed decoder would truncate it.
        let raw = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n中ok\r\n0\r\n\r\n"
            .as_bytes();
        assert_eq!(parse_http_response(raw).unwrap().body, "中ok");
    }

    #[test]
    fn ignores_chunk_extensions_and_accepts_uppercase_header() {
        let raw =
            b"HTTP/1.1 200 OK\r\nTRANSFER-ENCODING: Chunked\r\n\r\n2;name=value\r\nok\r\n0\r\n\r\n";
        assert_eq!(parse_http_response(raw).unwrap().body, "ok");
    }

    #[test]
    fn rejects_truncated_chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\nshort\r\n";
        assert!(matches!(
            parse_http_response(raw),
            Err(HttpError::InvalidChunkedBody)
        ));
    }

    #[test]
    fn keeps_identity_bodies_untouched() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nping";
        assert_eq!(parse_http_response(raw).unwrap().body, "ping");
    }
}
