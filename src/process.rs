use std::env;
use std::fs::File;
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

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

pub(crate) fn run_capture_with_timeout(
    cmd: &mut Command,
    label: &str,
    timeout: Duration,
) -> DynResult<Captured> {
    if should_echo() {
        println!("$ {}", render_command(cmd));
    }
    prepare_process_group(cmd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let pid = child.id();
    let stdout_reader = read_pipe_in_thread(child.stdout.take());
    let stderr_reader = read_pipe_in_thread(child.stderr.take());
    let deadline = Instant::now() + timeout;

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_child_group(&mut child);
            let stdout = join_pipe(stdout_reader);
            let stderr = join_pipe(stderr_reader);
            bail!(
                "{label} timed out after {}s (pid={pid})\nstdout:\n{}\nstderr:\n{}",
                timeout.as_secs(),
                truncate_for_error(&stdout),
                truncate_for_error(&stderr)
            );
        }
        thread::sleep(Duration::from_millis(100));
    };

    let captured = Captured {
        status,
        stdout: join_pipe(stdout_reader),
        stderr: join_pipe(stderr_reader),
    };

    if !captured.status.success() {
        bail!("{label} failed: {}", captured.stderr);
    }

    Ok(captured)
}

pub(crate) fn run_forwarded_with_timeout(
    cmd: &mut Command,
    label: &str,
    timeout: Duration,
) -> DynResult<()> {
    if should_echo() {
        println!("$ {}", render_command(cmd));
    }
    prepare_process_group(cmd);
    let mut child = cmd.spawn()?;
    let pid = child.id();
    let deadline = Instant::now() + timeout;

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_child_group(&mut child);
            bail!("{label} timed out after {}s (pid={pid})", timeout.as_secs());
        }
        thread::sleep(Duration::from_millis(100));
    };

    if !status.success() {
        bail!("{label} failed with {status}");
    }
    Ok(())
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

fn read_pipe_in_thread<R>(pipe: Option<R>) -> thread::JoinHandle<String>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let Some(mut pipe) = pipe else {
            return String::new();
        };
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).to_string()
    })
}

fn join_pipe(handle: thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_else(|_| String::new())
}

fn truncate_for_error(text: &str) -> String {
    const MAX: usize = 8_000;
    if text.len() <= MAX {
        return text.to_string();
    }
    format!("{}\n<truncated>", &text[text.len().saturating_sub(MAX)..])
}

#[cfg(unix)]
fn prepare_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            if setpgid_for_child(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

#[cfg(not(unix))]
fn prepare_process_group(_cmd: &mut Command) {}

#[cfg(unix)]
fn terminate_child_group(child: &mut Child) {
    let pgid = format!("-{}", child.id());
    let _ = Command::new("kill").arg("-TERM").arg(&pgid).status();
    thread::sleep(Duration::from_millis(500));
    if child.try_wait().ok().flatten().is_none() {
        let _ = Command::new("kill").arg("-KILL").arg(&pgid).status();
    }
    let _ = child.wait();
}

#[cfg(not(unix))]
fn terminate_child_group(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn setpgid_for_child(pid: i32, pgid: i32) -> i32 {
    extern "C" {
        fn setpgid(pid: i32, pgid: i32) -> i32;
    }
    unsafe { setpgid(pid, pgid) }
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

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use super::run_capture_with_timeout;

    #[test]
    fn capture_with_timeout_returns_command_output() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("printf ok");
        let out =
            run_capture_with_timeout(&mut cmd, "quick command", Duration::from_secs(2)).unwrap();
        assert_eq!(out.stdout, "ok");
    }

    #[test]
    fn capture_with_timeout_fails_bounded_command() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 5");
        let err = match run_capture_with_timeout(&mut cmd, "slow command", Duration::from_secs(1)) {
            Ok(_) => panic!("slow command should time out"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("slow command timed out after 1s"),
            "unexpected error: {err}"
        );
    }
}
