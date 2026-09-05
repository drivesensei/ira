//! Detached command runner with captured output, for the bottom "process
//! panel" (`0`). The child runs on its own threads; the UI never blocks on
//! it. Output accumulates in a bounded ring buffer that survives the panel
//! being hidden, so long-running commands (dev servers) keep buffering
//! while the panel is closed.
//!
//! No PTY: the child gets piped stdio, so full-screen TUI apps are out of
//! scope — this is for line-oriented processes (servers, builds, tail).
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;

/// Ring-buffer cap: old lines are dropped when exceeded.
pub const PANEL_BUFFER_LINES: usize = 2000;

/// State of the command running in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Running,
    Exited(u8), // exit code (0 = success)
    Failed,     // could not spawn / crashed
    Killed,     // stopped via the stop keybind
}

/// Shared state between the UI thread and the pump threads.
pub struct CommandRun {
    /// Last N output lines (stdout and stderr interleaved as they arrive).
    buffer: Mutex<VecDeque<String>>,
    /// Whether output changed since the last UI read (avoids needless redraw).

    /// Child stdin (line mode: commands read plain lines; Ctrl+C is sent as
    /// a raw 0x03 byte).
    stdin: Mutex<Option<std::process::ChildStdin>>,
    /// Child PID, for `kill -INT` on Unix.
    pub pid: u32,
    state: Mutex<RunState>,
    /// Set when the child has fully exited (wait() returned on the reaper).
    finished: Arc<AtomicBool>,
}

impl CommandRun {
    fn new(pid: u32) -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new(VecDeque::with_capacity(PANEL_BUFFER_LINES)),
            stdin: Mutex::new(None),
            pid,
            state: Mutex::new(RunState::Running),
            finished: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Snapshot of up to `rows` most recent lines, oldest first.
    pub fn tail(&self, rows: usize) -> Vec<String> {
        let buf = self.buffer.lock();
        let skip = buf.len().saturating_sub(rows);
        buf.iter().skip(skip).cloned().collect()
    }

    pub fn state(&self) -> RunState {
        *self.state.lock()
    }

    pub fn finished(&self) -> bool {
        self.finished.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn push_line(&self, line: String) {
        let mut buf = self.buffer.lock();
        if buf.len() >= PANEL_BUFFER_LINES {
            buf.pop_front();
        }
        buf.push_back(line);
    }

    /// Sends one input line to the child's stdin (LF-terminated).
    pub fn send_line(&self, line: &str) {
        let mut guard = self.stdin.lock();
        if let Some(stdin) = guard.as_mut() {
            let _ = writeln!(stdin, "{line}");
            let _ = stdin.flush();
        }
    }

    /// Sends the raw Ctrl+C byte (0x03) to the child's stdin — many dev
    /// servers (node, npm run dev) interpret it as an interrupt request.
    pub fn send_ctrl_c_byte(&self) {
        let mut guard = self.stdin.lock();
        if let Some(stdin) = guard.as_mut() {
            let _ = stdin.write_all(&[0x03]);
            let _ = stdin.flush();
        }
    }

    /// User-facing stop alias.
    pub fn stop(&self) {
        self.request_stop();
    }

    /// OS-level stop: on Unix sends SIGINT via `kill` (std has no kill API);
    /// on Windows uses taskkill. Runs as a detached command so the UI never
    /// blocks. Falls back to nothing if the process already exited.
    pub fn request_stop(&self) {
        *self.state.lock() = RunState::Killed;
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill")
                .args(["-INT", &self.pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &self.pid.to_string(), "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        // The reaper thread will flip `state` to Killed/Exited on wait().
    }
}

/// Spawns `program args` in `cwd` with all stdio piped; output is pumped
/// into a shared ring buffer by two threads. Returns the shared handle.
/// The child runs detached from the caller: spawn never blocks on it.
pub fn spawn_command(program: &str, args: &[&str], cwd: &str) -> Result<Arc<CommandRun>, String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run '{program}': {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = child.stdin.take();
    let run = CommandRun::new(child.id());
    {
        let mut g = run.stdin.lock();
        *g = stdin;
    }

    // stdout pump
    if let Some(out) = stdout {
        let run = Arc::clone(&run);
        thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                run.push_line(strip_ansi(&line));
            }
        });
    }
    // stderr pump
    if let Some(err) = stderr {
        let run = Arc::clone(&run);
        thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                run.push_line(strip_ansi(&line));
            }
        });
    }

    // Reaper: waits for the child on its own thread so the UI never does.
    let run_reaper = Arc::clone(&run);
    thread::spawn(move || {
        let status = child.wait();
        let run = run_reaper;
        run.finished
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let new_state = match status {
            Ok(s) => {
                if *run.state.lock() == RunState::Killed {
                    RunState::Killed
                } else {
                    RunState::new_from_exit(s)
                }
            }
            Err(_) => RunState::Failed,
        };
        *run.state.lock() = new_state;
    });

    Ok(run)
}

impl RunState {
    fn new_from_exit(s: std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(sig) = s.signal() {
                if sig == 2 {
                    return RunState::Killed; // SIGINT = our stop
                }
                return RunState::Failed;
            }
        }
        RunState::Exited(s.code().unwrap_or(1).clamp(0, 255) as u8)
    }
}

/// Removes ANSI escape sequences (colors/cursor moves) from a line. Tools
/// that colorize when piped would otherwise print garbage into the panel.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.clone().next() {
                // CSI: parameters/intermediates then a final alphabetic byte.
                Some('[') => {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                // Any other escape form (e.g. \x1bK, \x1bc): consume the
                // single following byte.
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn strips_ansi_sequences() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\x1b[1;34mnavy\x1b[m text"), "navy text");
        assert_eq!(strip_ansi("no\x1bKcodes"), "nocodes");
    }

    #[test]
    fn runs_command_and_captures_output() {
        let run = spawn_command("echo", &["hello-panel"], "/tmp").unwrap();
        let mut got = false;
        for _ in 0..100 {
            if !run.tail(10).is_empty() {
                got = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(got, "stdout must reach the buffer");
        let lines = run.tail(10);
        assert!(lines.iter().any(|l| l.contains("hello-panel")));
    }

    #[test]
    fn reports_failure_for_missing_program() {
        let result = spawn_command("/nonexistent-cmd-xyz", &[], "/tmp");
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("missing program must fail to spawn"),
        };
        assert!(err.contains("Failed to run"), "{err}");
    }

    #[test]
    fn ring_buffer_caps_at_limit() {
        // 3000 echoes of one line through sh: buffer must hold only the
        // last PANEL_BUFFER_LINES.
        let script = "for i in $(seq 1 3000); do echo line-$i; done";
        let run = spawn_command("sh", &["-c", script], "/tmp").unwrap();
        for _ in 0..500 {
            if run.finished() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(run.finished(), "3000 echoes must finish");
        let lines = run.tail(usize::MAX);
        assert!(lines.len() <= PANEL_BUFFER_LINES);
        assert!(lines.iter().any(|l| l.contains("line-3000")));
        assert!(!lines.iter().any(|l| l.contains("line-1\n")));
    }
}
