//! Ready-line parsing for the web sidecar supervisor signal.

use std::io;
use std::time::Instant;

/// Prefix printed by `dsh web` after Loader settlement.
const READY_PREFIX: &str = "dsh web: http://127.0.0.1:";

/// Failure while waiting for the sidecar ready line.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReadyError {
    /// No matching line arrived before the deadline.
    #[error("sidecar ready line timed out")]
    Timeout,
    /// Stdout closed before a ready line appeared.
    #[error("sidecar stdout closed before the ready line")]
    StdoutClosed,
    /// A read from the sidecar stdout failed.
    #[error("sidecar stdout read failed")]
    Io,
}

/// Parse one stdout line for `dsh web: http://127.0.0.1:<port>`.
///
/// The optional LAN suffix is ignored. Port `0` is rejected.
///
/// # Parameters
/// - `line`: one stdout line, with or without trailing whitespace.
///
/// # Returns
/// The bound loopback port, or `None` when the line is not a ready signal.
pub fn parse_ready_line(line: &str) -> Option<u16> {
    let rest = line.trim().strip_prefix(READY_PREFIX)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok().filter(|port| *port > 0)
}

/// Read lines until a ready port appears, the iterator ends, or `deadline` passes.
///
/// # Parameters
/// - `lines`: stdout lines from the sidecar.
/// - `deadline`: exclusive timeout instant.
/// - `now`: clock used to compare against `deadline` (injectable in tests).
///
/// # Returns
/// The parsed port, or a [`ReadyError`].
pub fn wait_for_ready_line<I, C>(mut lines: I, deadline: Instant, now: C) -> Result<u16, ReadyError>
where
    I: Iterator<Item = io::Result<String>>,
    C: Fn() -> Instant,
{
    loop {
        if now() >= deadline {
            return Err(ReadyError::Timeout);
        }
        match lines.next() {
            None => {
                return Err(if now() >= deadline {
                    ReadyError::Timeout
                } else {
                    ReadyError::StdoutClosed
                });
            }
            Some(Err(_)) => return Err(ReadyError::Io),
            Some(Ok(line)) => {
                if let Some(port) = parse_ready_line(&line) {
                    return Ok(port);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parses_plain_ready_line() {
        assert_eq!(
            parse_ready_line("dsh web: http://127.0.0.1:61064"),
            Some(61064)
        );
    }

    #[test]
    fn parses_ready_line_with_lan_suffix() {
        assert_eq!(
            parse_ready_line("dsh web: http://127.0.0.1:4321 (LAN: http://192.168.1.5:4321)"),
            Some(4321)
        );
    }

    #[test]
    fn rejects_non_ready_and_port_zero() {
        assert_eq!(parse_ready_line("booting"), None);
        assert_eq!(parse_ready_line("dsh web: http://0.0.0.0:8080"), None);
        assert_eq!(parse_ready_line("dsh web: http://127.0.0.1:0"), None);
    }

    #[test]
    fn waits_through_noise_then_matches() {
        let start = Instant::now();
        let lines = ["info boot", "dsh web: http://127.0.0.1:3456"]
            .into_iter()
            .map(|line| Ok(line.to_string()));
        assert_eq!(
            wait_for_ready_line(lines, start + Duration::from_secs(15), || start).unwrap(),
            3456
        );
    }

    #[test]
    fn times_out_when_deadline_has_passed() {
        let start = Instant::now();
        let deadline = start;
        let now = || start + Duration::from_secs(1);
        let lines = std::iter::once(Ok("still booting".to_string()));
        assert_eq!(
            wait_for_ready_line(lines, deadline, now),
            Err(ReadyError::Timeout)
        );
    }

    #[test]
    fn reports_closed_stdout_before_ready() {
        let start = Instant::now();
        let lines = std::iter::empty();
        assert_eq!(
            wait_for_ready_line(lines, start + Duration::from_secs(15), || start),
            Err(ReadyError::StdoutClosed)
        );
    }
}
