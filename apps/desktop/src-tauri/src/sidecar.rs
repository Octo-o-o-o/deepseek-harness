//! Spawn the Node sidecar and wait for its ready line.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::env::apply_sidecar_env;
use crate::overlay::assert_safe_sidecar_args;
use crate::process::{hide_child_console, shutdown_tree, ChildTree};
use crate::ready::{wait_for_ready_line, ReadyError};

/// How the sidecar is launched.
#[derive(Debug, Clone)]
pub struct SidecarSpec {
    /// Node (or test double) executable.
    pub program: PathBuf,
    /// Arguments after the program (script + `web --port 0 --host 127.0.0.1` + extras).
    pub args: Vec<String>,
    /// Child working directory (workspace cwd contract).
    pub cwd: PathBuf,
    /// Extra environment pairs (`DSH_HOME` is always injected by the caller).
    pub env: Vec<(String, String)>,
    /// Sidecar stdout/stderr log file.
    pub log_path: PathBuf,
}

/// A live sidecar plus a oneshot ready-port receiver.
pub struct SpawnedSidecar {
    /// Child process (owns the process group on Unix).
    pub process: SidecarProcess,
    ready: mpsc::Receiver<Result<u16, ReadyError>>,
}

/// Owned sidecar child.
pub struct SidecarProcess {
    child: Child,
    readers: Vec<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    #[cfg(windows)]
    cancels: Vec<crate::job::CancelHandle>,
    shutdown_done: bool,
    #[cfg(windows)]
    job: Option<crate::job::JobObject>,
}

/// Failure to spawn or supervise the sidecar.
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    /// Overlay rejected the argv.
    #[error(transparent)]
    Overlay(#[from] crate::overlay::OverlayError),
    /// `Command::spawn` failed.
    #[error("failed to spawn sidecar: {0}")]
    Spawn(std::io::Error),
    /// Ready-line wait failed.
    #[error(transparent)]
    Ready(#[from] ReadyError),
    /// Log file could not be created.
    #[error("failed to open sidecar log: {0}")]
    Log(std::io::Error),
}

/// Build the production `dsh web --port 0 --host 127.0.0.1` argument list.
///
/// # Parameters
/// - `bin_js`: absolute path to `apps/cli/lib/bin.js`.
/// - `extra`: extra argv after the pinned loopback flags (secrets go in env, not here).
///
/// # Returns
/// Argv after the `node` program.
pub fn desktop_web_args(bin_js: &Path, extra: &[String]) -> Vec<String> {
    let mut args = vec![
        bin_js.to_string_lossy().into_owned(),
        "web".into(),
        "--port".into(),
        "0".into(),
        "--host".into(),
        "127.0.0.1".into(),
    ];
    args.extend(extra.iter().cloned());
    args
}

/// Spawn the sidecar, tee stdio to `log_path`, and begin ready-line parsing.
///
/// # Parameters
/// - `spec`: launch specification. Argv is overlay-checked before spawn.
///
/// # Returns
/// A live process and a receiver that yields the first ready port.
pub fn spawn_sidecar(spec: &SidecarSpec) -> Result<SpawnedSidecar, SidecarError> {
    assert_safe_sidecar_args(&spec.args)?;
    if let Some(parent) = spec.log_path.parent() {
        fs::create_dir_all(parent).map_err(SidecarError::Log)?;
    }
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_sidecar_env(&mut command, &spec.env);
    hide_child_console(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(SidecarError::Spawn)?;
    #[cfg(windows)]
    let job = match crate::job::JobObject::create().and_then(|job| {
        job.assign(&child)?;
        Ok(job)
    }) {
        Ok(job) => Some(job),
        Err(err) => {
            eprintln!("desktop: Job Object assign failed (not locally verified): {err}");
            None
        }
    };
    let stdout = child.stdout.take().ok_or_else(|| {
        SidecarError::Spawn(std::io::Error::other("sidecar stdout was not piped"))
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        SidecarError::Spawn(std::io::Error::other("sidecar stderr was not piped"))
    })?;

    let stop = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        set_nonblocking(&stdout);
        set_nonblocking(&stderr);
    }
    #[cfg(windows)]
    let cancels: Vec<crate::job::CancelHandle> = {
        use std::os::windows::io::AsRawHandle as _;
        [
            crate::job::CancelHandle::duplicate(stdout.as_raw_handle()),
            crate::job::CancelHandle::duplicate(stderr.as_raw_handle()),
        ]
        .into_iter()
        .flatten()
        .collect()
    };

    let log_err = spec.log_path.clone();
    let stderr_stop = Arc::clone(&stop);
    let stderr_reader = thread::spawn(move || drain_to_log(stderr, &log_err, stderr_stop));

    let (tx, rx) = mpsc::channel();
    let log_out = spec.log_path.clone();
    let stdout_stop = Arc::clone(&stop);
    let stdout_reader = thread::spawn(move || {
        let lines = StoppableLines::new(BufReader::new(stdout), stdout_stop);
        let mut lines = LoggingLines {
            inner: lines,
            log_path: log_out.clone(),
        };
        let result = wait_for_ready_line(
            &mut lines,
            Instant::now() + Duration::from_secs(60 * 60),
            Instant::now,
        );
        let _ = tx.send(result);
        lines.drain_rest();
    });

    Ok(SpawnedSidecar {
        process: SidecarProcess {
            child,
            readers: vec![stderr_reader, stdout_reader],
            stop,
            #[cfg(windows)]
            cancels,
            shutdown_done: false,
            #[cfg(windows)]
            job,
        },
        ready: rx,
    })
}

/// Put a pipe into non-blocking mode so readers can poll a stop flag.
///
/// Failure is non-fatal: the reader stays blocking and shutdown falls back
/// to the bounded join.
#[cfg(unix)]
fn set_nonblocking<F: std::os::unix::io::AsRawFd>(file: &F) {
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    // SAFETY: fcntl on a live pipe descriptor; flags preserved on failure.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            eprintln!(
                "desktop: failed to set sidecar pipe non-blocking: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

impl SpawnedSidecar {
    /// Split the live child from the ready-port receiver so the child can be stored first.
    ///
    /// # Returns
    /// The process and the oneshot-like ready channel.
    pub fn into_parts(self) -> (SidecarProcess, mpsc::Receiver<Result<u16, ReadyError>>) {
        (self.process, self.ready)
    }
}

/// Block until the ready line arrives or `timeout` elapses.
///
/// # Parameters
/// - `ready`: receiver produced by [`SpawnedSidecar::into_parts`].
/// - `timeout`: maximum wait.
///
/// # Returns
/// The loopback port from the ready line.
pub fn wait_ready(
    ready: &mpsc::Receiver<Result<u16, ReadyError>>,
    timeout: Duration,
) -> Result<u16, ReadyError> {
    match ready.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ReadyError::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ReadyError::StdoutClosed),
    }
}

impl SidecarProcess {
    /// Sidecar process id for `sidecar.pid`.
    ///
    /// # Returns
    /// The OS pid of the Node process.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Pid-reuse-proof identity for the pid file: the process creation time
    /// on Windows. Unix records keep command-line identity via `ps`.
    ///
    /// # Returns
    /// The creation-time token on Windows, `None` elsewhere or on failure.
    #[cfg(windows)]
    pub fn start_token(&self) -> Option<u64> {
        crate::job::child_creation_time(&self.child)
    }

    #[cfg(not(windows))]
    pub fn start_token(&self) -> Option<u64> {
        None
    }

    /// Non-consuming exit poll for the supervisor's exit watcher.
    ///
    /// # Returns
    /// The exit status once the child is observed dead, `None` while running.
    pub fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status),
            Ok(None) | Err(_) => None,
        }
    }

    /// Stop the sidecar tree: terminate, escalate to a forced kill while the
    /// tree (Unix process group) is still alive within `grace`, then stop the
    /// log readers. Windows terminates through the Job Object immediately —
    /// there is no drain window; Unix drains for `grace` before SIGKILL.
    ///
    /// # Parameters
    /// - `grace`: Unix drain window before a forced group kill.
    pub fn shutdown(&mut self, grace: Duration) {
        if self.shutdown_done {
            return;
        }
        self.shutdown_done = true;
        let mut tree = ChildTree {
            child: &mut self.child,
            #[cfg(windows)]
            job: self.job.as_ref(),
        };
        shutdown_tree(&mut tree, grace, Instant::now, thread::sleep);
        self.stop.store(true, Ordering::SeqCst);
        #[cfg(windows)]
        for cancel in &self.cancels {
            cancel.cancel();
        }
        join_with_deadline(self.readers.drain(..).collect(), Duration::from_secs(2));
    }
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        if !self.shutdown_done {
            self.shutdown(Duration::from_secs(5));
        }
    }
}

/// Join reader threads with a hard deadline.
///
/// Readers are cancellable (stop flag plus `CancelIoEx`/non-blocking pipes),
/// so the deadline only fires when cancellation itself failed; the stuck
/// thread is then left behind rather than hanging shutdown.
fn join_with_deadline(readers: Vec<thread::JoinHandle<()>>, deadline: Duration) {
    if readers.is_empty() {
        return;
    }
    let (done, waited) = mpsc::channel();
    thread::spawn(move || {
        for reader in readers {
            let _ = reader.join();
        }
        let _ = done.send(());
    });
    if waited.recv_timeout(deadline).is_err() {
        eprintln!("desktop: sidecar log readers did not stop; leaving them behind");
    }
}

/// Line iterator over a pipe that either yields a line, hits EOF, or is
/// stopped by a shutdown flag.
///
/// Unix pipes are non-blocking, so `WouldBlock` means "no line yet": poll the
/// stop flag and retry. Windows pipes block, so an error after `cancel()`
/// unwinds through the stop-flag check. Partial lines survive retries.
struct StoppableLines<R: std::io::Read> {
    reader: BufReader<R>,
    stop: Arc<AtomicBool>,
    partial: Vec<u8>,
}

impl<R: std::io::Read> StoppableLines<R> {
    fn new(reader: BufReader<R>, stop: Arc<AtomicBool>) -> Self {
        Self {
            reader,
            stop,
            partial: Vec::new(),
        }
    }
}

impl<R: std::io::Read> Iterator for StoppableLines<R> {
    type Item = std::io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return None;
            }
            match self.reader.read_until(b'\n', &mut self.partial) {
                Ok(0) => {
                    if self.partial.is_empty() {
                        return None;
                    }
                    return Some(Ok(take_line(&mut self.partial)));
                }
                Ok(_) => {
                    if self.partial.last() == Some(&b'\n') {
                        return Some(Ok(take_line(&mut self.partial)));
                    }
                    // EOF flush without a trailing newline: the next call
                    // returns Ok(0) and takes the remainder.
                    continue;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                    continue;
                }
                Err(err) => {
                    if self.stop.load(Ordering::Relaxed) {
                        return None;
                    }
                    return Some(Err(err));
                }
            }
        }
    }
}

/// Take `partial` as one log line, trimming the LF/CRLF the way `lines()` does.
fn take_line(partial: &mut Vec<u8>) -> String {
    let bytes = std::mem::take(partial);
    let mut line = String::from_utf8_lossy(&bytes).into_owned();
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

struct LoggingLines<I> {
    inner: I,
    log_path: PathBuf,
}

impl<I> Iterator for LoggingLines<I>
where
    I: Iterator<Item = std::io::Result<String>>,
{
    type Item = std::io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next();
        if let Some(Ok(line)) = &item {
            let _ = append_log(&self.log_path, line);
        }
        item
    }
}

impl<I> LoggingLines<I>
where
    I: Iterator<Item = std::io::Result<String>>,
{
    fn drain_rest(&mut self) {
        for item in self.inner.by_ref() {
            match item {
                Ok(line) => {
                    let _ = append_log(&self.log_path, &line);
                }
                Err(_) => break,
            }
        }
    }
}

fn drain_to_log<R: std::io::Read>(reader: R, log_path: &Path, stop: Arc<AtomicBool>) {
    for item in StoppableLines::new(BufReader::new(reader), stop) {
        match item {
            Ok(line) => {
                let _ = append_log(log_path, &line);
            }
            Err(_) => break,
        }
    }
}

fn append_log(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::Read;

    fn temp_log() -> PathBuf {
        env::temp_dir().join(format!("dsh-sidecar-test-{}.log", std::process::id()))
    }

    fn which_node() -> Option<PathBuf> {
        let path = env::var_os("PATH")?;
        for dir in env::split_paths(&path) {
            let candidate = dir.join("node");
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let exe = dir.join("node.exe");
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
        None
    }

    #[test]
    fn desktop_args_pin_loopback() {
        let args = desktop_web_args(Path::new("/tmp/bin.js"), &[]);
        assert_eq!(
            args,
            vec![
                "/tmp/bin.js".to_string(),
                "web".into(),
                "--port".into(),
                "0".into(),
                "--host".into(),
                "127.0.0.1".into(),
            ]
        );
        assert!(assert_safe_sidecar_args(&args).is_ok());
    }

    #[test]
    fn spawn_fake_sidecar_reports_ready_then_shuts_down() {
        let Some(node) = which_node() else {
            return;
        };
        let dir = env::temp_dir();
        let script = dir.join(format!("dsh-fake-sidecar-{}.js", std::process::id()));
        fs::write(
            &script,
            "console.log('dsh web: http://127.0.0.1:34567');\nsetInterval(() => {}, 1000);\n",
        )
        .unwrap();
        let log_path = temp_log();
        let spec = SidecarSpec {
            program: node,
            args: vec![
                script.to_string_lossy().into_owned(),
                "--host".into(),
                "127.0.0.1".into(),
            ],
            cwd: dir,
            env: vec![],
            log_path: log_path.clone(),
        };
        let spawned = spawn_sidecar(&spec).expect("spawn fake sidecar");
        let (mut process, ready) = spawned.into_parts();
        let port = wait_ready(&ready, Duration::from_secs(5)).unwrap_or_else(|err| {
            let log = fs::read_to_string(&log_path).unwrap_or_default();
            panic!("ready line: {err}; log={log}");
        });
        assert_eq!(port, 34567);
        process.shutdown(Duration::from_secs(2));
        let _ = fs::remove_file(script);
    }

    /// A reader that reports WouldBlock once, then serves a line, then EOF.
    struct GatedReader {
        step: usize,
    }

    impl Read for GatedReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.step += 1;
            match self.step {
                1 => Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "no line yet",
                )),
                2 => {
                    buf[..6].copy_from_slice(b"hello\n");
                    Ok(6)
                }
                _ => Ok(0),
            }
        }
    }

    #[test]
    fn stoppable_lines_retry_through_would_block() {
        let stop = Arc::new(AtomicBool::new(false));
        let mut lines = StoppableLines::new(BufReader::new(GatedReader { step: 0 }), stop);
        assert_eq!(lines.next().unwrap().unwrap(), "hello");
        assert!(lines.next().is_none());
    }

    #[test]
    fn stoppable_lines_stop_wins_over_pending_data() {
        let stop = Arc::new(AtomicBool::new(true));
        let mut lines = StoppableLines::new(BufReader::new(GatedReader { step: 0 }), stop);
        assert!(lines.next().is_none());
    }

    #[test]
    fn join_with_deadline_leaves_a_stuck_reader_behind() {
        let started = Instant::now();
        let stuck = thread::spawn(|| loop {
            thread::sleep(Duration::from_secs(60));
        });
        join_with_deadline(vec![stuck], Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
