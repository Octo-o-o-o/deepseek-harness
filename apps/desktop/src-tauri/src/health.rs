//! Loader-ready probe: GET `/` must be 200 and contain `__DSH_BOOT__`.

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
}
