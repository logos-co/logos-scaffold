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
    use std::fs;
    use std::io::Write;
    use std::time::Instant;

    if print_output_enabled() {
        return run_forwarded(cmd, step);
    }

    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("create log dir {}: {e}", parent.display()))?;
    }

    // Log file header: timestamp + step + command argv + cwd. Self-describing
    // so someone looking at the file later can tell what it represents.
    let mut log = File::create(log_path)
        .map_err(|e| anyhow::anyhow!("open log {}: {e}", log_path.display()))?;
    let _ = writeln!(log, "# step: {step}");
    let _ = writeln!(log, "# command: {}", render_command(cmd));
    let _ = writeln!(log, "# started: {}", chrono_like_stamp());
    let _ = writeln!(log, "---");

    println!("{step}… (log: {})", log_path.display());
    let start = Instant::now();

    cmd.stdout(Stdio::from(
        log.try_clone()
            .map_err(|e| anyhow::anyhow!("clone log handle: {e}"))?,
    ));
    cmd.stderr(Stdio::from(
        log.try_clone()
            .map_err(|e| anyhow::anyhow!("clone log handle: {e}"))?,
    ));

    let status = cmd.status()?;
    let elapsed = start.elapsed();

    if status.success() {
        println!("  ✓ {step} ({})", fmt_duration(elapsed));
        Ok(())
    } else {
        println!("  ✗ {step} ({})", fmt_duration(elapsed));
        bail!("{step} failed with {status}; see {}", log_path.display());
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
    // YYYYMMDD-HHMMSS using the system clock. No chrono dep; good enough
    // for log filenames (collision-free within a second).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let s = now.as_secs();
    let (y, mo, d, h, mi, se) = unix_to_ymdhms(s);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{se:02}")
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
