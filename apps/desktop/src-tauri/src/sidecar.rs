//! Spawn the Node sidecar and wait for its ready line.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::overlay::assert_safe_sidecar_args;
use crate::process::{shutdown_tree, ChildTree};
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
/// - `extra`: additional flags such as `--desktop-token` (Stage B).
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
    for (key, value) in &spec.env {
        command.env(key, value);
    }
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

    let log_err = spec.log_path.clone();
    let stderr_reader = thread::spawn(move || drain_to_log(stderr, &log_err));

    let (tx, rx) = mpsc::channel();
    let log_out = spec.log_path.clone();
    let stdout_reader = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut lines = LoggingLines {
            inner: reader.lines(),
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
            shutdown_done: false,
            #[cfg(windows)]
            job,
        },
        ready: rx,
    })
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
    /// SIGTERM the process group, wait `grace`, then SIGKILL.
    ///
    /// # Parameters
    /// - `grace`: drain window matching the Node process-shutdown contract (5s).
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
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        if !self.shutdown_done {
            self.shutdown(Duration::from_secs(5));
        }
    }
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

fn drain_to_log<R: std::io::Read>(reader: R, log_path: &Path) {
    let buf = BufReader::new(reader);
    for line in buf.lines() {
        let Ok(line) = line else { break };
        let _ = append_log(log_path, &line);
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
}
