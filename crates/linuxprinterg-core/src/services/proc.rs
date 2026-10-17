//! Безопасный запуск внешних команд.
//!
//! Принципы:
//! * без shell — только вектор аргументов (защита от инъекций);
//! * жёсткий таймаут, убийство процесса по истечении времени;
//! * отмена операции пользователем ([`CancelHandle`]);
//! * `LC_ALL=C` для детерминированного разбора вывода;
//! * дочерние потоки читают stdout/stderr, чтобы избежать дедлока на pipe.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// Результат выполнения команды.
#[derive(Debug, Clone)]
pub struct CmdOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl CmdOutput {
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Ошибка, если код возврата не равен 0.
    pub fn ensure_ok(self, tool: &str) -> Result<Self> {
        if self.status == 0 {
            Ok(self)
        } else {
            Err(Error::ToolFailed {
                tool: tool.to_string(),
                code: self.status,
                message: self.stderr.trim().to_string(),
            })
        }
    }
}

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    /// Ссылка на текущего ребёнка активного запуска (ослабленная, без циклов).
    child: Mutex<std::sync::Weak<Mutex<Option<Child>>>>,
}

/// Удостоверение отмены: клонируется и передаётся в UI, вызывает `cancel()`.
#[derive(Clone, Default)]
pub struct CancelHandle(Arc<CancelInner>);

impl CancelHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Запросить отмену: убить текущий процесс (если он есть).
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        let weak = self.0.child.lock().map(|g| g.clone()).unwrap_or_default();
        if let Some(shared) = weak.upgrade() {
            if let Ok(mut guard) = shared.lock() {
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                }
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Сброс состояния перед повторным использованием.
    pub fn reset(&self) {
        self.0.cancelled.store(false, Ordering::SeqCst);
    }

    fn attach(&self, shared: &Arc<Mutex<Option<Child>>>) {
        if let Ok(mut guard) = self.0.child.lock() {
            *guard = Arc::downgrade(shared);
        }
    }

    fn detach(&self) {
        if let Ok(mut guard) = self.0.child.lock() {
            *guard = std::sync::Weak::new();
        }
    }
}

/// Поиск исполняемого файла в `PATH` (или прямой путь, если указан `/`).
pub fn tool_available(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable_file(&p).then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(p: &Path) -> bool {
    p.is_file()
}

/// Запустить команду и дождаться результата с таймаутом.
pub fn run<S: AsRef<OsStr>>(cmd: &str, args: &[S], timeout: Duration) -> Result<CmdOutput> {
    run_inner(cmd, args, timeout, None)
}

/// Запустить команду с возможностью отмены.
pub fn run_cancellable<S: AsRef<OsStr>>(
    cmd: &str,
    args: &[S],
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<CmdOutput> {
    cancel.reset();
    run_inner(cmd, args, timeout, Some(cancel))
}

fn run_inner<S: AsRef<OsStr>>(
    cmd: &str,
    args: &[S],
    timeout: Duration,
    cancel: Option<&CancelHandle>,
) -> Result<CmdOutput> {
    let program = tool_available(cmd).ok_or_else(|| Error::ToolMissing {
        tool: cmd.to_string(),
    })?;

    let mut child = Command::new(&program)
        .args(args)
        // Детерминированный локально-независимый вывод утилит CUPS/SANE.
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::io(format!("запуск «{cmd}»"), e))?;

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let out_reader = stdout_pipe.take().map(|mut p| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = p.read_to_end(&mut buf);
            buf
        })
    });
    let err_reader = stderr_pipe.take().map(|mut p| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = p.read_to_string(&mut buf);
            buf
        })
    });

    let shared: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));
    if let Some(c) = cancel {
        if c.is_cancelled() {
            kill_and_wait(&shared);
            drain(out_reader, err_reader);
            return Err(Error::Cancelled);
        }
        c.attach(&shared);
    }

    let started = Instant::now();
    let status = loop {
        if cancel.map(|c| c.is_cancelled()).unwrap_or(false) {
            kill_and_wait(&shared);
            if let Some(c) = cancel {
                c.detach();
            }
            drain(out_reader, err_reader);
            return Err(Error::Cancelled);
        }
        if started.elapsed() > timeout {
            kill_and_wait(&shared);
            if let Some(c) = cancel {
                c.detach();
            }
            drain(out_reader, err_reader);
            return Err(Error::Timeout {
                tool: cmd.to_string(),
                secs: timeout.as_secs(),
            });
        }
        let status = {
            let mut guard = shared.lock().map_err(|_| Error::msg("ошибка блокировки"))?;
            match guard.as_mut() {
                Some(ch) => match ch.try_wait() {
                    Ok(Some(status)) => Some(status),
                    Ok(None) => None,
                    Err(e) => {
                        drop(guard);
                        kill_and_wait(&shared);
                        if let Some(c) = cancel {
                            c.detach();
                        }
                        drain(out_reader, err_reader);
                        return Err(Error::io(format!("ожидание «{cmd}»"), e));
                    }
                },
                // Ребёнок убит другой стороной (cancel): завершаем по флагу.
                None => None,
            }
        };
        if let Some(status) = status {
            if let Some(c) = cancel {
                c.detach();
            }
            break status;
        }
        std::thread::sleep(Duration::from_millis(25));
    };

    let stdout = out_reader.and_then(|t| t.join().ok()).unwrap_or_default();
    let stderr = err_reader.and_then(|t| t.join().ok()).unwrap_or_default();

    Ok(CmdOutput {
        status: status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

fn kill_and_wait(shared: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut guard) = shared.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn drain(
    out: Option<std::thread::JoinHandle<Vec<u8>>>,
    err: Option<std::thread::JoinHandle<String>>,
) {
    if let Some(t) = out {
        let _ = t.join();
    }
    if let Some(t) = err {
        let _ = t.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_lookup_finds_sh() {
        assert!(tool_available("sh").is_some());
        assert!(tool_available("definitely-not-a-real-tool-xyz").is_none());
    }

    #[test]
    fn runs_command_and_captures_output() {
        let out = run("sh", &["-c", "printf hello"], Duration::from_secs(5)).unwrap();
        assert_eq!(out.status, 0);
        assert_eq!(out.stdout, b"hello");
    }

    #[test]
    fn timeout_kills_process() {
        let err = run("sleep", &["5"], Duration::from_millis(100)).unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    }

    #[test]
    fn cancellation_kills_process() {
        let h = CancelHandle::new();
        let h2 = h.clone();
        let t = std::thread::spawn(move || {
            run_cancellable("sleep", &["5"], Duration::from_secs(30), &h2)
        });
        std::thread::sleep(Duration::from_millis(200));
        h.cancel();
        let err = t.join().unwrap().unwrap_err();
        assert!(err.is_cancelled(), "{err:?}");
    }

    #[test]
    fn missing_tool_reports_error() {
        let err = run("no-such-tool-abc", &["x"], Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, Error::ToolMissing { .. }));
    }
}
