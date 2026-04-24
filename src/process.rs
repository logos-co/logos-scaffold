use std::env;
use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::bail;

use crate::model::Captured;
use crate::DynResult;

static ECHO_COMMANDS: AtomicBool = AtomicBool::new(true);

pub(crate) fn set_command_echo(enabled: bool) {
    ECHO_COMMANDS.store(enabled, Ordering::Relaxed);
}

fn should_echo() -> bool {
    ECHO_COMMANDS.load(Ordering::Relaxed)
}

pub(crate) fn render_command(cmd: &Command) -> String {
    let mut out = cmd.get_program().to_string_lossy().to_string();
    for arg in cmd.get_args() {
        out.push(' ');
        out.push_str(&arg.to_string_lossy());
    }
    out
}

pub(crate) fn run_checked(cmd: &mut Command, label: &str) -> DynResult<()> {
    run_forwarded(cmd, label)
}

pub(crate) fn run_forwarded(cmd: &mut Command, label: &str) -> DynResult<()> {
    if should_echo() {
        println!("$ {}", render_command(cmd));
    }
    let status = cmd.status()?;
    if !status.success() {
        bail!("{label} failed with {status}");
    }
    Ok(())
}

/// Set to `true` when `--print-output` or `LOGOS_SCAFFOLD_PRINT_OUTPUT=1` is
/// in effect. When true, `run_logged` falls back to streaming subprocess
/// output directly to the terminal — useful for CI pipelines that already
/// capture structured logs, or for debugging a weird build failure.
static PRINT_OUTPUT: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_print_output(enabled: bool) {
    PRINT_OUTPUT.store(enabled, Ordering::Relaxed);
}

pub(crate) fn print_output_enabled() -> bool {
    PRINT_OUTPUT.load(Ordering::Relaxed)
        || std::env::var_os("LOGOS_SCAFFOLD_PRINT_OUTPUT")
            .map(|v| v != "0" && v != "")
            .unwrap_or(false)
}

/// Run a subprocess with captured output. The user sees:
/// - A single start line: `<step>… (log: <abs-log-path>)`
/// - On success: `  ✓ <step> (<duration>)`
/// - On failure: `  ✗ <step> (<duration>) — see <abs-log-path>`
///
/// Full stdout+stderr is appended to the log file. Under `--print-output`
/// / `LOGOS_SCAFFOLD_PRINT_OUTPUT=1`, falls back to streaming as today.
///
/// Duration is always reported, on both success and failure, per PR #75
/// review finding 4.
pub(crate) fn run_logged(cmd: &mut Command, step: &str, log_path: &Path) -> DynResult<()> {
    if print_output_enabled() {
        return run_forwarded(cmd, step);
    }

    // Single-step path: no session context. Create a one-off session and run.
    let session = BuildSession::new(step, log_path, None);
    let handle = session.step(step);
    handle.run(cmd)
}

/// A session of related nix builds displayed as a checkbox tree in TTY mode
/// and a line-per-update stream in non-TTY mode. Writes full subprocess
/// output to a shared log file under `.scaffold/logs/` and prints a
/// `tip: tail -f <path>` line at session start so the dev can watch.
///
/// Each step shows:
/// - `○` pending
/// - `⋯` in-progress (spinner + elapsed)
/// - `✓ (<duration>)` on success
/// - `✗ (<duration>)` on failure, followed by `error: see <log path>`
///
/// Under `--print-output` / `LOGOS_SCAFFOLD_PRINT_OUTPUT=1` the session falls
/// back to streaming nix output directly — the tree isn't printed.
pub(crate) struct BuildSession {
    log_path: PathBuf,
    multi: Option<indicatif::MultiProgress>,
    // Streaming-direct mode (set via --print-output / env var). When true
    // we skip log capture and the checkbox rendering.
    print_output: bool,
}

impl BuildSession {
    /// Start a new session. If `project_root` is `Some`, the session's log
    /// path is derived under `<project_root>/.scaffold/logs/<ts>-<command>.log`
    /// and rotated. Pass `None` to use `log_path` directly (legacy callers).
    pub(crate) fn new(header: &str, log_path: &Path, project_root: Option<&Path>) -> Self {
        let print_output = print_output_enabled();
        let log_path = log_path.to_path_buf();

        if !print_output {
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // Seed the log file with a header block; later steps append.
            if let Ok(mut f) = File::create(&log_path) {
                use std::io::Write;
                let _ = writeln!(f, "# session: {header}");
                let _ = writeln!(f, "# started: {}", chrono_like_stamp());
                let _ = writeln!(f, "---");
            }
            // Print header + tail -f hint on the user's terminal.
            println!("{header}");
            println!("  tip: tail -f {}", log_path.display());
        }

        // Rotate if project_root given.
        if let Some(pr) = project_root {
            // Extract the command suffix from the log path filename.
            if let Some(stem) = log_path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.split_once('-').map(|(_, rest)| rest))
            {
                rotate_logs(pr, stem, 10);
            }
        }

        let multi = if print_output {
            None
        } else {
            use std::io::IsTerminal;
            if std::io::stderr().is_terminal() {
                Some(indicatif::MultiProgress::new())
            } else {
                None // non-TTY: we'll print line-per-event from within StepHandle
            }
        };

        Self {
            log_path,
            multi,
            print_output,
        }
    }

    /// Begin a step. The returned handle renders a spinner line in TTY mode
    /// (or a `started <description>` line in non-TTY mode). Call `.run(cmd)`
    /// on the handle to execute; the handle finishes automatically on drop.
    pub(crate) fn step(&self, description: &str) -> StepHandle {
        if self.print_output {
            return StepHandle {
                description: description.to_string(),
                start: std::time::Instant::now(),
                log_path: self.log_path.clone(),
                bar: None,
                print_output: true,
                finished: false,
            };
        }

        let bar = if let Some(multi) = &self.multi {
            let pb = multi.add(indicatif::ProgressBar::new_spinner());
            pb.set_style(
                indicatif::ProgressStyle::with_template("  {spinner:.cyan} {wide_msg}")
                    .expect("valid progress template")
                    .tick_strings(&["⋯", "⋯.", "⋯..", "⋯..."]),
            );
            pb.set_message(description.to_string());
            pb.enable_steady_tick(std::time::Duration::from_millis(200));
            Some(pb)
        } else {
            // Non-TTY: print a simple "⋯ <description>" line. Final status
            // line will append on completion.
            println!("  ⋯ {description}");
            None
        };

        StepHandle {
            description: description.to_string(),
            start: std::time::Instant::now(),
            log_path: self.log_path.clone(),
            bar,
            print_output: false,
            finished: false,
        }
    }
}

pub(crate) struct StepHandle {
    description: String,
    start: std::time::Instant,
    log_path: PathBuf,
    bar: Option<indicatif::ProgressBar>,
    print_output: bool,
    finished: bool,
}

impl StepHandle {
    /// Run `cmd`, capturing stdout+stderr to the session log file (except
    /// under `--print-output` where we stream directly). Finalizes the step
    /// with ✓ or ✗ based on exit status.
    pub(crate) fn run(mut self, cmd: &mut Command) -> DynResult<()> {
        if self.print_output {
            // Streaming fallback: echo command + stream output live.
            println!("  running: {}", render_command(cmd));
            let status = cmd.status()?;
            let elapsed = self.start.elapsed();
            self.finished = true;
            if status.success() {
                println!("  ✓ {} ({})", self.description, fmt_duration(elapsed));
                return Ok(());
            }
            println!("  ✗ {} ({})", self.description, fmt_duration(elapsed));
            bail!("{} failed with {status}", self.description);
        }

        // Append subprocess output to the session log.
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| anyhow::anyhow!("open log {}: {e}", self.log_path.display()))?;
        {
            use std::io::Write;
            let mut w = &log;
            let _ = writeln!(w, "\n[step: {}]", self.description);
            let _ = writeln!(w, "# command: {}", render_command(cmd));
        }

        cmd.stdout(Stdio::from(
            log.try_clone()
                .map_err(|e| anyhow::anyhow!("clone log handle: {e}"))?,
        ));
        cmd.stderr(Stdio::from(
            log.try_clone()
                .map_err(|e| anyhow::anyhow!("clone log handle: {e}"))?,
        ));

        let status = cmd.status()?;
        let elapsed = self.start.elapsed();
        self.finished = true;

        let duration = fmt_duration(elapsed);
        if status.success() {
            let msg = format!("✓ {} ({duration})", self.description);
            if let Some(bar) = self.bar.take() {
                bar.set_style(
                    indicatif::ProgressStyle::with_template("  {msg:.green}")
                        .expect("valid template"),
                );
                bar.finish_with_message(msg);
            } else {
                println!("  ✓ {} ({duration})", self.description);
            }
            Ok(())
        } else {
            let msg = format!("✗ {} ({duration})", self.description);
            if let Some(bar) = self.bar.take() {
                bar.set_style(
                    indicatif::ProgressStyle::with_template("  {msg:.red}")
                        .expect("valid template"),
                );
                bar.finish_with_message(msg);
            } else {
                println!("  ✗ {} ({duration})", self.description);
            }
            let mut detail = format!(
                "{} failed with {status}; see {}",
                self.description,
                self.log_path.display()
            );
            if log_indicates_truncated_trace(&self.log_path) {
                detail.push_str(&format!(
                    "\nhint: nix elided part of the eval trace — re-run with --show-trace for full detail: {} --show-trace",
                    render_command(cmd)
                ));
            }
            bail!("{detail}");
        }
    }
}

/// Scan a build log for nix's stack-trace-truncation marker. Only appears on
/// evaluation-time failures when the eval stack exceeds nix's default frame
/// limit (~25). Not present for builder-stage failures, so this check gates
/// the `--show-trace` hint to cases where it actually helps.
fn log_indicates_truncated_trace(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| s.contains("stack trace truncated"))
        .unwrap_or(false)
}

impl Drop for StepHandle {
    fn drop(&mut self) {
        // Belt-and-braces: if the handle drops without `.run()` being called
        // (e.g. caller code path that doesn't invoke a subprocess), clear the
        // pending bar rather than leaving a phantom spinner.
        if !self.finished {
            if let Some(bar) = self.bar.take() {
                bar.finish_and_clear();
            }
        }
    }
}

/// Build a log path `<project_root>/.scaffold/logs/<stamp>-<command>.log`.
/// Caller is responsible for ensuring `project_root` exists.
pub(crate) fn derive_log_path(project_root: &Path, command: &str) -> PathBuf {
    project_root
        .join(".scaffold/logs")
        .join(format!("{}-{}.log", timestamp_compact(), command))
}

/// Delete all but the most recent `keep` log files for a given command prefix
/// (matches on filename suffix `-<command>.log`). No-op if the logs dir
/// doesn't exist yet.
pub(crate) fn rotate_logs(project_root: &Path, command: &str, keep: usize) {
    use std::fs;
    let logs_dir = project_root.join(".scaffold/logs");
    let Ok(entries) = fs::read_dir(&logs_dir) else {
        return;
    };
    let suffix = format!("-{command}.log");
    let mut matching: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with(&suffix))
                .unwrap_or(false)
        })
        .filter_map(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| (t, e.path()))
        })
        .collect();
    matching.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    for (_, path) in matching.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}

fn fmt_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}.{:01}s", secs, d.subsec_millis() / 100)
    } else {
        let m = secs / 60;
        let s = secs % 60;
        format!("{m}m{s:02}s")
    }
}

fn timestamp_compact() -> String {
    // YYYYMMDD-HHMMSS-mmm using the system clock. Millis granularity so two
    // `run_logged` calls completing in the same wall-clock second (warm nix
    // cache, fast builds) don't produce the same filename and clobber each
    // other's log via the subsequent `File::create`.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let (y, mo, d, h, mi, se) = unix_to_ymdhms(now.as_secs());
    let ms = now.subsec_millis();
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{se:02}-{ms:03}")
}

fn chrono_like_stamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let (y, mo, d, h, mi, se) = unix_to_ymdhms(now.as_secs());
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z")
}

/// Minimal unix-epoch → (Y, M, D, h, m, s) conversion (UTC). Handles 1970+
/// for log-naming purposes; we don't need TZ or leap seconds.
fn unix_to_ymdhms(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = secs / 86400;
    let tod = secs % 86400;
    let h = (tod / 3600) as u32;
    let mi = ((tod % 3600) / 60) as u32;
    let se = (tod % 60) as u32;

    // Civil-from-days, Howard Hinnant's algorithm.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (y + if m <= 2 { 1 } else { 0 }) as u32;
    (y, m, d, h, mi, se)
}

#[cfg(test)]
mod logged_tests {
    use super::*;

    #[test]
    fn fmt_duration_short_secs() {
        assert_eq!(fmt_duration(std::time::Duration::from_millis(500)), "0.5s");
        assert_eq!(fmt_duration(std::time::Duration::from_secs(37)), "37.0s");
    }

    #[test]
    fn fmt_duration_minutes() {
        assert_eq!(fmt_duration(std::time::Duration::from_secs(65)), "1m05s");
        assert_eq!(fmt_duration(std::time::Duration::from_secs(3601)), "60m01s");
    }

    #[test]
    fn unix_to_ymdhms_spot_checks() {
        // 2020-01-01 00:00:00 UTC
        assert_eq!(unix_to_ymdhms(1577836800), (2020, 1, 1, 0, 0, 0));
        // 2024-06-15 12:34:56 UTC
        assert_eq!(unix_to_ymdhms(1718454896), (2024, 6, 15, 12, 34, 56));
    }

    #[test]
    fn derive_log_path_uses_scaffold_logs_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let p = derive_log_path(tmp.path(), "setup");
        assert!(p.starts_with(tmp.path().join(".scaffold/logs")));
        assert!(p.to_string_lossy().ends_with("-setup.log"));
    }

    #[test]
    fn derive_log_path_includes_millisecond_suffix_in_stamp() {
        // R-C1: two calls within the same second must produce different
        // filenames. Millis granularity gives us 1000x the headroom before
        // two truly-simultaneous calls collide on File::create truncation.
        let tmp = tempfile::tempdir().unwrap();
        let p = derive_log_path(tmp.path(), "install");
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap();
        // Expected shape: YYYYMMDD-HHMMSS-mmm-<command>
        //   parts[0] = YYYYMMDD, parts[1] = HHMMSS,
        //   parts[2] = mmm,      parts[3] = command
        let parts: Vec<&str> = stem.split('-').collect();
        assert!(
            parts.len() >= 4,
            "expected 4 dash-separated segments, got: {stem}"
        );
        assert_eq!(
            parts[1].len(),
            6,
            "HHMMSS should be 6 chars, got {:?} in {stem}",
            parts[1]
        );
        assert_eq!(
            parts[2].len(),
            3,
            "millis should be 3 digits, got {:?} in {stem}",
            parts[2]
        );
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(parts[3], "install");
    }
}

pub(crate) fn run_capture(cmd: &mut Command, label: &str) -> DynResult<Captured> {
    if should_echo() {
        println!("$ {}", render_command(cmd));
    }
    let Output {
        status,
        stdout,
        stderr,
    } = cmd.output()?;

    let captured = Captured {
        status,
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    };

    if !captured.status.success() {
        bail!("{label} failed: {}", captured.stderr);
    }

    Ok(captured)
}

pub(crate) fn run_with_stdin(mut cmd: Command, input: String) -> DynResult<Captured> {
    if should_echo() {
        println!("$ {}", render_command(&cmd));
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        if let Err(err) = stdin.write_all(input.as_bytes()) {
            if err.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(err.into());
            }
        }
    }
    let out = child.wait_with_output()?;
    Ok(Captured {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

pub(crate) fn spawn_to_log(cmd: &mut Command, log_path: &Path) -> DynResult<u32> {
    if should_echo() {
        println!("$ {}", render_command(cmd));
    }
    let file = File::create(log_path)?;
    let err_file = file.try_clone()?;
    cmd.stdout(Stdio::from(file)).stderr(Stdio::from(err_file));
    let child = cmd.spawn()?;
    Ok(child.id())
}

#[cfg(unix)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn pid_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn pid_is_zombie(pid: u32) -> bool {
    let output = Command::new("ps")
        .arg("-o")
        .arg("stat=")
        .arg("-p")
        .arg(pid.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    let stat = String::from_utf8_lossy(&output.stdout);
    stat.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.starts_with('Z'))
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn pid_is_zombie(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn pid_running(pid: u32) -> bool {
    pid_alive(pid) && !pid_is_zombie(pid)
}

#[cfg(not(unix))]
pub(crate) fn pid_running(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn pid_command(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .arg("-o")
        .arg("command=")
        .arg("-p")
        .arg(pid.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

#[cfg(not(unix))]
pub(crate) fn pid_command(_pid: u32) -> Option<String> {
    None
}

pub(crate) fn port_open(addr: &str) -> bool {
    let parsed: SocketAddr = match addr.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&parsed, Duration::from_millis(500)).is_ok()
}

#[cfg(unix)]
pub(crate) fn listener_pid(port: u16) -> Option<u32> {
    let output = Command::new("lsof")
        .arg("-nP")
        .arg(format!("-iTCP:{port}"))
        .arg("-sTCP:LISTEN")
        .arg("-t")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.trim().parse::<u32>().ok())
}

#[cfg(not(unix))]
pub(crate) fn listener_pid(_port: u16) -> Option<u32> {
    None
}

pub(crate) fn which(binary: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for p in env::split_paths(&paths) {
        let candidate = p.join(binary);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
