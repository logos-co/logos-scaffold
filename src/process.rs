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

    // Daemonize: detach from parent process group so the sequencer
    // survives shell/tmux session closure (fixes logos-co/logos-scaffold#33)
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            // Create a new session — detaches from controlling terminal.
            // setsid() returns -1 on failure (e.g. process is already a group leader).
            let ret = libc::setsid();
            if ret == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

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
