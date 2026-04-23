use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};

use crate::constants::{
    BASECAMP_AUTODISCOVER_SKIP_SUBDIRS, BASECAMP_DEPENDENCIES, BASECAMP_PREINSTALLED_MODULES,
    BASECAMP_PROFILES_REL, BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB, BASECAMP_URL,
    BASECAMP_XDG_APP_SUBPATH, DEFAULT_BASECAMP_PIN, DEFAULT_LGPM_FLAKE,
};
use crate::model::{BasecampSource, BasecampState, Project, RepoRef};
use crate::process::{derive_log_path, rotate_logs, run_checked, run_logged, set_print_output};
use crate::project::{load_project, save_project_config};
use crate::repo::{sync_repo_to_pin, RepoSyncOptions};
use crate::state::{read_basecamp_state, write_basecamp_state};
use crate::DynResult;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields wired up in later phases
pub(crate) enum BasecampAction {
    Setup,
    Modules {
        paths: Vec<PathBuf>,
        flakes: Vec<String>,
        show: bool,
    },
    /// Pure replay: build and install everything captured in `state` (deps
    /// first). No source-set flags — use `basecamp modules` to change what's
    /// captured. If state is empty, transparently invokes `modules` in
    /// auto-discover mode and proceeds.
    Install {
        print_output: bool,
    },
    Launch {
        profile: String,
        no_clean: bool,
    },
    Reset {
        dry_run: bool,
    },
    /// Attr-swap replay on `state.project_sources` only (`#lgx` →
    /// `#lgx-portable`). `state.dependencies` is ignored — the target AppImage
    /// provides those. No CLI source flags.
    BuildPortable,
    /// Basecamp-specific doctor: captured modules summary, manifest variant
    /// check per seeded profile, and drift between auto-discovery and state.
    Doctor {
        json: bool,
    },
    ProfileList {
        json: bool,
    },
}

pub(crate) fn cmd_basecamp(action: BasecampAction) -> DynResult<()> {
    let project = load_project().context(
        "This command must be run inside a logos-scaffold project.\nNext step: cd into your scaffolded project directory and retry.",
    )?;

    match action {
        BasecampAction::Setup => cmd_basecamp_setup(project),
        BasecampAction::Modules {
            paths,
            flakes,
            show,
        } => cmd_basecamp_modules(project, paths, flakes, show, &NixLgxProbe),
        BasecampAction::Install { print_output } => {
            if print_output {
                set_print_output(true);
            }
            cmd_basecamp_install(project, &NixLgxProbe)
        }
        BasecampAction::Launch { profile, no_clean } => {
            cmd_basecamp_launch(project, profile, no_clean)
        }
        BasecampAction::Reset { dry_run } => cmd_basecamp_reset(project, dry_run),
        BasecampAction::BuildPortable => cmd_basecamp_build_portable(project),
        BasecampAction::Doctor { json } => cmd_basecamp_doctor(project, json),
        // Phase 5 stub: load_project() above is intentional so "outside project"
        // errors precede "not implemented" — future implementer must preserve that order.
        BasecampAction::ProfileList { .. } => bail!("basecamp profile list is not yet implemented"),
    }
}

fn cmd_basecamp_setup(mut project: Project) -> DynResult<()> {
    let mut bc = project.config.basecamp.clone().unwrap_or_default();
    if bc.source.is_empty() {
        bc.source = BASECAMP_URL.to_string();
    }
    if bc.pin.is_empty() {
        bc.pin = DEFAULT_BASECAMP_PIN.to_string();
    }

    let cache_root = project.root.join(&project.config.cache_root);
    let basecamp_repo_path = cache_root.join("repos/basecamp").join(&bc.pin);

    println!("cloning basecamp at {}", &bc.pin);
    let mut repo_ref = RepoRef {
        url: bc.source.clone(),
        source: bc.source.clone(),
        path: basecamp_repo_path.display().to_string(),
        pin: bc.pin.clone(),
    };
    sync_repo_to_pin(
        &mut repo_ref,
        "basecamp",
        RepoSyncOptions::auto_reclone_cache_repo(),
    )?;
    bc.pin = repo_ref.pin.clone();

    let pin_artifacts = cache_root.join("basecamp").join(&bc.pin);
    fs::create_dir_all(&pin_artifacts)
        .with_context(|| format!("create {}", pin_artifacts.display()))?;

    let basecamp_bin = build_basecamp_app(&project.root, &basecamp_repo_path, &pin_artifacts)?;
    let lgpm_bin = build_lgpm(&project.root, &pin_artifacts, bc.lgpm_flake.as_str())?;

    let profiles_root = project.root.join(BASECAMP_PROFILES_REL);
    let seeded = seed_profiles(
        &profiles_root,
        &[BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB],
    )?;
    println!("seeded profiles: {}", seeded.join(", "));

    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let existing = read_basecamp_state(&state_path).unwrap_or_default();
    let state = BasecampState {
        pin: bc.pin.clone(),
        basecamp_bin: basecamp_bin.display().to_string(),
        lgpm_bin: lgpm_bin.display().to_string(),
        project_sources: existing.project_sources,
        dependencies: existing.dependencies,
    };
    write_basecamp_state(&state_path, &state)?;

    project.config.basecamp = Some(bc);
    save_project_config(&project)?;

    println!("setup complete");
    Ok(())
}

fn build_basecamp_app(project_root: &Path, repo: &Path, out_dir: &Path) -> DynResult<PathBuf> {
    let link = out_dir.join("app-result");
    let log = derive_log_path(project_root, "setup-basecamp");
    let mut cmd = Command::new("nix");
    cmd.current_dir(repo)
        .arg("build")
        .arg(".#app")
        .arg("--out-link")
        .arg(&link);
    run_logged(&mut cmd, "building basecamp", &log)?;
    rotate_logs(project_root, "setup-basecamp", 10);
    resolve_basecamp_binary(&link)
}

fn build_lgpm(project_root: &Path, out_dir: &Path, override_flake: &str) -> DynResult<PathBuf> {
    let link = out_dir.join("lgpm-result");
    let flake_ref = if override_flake.is_empty() {
        DEFAULT_LGPM_FLAKE.to_string()
    } else {
        override_flake.to_string()
    };
    let log = derive_log_path(project_root, "setup-lgpm");
    let mut cmd = Command::new("nix");
    cmd.arg("build")
        .arg(&flake_ref)
        .arg("--out-link")
        .arg(&link);
    run_logged(&mut cmd, &format!("building lgpm ({flake_ref})"), &log)?;
    rotate_logs(project_root, "setup-lgpm", 10);
    Ok(link.join("bin/lgpm"))
}

fn resolve_basecamp_binary(app_link: &Path) -> DynResult<PathBuf> {
    // v0.1.1 layout: bin/logos-basecamp (Linux); macOS app bundle ships under Applications/.
    for rel in ["bin/logos-basecamp", "bin/LogosBasecamp", "bin/basecamp"] {
        let candidate = app_link.join(rel);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let platform_hint = if cfg!(target_os = "macos") {
        "\nNote: on macOS, `nix build .#app` produces an app bundle under Applications/. \
         v0.1.x does not yet expose a CLI-invocable binary on macOS; track basecamp-profiles spec §6."
    } else {
        ""
    };
    bail!(
        "could not locate basecamp binary inside nix build result {}{platform_hint}",
        app_link.display()
    )
}

// Concurrent `launch <same-profile>` is undefined per spec §2.3 ("v1 does not
// lock; document as 'don't do that'"). The code below assumes a single launcher
// per profile at a time — scrub, re-seed, replay install, and PID write are all
// non-atomic. If two invocations race, expect partial state.
fn cmd_basecamp_launch(project: Project, profile: String, no_clean: bool) -> DynResult<()> {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let state = match read_basecamp_state(&state_path).ok() {
        Some(s) if !s.basecamp_bin.is_empty() && !s.lgpm_bin.is_empty() => s,
        _ => bail!("basecamp not set up yet; run: logos-scaffold basecamp setup"),
    };
    if !Path::new(&state.basecamp_bin).exists() || !Path::new(&state.lgpm_bin).exists() {
        bail!("basecamp not set up yet; run: logos-scaffold basecamp setup");
    }

    if profile != BASECAMP_PROFILE_ALICE && profile != BASECAMP_PROFILE_BOB {
        bail!(
            "unknown profile `{profile}`; v1 only supports `{}` and `{}`",
            BASECAMP_PROFILE_ALICE,
            BASECAMP_PROFILE_BOB
        );
    }
    // Defense-in-depth against a future caller bypassing the allowlist: refuse
    // any profile name that could escape the profiles root via path separators.
    if profile.contains('/') || profile.contains(MAIN_SEPARATOR) {
        bail!("profile name `{profile}` must not contain path separators");
    }

    let profiles_root = project.root.join(BASECAMP_PROFILES_REL);
    let profile_dir = profiles_root.join(&profile);
    if !profile_dir.is_dir() {
        bail!(
            "profile `{profile}` missing under {}; re-run `logos-scaffold basecamp setup`",
            profiles_root.display()
        );
    }

    let launch_state_path = profile_dir.join("launch.state");
    let expected_comm = basecamp_comm_name(&state.basecamp_bin);
    if let Some(pid) = read_launch_pid(&launch_state_path) {
        // PID reuse means we can't be sure the recorded PID still maps to basecamp.
        // Only issue signals if the PID's current comm matches; `kill_process_tree`
        // re-verifies the comm before each signal to reduce the TOCTOU window.
        if pid_comm_matches(pid, &expected_comm) {
            kill_process_tree(pid, &expected_comm);
        }
        let _ = fs::remove_file(&launch_state_path);
    }

    // seed_profiles is idempotent (tested) and cheap — always run it so a prior
    // crash mid-scrub doesn't leave the profile without its xdg subdirs.
    seed_profiles(&profiles_root, &[profile.as_str()])?;
    if !no_clean {
        scrub_profile_data_and_cache(&project.root, &profile_dir)?;
        // Re-seed after scrub: scrub removed xdg-data + xdg-cache; put their
        // module/plugin subtrees back before lgpm writes into them.
        seed_profiles(&profiles_root, &[profile.as_str()])?;
        install_sources_into_profiles(
            &state,
            &project.root,
            &project.config.cache_root,
            &profiles_root,
            &[profile.clone()],
        )?;
    }

    // Variant pre-flight: warn if any installed module is missing the current
    // platform's `<plat>-dev` manifest.json `main` key. Basecamp v0.1.1's
    // variant resolver silently hangs on first click when loading such a
    // plugin, so catch it before the user ever clicks. Non-blocking: we
    // still exec basecamp regardless — the dev may want to click around
    // anyway.
    if let Some(expected_dev) = platform_dev_variant_key() {
        let issues = check_manifest_variants(&profile_dir, &profile, expected_dev);
        let module_count = count_installed_modules(&profile_dir);
        if issues.is_empty() {
            println!(
                "launch: profile {profile} has {module_count} module(s); all {expected_dev} variants present ✓"
            );
        } else {
            let names: Vec<String> = issues.iter().map(|i| i.module_name.clone()).collect();
            println!(
                "launch: profile {profile} has {module_count} module(s); \
                 {} missing {expected_dev} variant ({}); \
                 run `logos-scaffold doctor` for details — plugins will hang on click.",
                issues.len(),
                names.join(", ")
            );
        }
    }

    let env = launch_env(&profile_dir, &profile);
    println!("launching basecamp for profile {profile}");
    let mut cmd = Command::new(&state.basecamp_bin);
    for (k, v) in &env {
        cmd.env(k, v);
    }
    // Per-module port-override env vars (spec §3.4) are owned by each module and
    // flow in via a registry — empty in v1 since no modules have published names.
    // Concurrent alice/bob on the same host may collide on module-level ports
    // until upstreams adopt overrides; see basecamp-profiles §3.4.
    write_launch_pid(&launch_state_path, std::process::id())?;
    let err = cmd.exec();
    // exec() only returns on failure. On Linux/Unix exec preserves the PID, so
    // launch.state is valid once exec succeeds — but on failure the PID we wrote
    // belongs to the scaffold process that's about to exit. Remove the file so a
    // later launch doesn't kill whatever reuses the PID.
    let _ = fs::remove_file(&launch_state_path);
    bail!("failed to exec basecamp at {}: {err}", state.basecamp_bin);
}

/// Env map exported to the basecamp child on launch. Scaffold-owned names only
/// (spec §3.4); module port-override vars are not yet registered.
fn launch_env(profile_dir: &Path, profile_name: &str) -> BTreeMap<String, OsString> {
    let mut env = BTreeMap::new();
    env.insert(
        "XDG_CONFIG_HOME".into(),
        profile_dir.join("xdg-config").into_os_string(),
    );
    env.insert(
        "XDG_DATA_HOME".into(),
        profile_dir.join("xdg-data").into_os_string(),
    );
    env.insert(
        "XDG_CACHE_HOME".into(),
        profile_dir.join("xdg-cache").into_os_string(),
    );
    env.insert("LOGOS_PROFILE".into(), profile_name.into());
    env
}

/// Remove a profile's `xdg-data` and `xdg-cache` trees. Refuses to operate on any
/// path outside `<project>/.scaffold/basecamp/profiles/` — guards against a
/// caller that constructs an absolute profile_dir pointing elsewhere.
fn scrub_profile_data_and_cache(project_root: &Path, profile_dir: &Path) -> DynResult<()> {
    let safe_root = project_root.join(BASECAMP_PROFILES_REL);
    // canonicalize() requires every path component to exist. Create safe_root up
    // front so the canonical form is well-defined before the prefix check. For
    // profile_dir we canonicalize the parent (which must exist — we just made it)
    // and append the final component, matching how the path would resolve.
    fs::create_dir_all(&safe_root).with_context(|| format!("create {}", safe_root.display()))?;
    let canon_safe = safe_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", safe_root.display()))?;
    let canon_profile = canonicalize_under(profile_dir)?;
    if !canon_profile.starts_with(&canon_safe) {
        bail!(
            "refusing to scrub {} — outside the profiles root {}",
            canon_profile.display(),
            canon_safe.display()
        );
    }
    for xdg in ["xdg-data", "xdg-cache"] {
        let dir = profile_dir.join(xdg);
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("scrub {}", dir.display()))?;
        }
    }
    Ok(())
}

/// Canonicalize `p`, allowing the final component not to exist yet (canonicalize
/// the parent and re-append). Returns an error if the parent is missing or a
/// symlink to somewhere that doesn't resolve — never silently falls back.
fn canonicalize_under(p: &Path) -> DynResult<PathBuf> {
    if let Ok(c) = p.canonicalize() {
        return Ok(c);
    }
    let parent = p
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", p.display()))?;
    let file = p
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("path has no final component: {}", p.display()))?;
    let canon_parent = parent
        .canonicalize()
        .with_context(|| format!("canonicalize {}", parent.display()))?;
    Ok(canon_parent.join(file))
}

fn read_launch_pid(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("pid=") {
            if let Ok(pid) = rest.parse::<u32>() {
                return Some(pid);
            }
        }
    }
    None
}

/// Atomic PID write via tmp-file + rename. A crash mid-write would otherwise
/// leave a truncated `launch.state` that reads back as malformed garbage.
fn write_launch_pid(path: &Path, pid: u32) -> DynResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let tmp = path.with_extension("state.tmp");
    fs::write(&tmp, format!("pid={pid}\n")).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))
}

/// `basename(basecamp_bin)` truncated to 15 bytes — matches `/proc/<pid>/comm`
/// semantics on Linux so [`pid_comm_matches`] can compare directly.
fn basecamp_comm_name(basecamp_bin: &str) -> String {
    let base = Path::new(basecamp_bin)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("basecamp");
    base.chars().take(15).collect()
}

/// True if the running process at `pid` has a comm matching `expected`. Uses
/// portable `ps -p` so this works on Linux and macOS. Returns false if `ps`
/// fails (process gone, PID reused by a process we can't inspect, etc.) — fail
/// closed: the caller skips the kill on a mismatch.
///
/// Both sides are truncated to 15 bytes (the kernel `/proc/<pid>/comm` limit)
/// before comparison, so a 20-byte binary name like `logos-basecamp-dev` still
/// matches a `comm` of `logos-basecamp-`. Equality is strict after truncation —
/// a prefix like `logos-bas` does not match `logos-basecamp-`.
fn pid_comm_matches(pid: u32, expected: &str) -> bool {
    let out = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("comm=")
        .output();
    let Ok(out) = out else { return false };
    if !out.status.success() {
        return false;
    }
    let comm = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if comm.is_empty() {
        return false;
    }
    // ps may print a leading path (BSD) or just the comm (Linux). Compare
    // basenames and tolerate the 15-byte comm truncation either side.
    let comm_base = Path::new(&comm)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&comm);
    let comm_trunc: String = comm_base.chars().take(15).collect();
    let expected_trunc: String = expected.chars().take(15).collect();
    !comm_trunc.is_empty() && comm_trunc == expected_trunc
}

/// Kill the process tree rooted at `pid`. Enumerates descendants via `/proc`
/// (Linux) and falls back to `pkill -P` on other Unix hosts where `/proc` isn't
/// available. Every signal is gated on a fresh `pid_comm_matches(pid, expected)`
/// check so a PID that was recycled between our entry check and the actual kill
/// doesn't get signalled at all. Descendants are always KILLed after the grace
/// period — the parent may exit before its children, leaving them orphaned to
/// init while still bound to profile ports.
fn kill_process_tree(pid: u32, expected_comm: &str) {
    // Snapshot descendants *before* TERM — once the parent starts exiting, its
    // children get reparented to init and we lose the ppid linkage.
    let descendants = collect_descendant_pids(pid);
    for child in &descendants {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(child.to_string())
            .status();
    }
    if pid_comm_matches(pid, expected_comm) {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status();
    }
    let parent_exited = wait_for_exit(pid, Duration::from_millis(1500));
    for child in &descendants {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(child.to_string())
            .status();
    }
    if !parent_exited && pid_comm_matches(pid, expected_comm) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
        let _ = wait_for_exit(pid, Duration::from_millis(500));
    }
}

/// BFS over `/proc/<pid>/stat` to collect every descendant PID rooted at
/// `root`. On non-Linux hosts (no `/proc`), falls back to a one-level `pgrep -P`
/// — best-effort; grandchildren may be missed on macOS until v2 tracks them via
/// process groups.
fn collect_descendant_pids(root: u32) -> Vec<u32> {
    if Path::new("/proc").is_dir() {
        return linux_descendant_pids(root);
    }
    // Non-Linux fallback: direct children only.
    let out = Command::new("pgrep")
        .arg("-P")
        .arg(root.to_string())
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .collect()
}

fn linux_descendant_pids(root: u32) -> Vec<u32> {
    // Build child-map: ppid -> [pid...].
    let mut children: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // /proc/<pid>/stat layout: `<pid> (<comm>) <state> <ppid> ...`
        // `comm` can contain spaces/parens; split on the *last* ')'.
        let Some(rparen) = stat.rfind(')') else {
            continue;
        };
        let rest = stat[rparen + 1..].trim_start();
        let mut fields = rest.split_ascii_whitespace();
        let _state = fields.next();
        let Some(ppid_str) = fields.next() else {
            continue;
        };
        let Ok(ppid) = ppid_str.parse::<u32>() else {
            continue;
        };
        children.entry(ppid).or_default().push(pid);
    }

    let mut out = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    queue.push_back(root);
    while let Some(p) = queue.pop_front() {
        if let Some(kids) = children.get(&p) {
            for &k in kids {
                if seen.insert(k) {
                    out.push(k);
                    queue.push_back(k);
                }
            }
        }
    }
    out
}

fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        // `kill -0` returns non-zero when the PID no longer exists (or we lack perms).
        let status = Command::new("kill").arg("-0").arg(pid.to_string()).status();
        if matches!(status, Ok(s) if !s.success()) || status.is_err() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Build portable `.lgx` artefacts for hand-loading into a basecamp AppImage.
///
/// Operates on `state.project_sources` only, with each flake entry's `#lgx`
/// attribute swapped to `#lgx-portable`. `state.dependencies` is intentionally
/// ignored — the target AppImage provides its own (release/portable) copies
/// of companion modules via its in-app Package Manager catalog.
///
/// No CLI source flags: the source set lives in `state`, managed by
/// `basecamp modules`. If you want to produce a portable variant of something
/// that isn't a project source, `basecamp modules --flake <ref>#lgx` it first,
/// run `build-portable`, then revert with another `modules` call.
fn cmd_basecamp_build_portable(project: Project) -> DynResult<()> {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let state = read_basecamp_state(&state_path).unwrap_or_default();

    if state.project_sources.is_empty() {
        bail!(
            "no project modules captured in state; run `basecamp modules` \
             first (auto-discover) or `basecamp modules --flake <ref>#lgx \
             --path <file.lgx>` to capture explicitly. `build-portable` \
             operates on captured project sources only — it never discovers."
        );
    }

    // Rewrite each project source: attr-swap `#lgx` → `#lgx-portable` on
    // flake refs; pass Path sources through unchanged (they're pre-built).
    let portable_sources: Vec<BasecampSource> = state
        .project_sources
        .iter()
        .map(|src| match src {
            BasecampSource::Path(p) => BasecampSource::Path(p.clone()),
            BasecampSource::Flake(f) => {
                BasecampSource::Flake(swap_flake_attr(f, "lgx", "lgx-portable"))
            }
        })
        .collect();

    let mut outputs: Vec<PathBuf> = Vec::new();
    for src in &portable_sources {
        match src {
            BasecampSource::Path(p) => {
                outputs.push(build_portable_resolve_path(Path::new(p))?);
            }
            BasecampSource::Flake(flake_ref) => {
                // Sibling overrides still computed against the post-swap set
                // so path-sibling inputs resolve locally like they do at install.
                let overrides = resolve_sibling_overrides(src, &portable_sources, flake_ref);
                let inv = build_portable_nix_invocation(flake_ref, &overrides);
                outputs.extend(run_build_portable_nix(&project.root, flake_ref, &inv)?);
            }
        }
    }

    println!("Built portable .lgx artefacts (copy these into your basecamp AppImage manually):");
    for out in &outputs {
        println!("  {}", out.display());
    }
    Ok(())
}

/// Swap the attribute suffix on a flake ref: `path:/abs#lgx` → `path:/abs#lgx-portable`.
/// If the ref's attr differs from `expected`, the ref is returned unchanged
/// (trust the user; they explicitly pinned a non-default attr via
/// `basecamp modules --flake <ref>#<other>`).
fn swap_flake_attr(flake_ref: &str, expected: &str, replacement: &str) -> String {
    match flake_ref.rsplit_once('#') {
        Some((base, attr)) if attr == expected => format!("{base}#{replacement}"),
        Some((_, _)) => flake_ref.to_string(),
        None => flake_ref.to_string(),
    }
}

/// Command-line arguments (after `nix`) plus an optional working-directory
/// override. For `path:<abs>#<attr>` refs we cd into `<abs>` and invoke
/// `nix build .#<attr>` so the default `./result-<attr>` symlink lands next
/// to the flake. For remote refs we stay in the caller's cwd.
#[derive(Debug, PartialEq, Eq)]
struct NixBuildInvocation {
    cwd_override: Option<PathBuf>,
    args: Vec<String>,
}

fn build_portable_nix_invocation(
    flake_ref: &str,
    overrides: &[(String, String)],
) -> NixBuildInvocation {
    let (cwd_override, ref_arg) = match flake_path_prefix(flake_ref) {
        Some(abs) => {
            let attr = flake_ref
                .split_once('#')
                .map(|(_, a)| a)
                .unwrap_or("lgx-portable");
            (Some(PathBuf::from(abs)), format!(".#{attr}"))
        }
        None => (None, flake_ref.to_string()),
    };

    let mut args = vec!["build".to_string(), ref_arg];
    for (name, value) in overrides {
        args.push("--override-input".to_string());
        args.push(name.clone());
        args.push(value.clone());
    }
    args.push("--print-out-paths".to_string());

    NixBuildInvocation { cwd_override, args }
}

/// Validate a `--path` source and return its canonical absolute path.
fn build_portable_resolve_path(src: &Path) -> DynResult<PathBuf> {
    if !src.is_file() || !src.extension().is_some_and(|e| e == "lgx") {
        bail!("path `{}` is not a .lgx file", src.display());
    }
    Ok(src.canonicalize().unwrap_or_else(|_| src.to_path_buf()))
}

fn run_build_portable_nix(
    project_root: &Path,
    flake_ref: &str,
    inv: &NixBuildInvocation,
) -> DynResult<Vec<PathBuf>> {
    println!("building {flake_ref}");
    let mut cmd = Command::new("nix");
    match &inv.cwd_override {
        Some(cwd) => {
            cmd.current_dir(cwd);
        }
        None => {
            cmd.current_dir(project_root);
        }
    }
    for a in &inv.args {
        cmd.arg(a);
    }
    let output = cmd
        .output()
        .with_context(|| format!("spawn nix build {flake_ref}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "nix build {flake_ref} failed ({}): {}",
            output.status,
            stderr.trim()
        );
    }
    // `nix build --print-out-paths` today emits only store paths on stdout;
    // diagnostics go to stderr. Accept only lines that look like absolute
    // filesystem paths so a future nix version that adds trailing summary
    // text to stdout doesn't pollute our output list. Users with non-standard
    // store prefixes (rare) still work — we don't hard-require `/nix/store/`.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let store_paths: Vec<PathBuf> = stdout
        .lines()
        .map(|s| s.trim())
        .filter(|s| s.starts_with('/'))
        .map(PathBuf::from)
        .collect();
    if store_paths.is_empty() {
        bail!("nix build {flake_ref} returned no output paths");
    }
    let mut results = Vec::new();
    for sp in store_paths {
        if sp.is_dir() {
            results.extend(list_lgx(&sp)?);
        } else {
            results.push(sp);
        }
    }
    Ok(results)
}

fn cmd_basecamp_reset(project: Project, dry_run: bool) -> DynResult<()> {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let state = match read_basecamp_state(&state_path).ok() {
        Some(s) if !s.basecamp_bin.is_empty() => s,
        _ => bail!("basecamp not set up yet; run: logos-scaffold basecamp setup"),
    };

    let expected_comm = basecamp_comm_name(&state.basecamp_bin);
    let profiles_root = project.root.join(BASECAMP_PROFILES_REL);
    let seed_names = [BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB];

    let mut live: Vec<(String, u32, String)> = Vec::new();
    for name in &seed_names {
        let launch_state = profiles_root.join(name).join("launch.state");
        if let Some(pid) = read_launch_pid(&launch_state) {
            if pid_comm_matches(pid, &expected_comm) {
                live.push(((*name).to_string(), pid, expected_comm.clone()));
            }
        }
    }

    for line in plan_reset_lines(&live, &profiles_root, state.total_sources(), &seed_names) {
        println!("{line}");
    }

    if dry_run {
        return Ok(());
    }

    // Log each step so a mid-flight failure is legible: the user can tell
    // from stdout exactly which stage completed and which errored.
    if !live.is_empty() {
        println!("killing {} live basecamp process(es)", live.len());
        for (_profile, pid, comm) in &live {
            kill_process_tree(*pid, comm);
        }
    }
    println!("wiping {}", profiles_root.display());
    remove_profiles_root(&project.root)?;
    println!("clearing recorded sources");
    clear_basecamp_sources(&state_path)?;
    println!("re-seeding profiles: {}", seed_names.join(", "));
    seed_profiles(&profiles_root, &seed_names)?;

    println!("reset complete; run `basecamp install` to install modules");
    Ok(())
}

fn plan_reset_lines(
    live: &[(String, u32, String)],
    profiles_root: &Path,
    source_count: usize,
    seed_profile_names: &[&str],
) -> Vec<String> {
    let mut out = vec!["reset plan:".to_string()];
    if live.is_empty() {
        out.push("  kill: none (no live basecamp tracked)".to_string());
    } else {
        for (profile, pid, comm) in live {
            out.push(format!(
                "  kill pid {pid} (profile: {profile}, comm: {comm})"
            ));
        }
    }
    out.push(format!("  rm -rf {}", profiles_root.display()));
    out.push(format!(
        "  clear basecamp.state sources ({source_count} recorded)"
    ));
    out.push(format!(
        "  re-seed profiles: {}",
        seed_profile_names.join(", ")
    ));
    out
}

/// Remove `<project_root>/.scaffold/basecamp/profiles/` idempotently, refusing
/// to follow any symlink that would escape `.scaffold/basecamp/`. The profiles
/// path is a constant, so the safety check is defense-in-depth against a hostile
/// or mistaken symlink.
fn remove_profiles_root(project_root: &Path) -> DynResult<()> {
    let profiles_root = project_root.join(BASECAMP_PROFILES_REL);
    if !profiles_root.exists() && !profiles_root.is_symlink() {
        return Ok(());
    }
    let scaffold_basecamp = project_root.join(".scaffold/basecamp");
    fs::create_dir_all(&scaffold_basecamp)
        .with_context(|| format!("create {}", scaffold_basecamp.display()))?;
    let canon_base = scaffold_basecamp
        .canonicalize()
        .with_context(|| format!("canonicalize {}", scaffold_basecamp.display()))?;
    let canon_profiles = canonicalize_under(&profiles_root)?;
    if !canon_profiles.starts_with(&canon_base) {
        bail!(
            "refusing to remove {} — resolves outside {}",
            canon_profiles.display(),
            canon_base.display()
        );
    }
    // Pass the canonical path to `remove_dir_all` so a TOCTOU symlink swap
    // between the check above and the removal can't redirect us to a path
    // outside the safe root. If `profiles_root` itself was a symlink (its
    // target was verified above to be under `canon_base`), the target is
    // now gone — clean up the dangling link afterwards.
    fs::remove_dir_all(&canon_profiles)
        .with_context(|| format!("remove {}", canon_profiles.display()))?;
    if profiles_root.is_symlink() {
        fs::remove_file(&profiles_root)
            .with_context(|| format!("remove symlink {}", profiles_root.display()))?;
    }
    Ok(())
}

/// Rewrite `basecamp.state` with `sources = []`, preserving `pin`,
/// `basecamp_bin`, and `lgpm_bin` so the expensive setup artefacts stay
/// usable. Atomic via `write_basecamp_state`'s underlying tmp+rename.
fn clear_basecamp_sources(state_path: &Path) -> DynResult<()> {
    let mut state = read_basecamp_state(state_path)
        .with_context(|| format!("read {}", state_path.display()))?;
    state.project_sources.clear();
    state.dependencies.clear();
    write_basecamp_state(state_path, &state)
}

/// Annotation attached to a captured source for the "captured modules:" printout.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceOrigin {
    /// Discovered by walking the project's root or immediate sub-flake(s).
    AutoDiscovered,
    /// Supplied by the user via `--flake` or `--path`.
    Explicit,
    /// Resolved from a `metadata.json` `dependencies` entry against the
    /// scaffold-level default pin table.
    DepDefault { name: String, via: String },
    /// Resolved from a `metadata.json` `dependencies` entry against a
    /// `[basecamp.dependencies]` override in `scaffold.toml`.
    DepOverride { name: String, via: String },
    /// Resolved from a matching input in the project's own `flake.lock`,
    /// next to the source that declared the dep in its `metadata.json`.
    /// The project's flake.lock is the most authoritative pin because
    /// that's what the dev is actually building against.
    DepFromProjectLock {
        name: String,
        via: String,
        source_flake: String,
    },
}

impl SourceOrigin {
    fn annotation(&self) -> String {
        match self {
            SourceOrigin::AutoDiscovered => "[auto-discovered]".to_string(),
            SourceOrigin::Explicit => "[explicit]".to_string(),
            SourceOrigin::DepDefault { name, via } => {
                format!("[dep `{name}` via {via}, scaffold default]")
            }
            SourceOrigin::DepOverride { name, via } => {
                format!("[dep `{name}` via {via}, scaffold.toml override]")
            }
            SourceOrigin::DepFromProjectLock {
                name,
                via,
                source_flake,
            } => {
                format!("[dep `{name}` via {via}, pinned by {source_flake}/flake.lock]")
            }
        }
    }
}

/// Extract the rev / tag segment from a `github:owner/repo/<ref>#…` flake
/// ref. Returns `None` for non-github refs or refs without a ref segment.
/// Same helper the doctor uses — kept local to `basecamp.rs` so the drift
/// warning is self-contained.
fn github_flake_ref_rev(flake_ref: &str) -> Option<&str> {
    let rest = flake_ref.strip_prefix("github:")?;
    let before_frag = rest.split_once('#').map_or(rest, |(b, _)| b);
    let parts: Vec<&str> = before_frag.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    Some(parts[2])
}

/// Emit a drift warning when a project's `flake.lock` pins a runtime
/// companion dep to a rev different from scaffold's default. The project
/// pin wins (they're building against it), but the warning surfaces the
/// risk of incompatibility with a basecamp shipped against the scaffold
/// default — matches the `lez standalone pin` drift warning in `doctor`.
fn warn_on_dep_pin_drift(
    name: &str,
    scaffold_default_ref: &str,
    project_resolved_ref: &str,
    source_flake_path: &Path,
) {
    let default_rev = github_flake_ref_rev(scaffold_default_ref);
    let project_rev = github_flake_ref_rev(project_resolved_ref);
    if let (Some(d), Some(p)) = (default_rev, project_rev) {
        if d == p {
            return; // revs match — no drift
        }
    } else if scaffold_default_ref == project_resolved_ref {
        return;
    }
    eprintln!(
        "warning: dep `{name}` pinned by `{}/flake.lock` at `{}` \
         differs from scaffold default `{}`. Your module will build and \
         run locally against the project-lock rev, but may not work \
         against a basecamp release that expects the scaffold default. \
         If you're building for release, either update the project lock \
         to match the scaffold default or override via \
         `[basecamp.dependencies]` in scaffold.toml with an explicit \
         compatible pin.",
        source_flake_path.display(),
        project_rev.unwrap_or(project_resolved_ref),
        default_rev.unwrap_or(scaffold_default_ref),
    );
}

/// Given a source flake's local path and a dep name declared in its
/// `metadata.json`, try to find that name as an input in the sibling
/// `flake.lock` and return a `github:<owner>/<repo>/<rev>#lgx` ref.
///
/// Returns `None` if no local `flake.lock`, no matching input, or the
/// locked input isn't a github ref we can reconstruct. Non-fatal: callers
/// fall back to the next precedence layer.
fn resolve_dep_from_project_flake_lock(source_flake_path: &Path, dep_name: &str) -> Option<String> {
    let lock_path = source_flake_path.join("flake.lock");
    let text = fs::read_to_string(&lock_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let root = json.pointer("/nodes/root/inputs")?.as_object()?;
    // The root `inputs` object is a mapping of declared-input-name → node-id.
    // The node-id is typically the same as the input name but may be
    // suffixed with `_N` when multiple inputs share a name. Match on the
    // declared input name, not the node key.
    let node_id = root.get(dep_name)?.as_str()?;
    let locked = json.pointer(&format!("/nodes/{node_id}/locked"))?;
    let ty = locked.get("type")?.as_str()?;
    if ty != "github" {
        return None;
    }
    let owner = locked.get("owner")?.as_str()?;
    let repo = locked.get("repo")?.as_str()?;
    let rev = locked.get("rev")?.as_str()?;
    Some(format!("github:{owner}/{repo}/{rev}#lgx"))
}

/// Capture the modules + dependencies set into `basecamp.state`. Sole writer of
/// `state.project_sources` and `state.dependencies`. `install` / `build-portable`
/// / `launch` only read state — they never discover on their own.
///
/// Three modes:
/// - `--show`: read-only; print both lists labeled.
/// - Explicit (non-empty `paths` or `flakes`): `project_sources` = exactly
///   those args; `dependencies` re-resolved from the explicit sources' manifests.
/// - Auto (default, no args): walk project flakes; resolve manifest deps.
fn cmd_basecamp_modules(
    project: Project,
    paths: Vec<PathBuf>,
    flakes: Vec<String>,
    show: bool,
    probe: &dyn LgxFlakeProbe,
) -> DynResult<()> {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let existing = read_basecamp_state(&state_path).unwrap_or_default();

    if show {
        print_modules_list(
            "captured modules",
            &existing.project_sources,
            &existing.dependencies,
            &[],
        );
        return Ok(());
    }

    let explicit = !paths.is_empty() || !flakes.is_empty();

    let flakes: Vec<String> = flakes
        .into_iter()
        .map(|f| normalize_flake_ref(&project.root, &f))
        .collect();

    // Resolve project sources: explicit args bypass discovery; otherwise walk
    // project flakes per existing rules.
    let (project_sources, project_origins): (Vec<BasecampSource>, Vec<SourceOrigin>) = if explicit {
        let mut out = Vec::new();
        let mut origins = Vec::new();
        for p in &paths {
            out.push(BasecampSource::Path(p.display().to_string()));
            origins.push(SourceOrigin::Explicit);
        }
        for f in &flakes {
            out.push(BasecampSource::Flake(f.clone()));
            origins.push(SourceOrigin::Explicit);
        }
        (out, origins)
    } else {
        let cache_root_first = first_path_component(&project.config.cache_root);
        let mut skip_subdirs: Vec<&str> =
            BASECAMP_AUTODISCOVER_SKIP_SUBDIRS.iter().copied().collect();
        if let Some(c) = cache_root_first.as_deref() {
            if !skip_subdirs.contains(&c) {
                skip_subdirs.push(c);
            }
        }
        let discovered = resolve_install_sources(
            &project.root,
            &[],
            &[],
            probe,
            &skip_subdirs,
            "lgx",
            "lgx-portable",
        )?;
        let origins = vec![SourceOrigin::AutoDiscovered; discovered.len()];
        (discovered, origins)
    };

    // Resolve manifest-declared runtime dependencies. Walks each project source's
    // local metadata.json (no nix build) and resolves declared dep names against:
    //   1. `[basecamp.dependencies]` per-project override (wins)
    //   2. `BASECAMP_DEPENDENCIES` scaffold default
    //   3. `BASECAMP_PREINSTALLED_MODULES` — skip silently (basecamp provides)
    //   4. unknown — warn, skip.
    let dep_overrides = project
        .config
        .basecamp
        .as_ref()
        .map(|bc| bc.dependencies.clone())
        .unwrap_or_default();
    let (dependencies, dep_origins) =
        resolve_manifest_dependencies(&project_sources, &dep_overrides);

    // Print the "previous modules (for reference)" block so reverting is copy-paste.
    print_modules_list(
        "previous modules (for reference)",
        &existing.project_sources,
        &existing.dependencies,
        &[],
    );

    let new_state = BasecampState {
        project_sources: project_sources.clone(),
        dependencies: dependencies.clone(),
        ..existing
    };
    write_basecamp_state(&state_path, &new_state)?;

    // Print the new set with annotations.
    let mut annotations: Vec<SourceOrigin> = project_origins.clone();
    annotations.extend(dep_origins.iter().cloned());
    print_modules_list(
        "captured modules",
        &project_sources,
        &dependencies,
        &annotations,
    );

    Ok(())
}

fn print_modules_list(
    header: &str,
    project_sources: &[BasecampSource],
    dependencies: &[BasecampSource],
    annotations: &[SourceOrigin],
) {
    println!("{header}:");
    if project_sources.is_empty() && dependencies.is_empty() {
        println!("  (none)");
        return;
    }
    println!("  project_sources:");
    if project_sources.is_empty() {
        println!("    (none)");
    } else {
        for (i, src) in project_sources.iter().enumerate() {
            let note = annotations
                .get(i)
                .map(|o| format!(" {}", o.annotation()))
                .unwrap_or_default();
            println!("    {}{}", flake_ref(src), note);
        }
    }
    println!("  dependencies:");
    if dependencies.is_empty() {
        println!("    (none)");
    } else {
        let offset = project_sources.len();
        for (i, src) in dependencies.iter().enumerate() {
            let note = annotations
                .get(offset + i)
                .map(|o| format!(" {}", o.annotation()))
                .unwrap_or_default();
            println!("    {}{}", flake_ref(src), note);
        }
    }
}

/// Read `metadata.json` from a flake source's local filesystem path and
/// collect its `dependencies: [...]` array. Returns empty for remote flakes
/// (no local path to read) or path-sources (`.lgx` files are build artefacts,
/// not source directories).
fn read_source_metadata_dependencies(src: &BasecampSource) -> Vec<String> {
    let BasecampSource::Flake(flake_ref) = src else {
        return Vec::new();
    };
    let Some(local_path) = flake_path_prefix(flake_ref) else {
        return Vec::new(); // github:, git+, http(s):, etc. — no local walk
    };
    let metadata_path = Path::new(local_path).join("metadata.json");
    let Ok(text) = fs::read_to_string(&metadata_path) else {
        return Vec::new();
    };
    let Ok(json): Result<serde_json::Value, _> = serde_json::from_str(&text) else {
        eprintln!(
            "warning: could not parse {} as JSON; skipping manifest deps",
            metadata_path.display()
        );
        return Vec::new();
    };
    json.get("dependencies")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Walk each project source's `metadata.json`, union the declared dep names,
/// resolve each against overrides → defaults → preinstalled → unknown.
/// Returns `(resolved_sources, per-source origins)` in a stable (sorted) order.
fn resolve_manifest_dependencies(
    project_sources: &[BasecampSource],
    overrides: &std::collections::BTreeMap<String, String>,
) -> (Vec<BasecampSource>, Vec<SourceOrigin>) {
    // Union of all dep names across project sources, with which source flake
    // ref they came from (for the "via" annotation) AND the source's local
    // path (used to consult that source's own flake.lock for a pin).
    let mut declared: std::collections::BTreeMap<String, (String, Option<PathBuf>)> =
        std::collections::BTreeMap::new();
    for src in project_sources {
        let via = flake_ref(src);
        let local_path = match src {
            BasecampSource::Flake(f) => flake_path_prefix(f).map(PathBuf::from),
            BasecampSource::Path(_) => None,
        };
        for dep_name in read_source_metadata_dependencies(src) {
            declared
                .entry(dep_name)
                .or_insert_with(|| (via.clone(), local_path.clone()));
        }
    }

    let mut out_sources: Vec<BasecampSource> = Vec::new();
    let mut out_origins: Vec<SourceOrigin> = Vec::new();

    for (name, (via, local_path)) in declared {
        // Skip project-internal deps: if another project source's metadata
        // `name` matches this dep, it's already captured as a project source.
        if project_sources.iter().any(|src| {
            let flake_ref = match src {
                BasecampSource::Flake(f) => f,
                BasecampSource::Path(_) => return false,
            };
            if let Some(local) = flake_path_prefix(flake_ref) {
                if let Ok(text) = fs::read_to_string(Path::new(local).join("metadata.json")) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        return json.get("name").and_then(|v| v.as_str()) == Some(&name);
                    }
                }
            }
            false
        }) {
            continue;
        }

        // 1. Project override in scaffold.toml wins (explicit user intent).
        if let Some(flake_ref) = overrides.get(&name) {
            out_sources.push(BasecampSource::Flake(flake_ref.clone()));
            out_origins.push(SourceOrigin::DepOverride {
                name: name.clone(),
                via: via.clone(),
            });
            continue;
        }
        // 2. Project's own flake.lock input with the same name — prefer the
        //    pin the project is already building against. This is the
        //    authoritative source and sidesteps scaffold-default staleness
        //    (e.g. when an upstream tag predates the `#lgx` convention).
        if let Some(path) = &local_path {
            if let Some(resolved) = resolve_dep_from_project_flake_lock(path, &name) {
                // Drift warning: if scaffold also has a default pin for this
                // dep AND the revs differ, surface it loudly. The project's
                // pin wins (they're building against it) but the dev should
                // know they're off-scaffold — their module may not work
                // against a basecamp that shipped with the scaffold default.
                if let Some((_, default_ref)) =
                    BASECAMP_DEPENDENCIES.iter().find(|(n, _)| *n == name)
                {
                    warn_on_dep_pin_drift(&name, default_ref, &resolved, path);
                }
                out_sources.push(BasecampSource::Flake(resolved));
                out_origins.push(SourceOrigin::DepFromProjectLock {
                    name: name.clone(),
                    via: via.clone(),
                    source_flake: path.display().to_string(),
                });
                continue;
            }
        }
        // 3. Scaffold default pin — last-resort, may be stale for projects
        //    that don't declare the dep in their flake.
        if let Some((_, flake_ref)) = BASECAMP_DEPENDENCIES.iter().find(|(n, _)| *n == name) {
            out_sources.push(BasecampSource::Flake((*flake_ref).to_string()));
            out_origins.push(SourceOrigin::DepDefault {
                name: name.clone(),
                via: via.clone(),
            });
            continue;
        }
        // 4. Basecamp preinstalls it — no-op.
        if BASECAMP_PREINSTALLED_MODULES.iter().any(|m| *m == name) {
            continue;
        }
        // 5. Unknown: warn and skip.
        eprintln!(
            "warning: dep `{name}` declared by `{via}` is not pinned in scaffold defaults, \
             not in `[basecamp.dependencies]`, not in the source's flake.lock, and is not a \
             preinstalled basecamp module; skipping. Add it to `[basecamp.dependencies]` in \
             scaffold.toml, declare it as a flake input of the source, or run \
             `basecamp modules --flake <ref>#lgx` to capture explicitly."
        );
    }

    (out_sources, out_origins)
}

/// Pure replay of `state.project_sources` + `state.dependencies` into every
/// seeded profile. Dependencies build first (fail-fast on a broken companion
/// pin, before we invest time on project sources). If state has no captured
/// sources, transparently invokes `basecamp modules` in auto-discover mode
/// and reloads state, then proceeds.
///
/// No CLI source flags (`--flake`/`--path`/`--profile`) — the source set
/// lives in state, managed by `basecamp modules`. No selective profile
/// installs either (KISS); reset + install again if you want one profile
/// clean.
fn cmd_basecamp_install(project: Project, probe: &dyn LgxFlakeProbe) -> DynResult<()> {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let state = read_basecamp_state(&state_path)
        .ok()
        .filter(|s| !s.basecamp_bin.is_empty() && !s.lgpm_bin.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("basecamp not set up yet; run: logos-scaffold basecamp setup")
        })?;

    if !Path::new(&state.basecamp_bin).exists() || !Path::new(&state.lgpm_bin).exists() {
        bail!("basecamp not set up yet; run: logos-scaffold basecamp setup");
    }

    // If state has no captured sources, run `modules` in auto-discover mode
    // transparently. This preserves the "bare install on a fresh project
    // just works" story.
    let state = if state.total_sources() == 0 {
        println!("no modules captured yet; running `basecamp modules` for you");
        cmd_basecamp_modules(project.clone(), Vec::new(), Vec::new(), false, probe)?;
        read_basecamp_state(&state_path).with_context(|| {
            format!("re-read state after auto-capture: {}", state_path.display())
        })?
    } else {
        state
    };

    let cache_root = project.root.join(&project.config.cache_root);
    let lgx_cache = cache_root.join("basecamp/lgx-links");
    fs::create_dir_all(&lgx_cache).with_context(|| format!("create {}", lgx_cache.display()))?;

    let profiles_root = project.root.join(BASECAMP_PROFILES_REL);
    let target_profiles: Vec<String> = vec![
        BASECAMP_PROFILE_ALICE.to_string(),
        BASECAMP_PROFILE_BOB.to_string(),
    ];

    // Deps-first build order. fail-fast: a broken companion pin surfaces
    // before we invest nix build time on the dev's own modules.
    let ordered: Vec<BasecampSource> = state.all_sources().cloned().collect();
    if ordered.is_empty() {
        bail!(
            "no modules captured after auto-discovery; \
             run `basecamp modules --flake <ref>#lgx` to supply sources explicitly"
        );
    }

    println!("installing {} module(s) (deps first):", ordered.len());
    for src in &ordered {
        println!("  - {}", flake_ref(src));
    }

    let lgx_files = collect_lgx_files(&project.root, &ordered, &lgx_cache)?;
    if lgx_files.is_empty() {
        bail!("no .lgx files produced by the captured sources");
    }

    run_lgpm_install(
        &state.lgpm_bin,
        &profiles_root,
        &target_profiles,
        &lgx_files,
        true,
    )?;

    println!("install complete");
    Ok(())
}

/// Materialize every source into its `.lgx` files in the given cache dir. When
/// building a sub-flake, add `--override-input <sibling-dirname> path:<sibling>`
/// for every other path-flake sibling in `sources` so local edits in one
/// sub-flake flow into its dependents instead of pulling the sibling's master
/// branch from the network. Assumes the developer's layout matches flake input
/// names to sibling directory names (the scaffold convention for multi-flake
/// module repos).
fn collect_lgx_files(
    project_root: &Path,
    sources: &[BasecampSource],
    lgx_cache: &Path,
) -> DynResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    for src in sources {
        let overrides = match src {
            BasecampSource::Flake(target_ref) => {
                resolve_sibling_overrides(src, sources, target_ref)
            }
            BasecampSource::Path(_) => Vec::new(),
        };
        out.extend(materialize_lgx_files(
            project_root,
            src,
            lgx_cache,
            &overrides,
        )?);
    }
    Ok(out)
}

/// Compute the sibling `--override-input` flags for a flake source, filtered
/// down to inputs the target flake actually declares. Shared by `install` and
/// `build-portable` so both commands stay in sync on the override rules and
/// the "skipped override" warning format.
///
/// On `flake_declared_inputs` failure (malformed flake, transient nix issue),
/// returns the unfiltered list — nix will warn on unknown inputs rather than
/// fail the build.
fn resolve_sibling_overrides(
    src: &BasecampSource,
    all: &[BasecampSource],
    target_ref: &str,
) -> Vec<(String, String)> {
    let mut overrides = sibling_overrides_for(src, all);
    if overrides.is_empty() {
        return overrides;
    }
    if let Ok(declared) = flake_declared_inputs(target_ref) {
        let before = overrides.clone();
        overrides = retain_declared_overrides(overrides, &declared);
        for (name, value) in &before {
            if !overrides.iter().any(|(n, _)| n == name) {
                eprintln!(
                    "warning: skipping sibling override `{name}` -> `{value}`: \
                     not declared as an input of `{target_ref}`"
                );
            }
        }
    }
    overrides
}

/// Drop overrides for input names that aren't in `declared`. Pure function so
/// tests can exercise the filter without shelling out to nix.
fn retain_declared_overrides(
    overrides: Vec<(String, String)>,
    declared: &std::collections::HashSet<String>,
) -> Vec<(String, String)> {
    overrides
        .into_iter()
        .filter(|(name, _)| declared.contains(name))
        .collect()
}

/// Names of the inputs declared by `flake_ref`'s root flake. Implemented via
/// `nix flake metadata --json` — lighter than `nix eval` since it only reads
/// the lockfile / flake.nix inputs block, not any package expressions.
fn flake_declared_inputs(flake_ref: &str) -> DynResult<std::collections::HashSet<String>> {
    // `nix flake metadata` rejects fragments (`path:/p#lgx` → "unexpected fragment
    // 'lgx' in flake reference"). Strip anything after `#` so we hand it just the
    // flake itself.
    let bare = flake_ref.split_once('#').map_or(flake_ref, |(b, _)| b);
    let out = Command::new("nix")
        .arg("flake")
        .arg("metadata")
        .arg("--json")
        .arg(bare)
        .output()
        .with_context(|| format!("spawn nix flake metadata {bare}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!(
            "nix flake metadata {flake_ref} failed ({}): {}",
            out.status,
            stderr.trim()
        );
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("parse nix flake metadata JSON")?;
    let inputs = json
        .pointer("/locks/nodes/root/inputs")
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    Ok(inputs)
}

/// Rewrite user-supplied plain-relative and absolute filesystem flake refs to
/// the `path:<abs>#attr` form the resolver and sibling-override logic expect.
/// Refs already carrying a scheme (`path:`, `github:`, `git+`, `http[s]:`,
/// `gitlab:`) pass through untouched.
///
/// Without this normalization, `--flake ./sub#lgx` would end up in the
/// resolver as-is; `sibling_overrides_for` only matches `path:` refs, so
/// sibling inputs silently wouldn't be auto-overridden, and `build-portable`'s
/// cwd-override would fall through to the project root instead of the flake
/// directory. Normalizing at the command boundary keeps the downstream
/// code paths uniform.
fn normalize_flake_ref(project_root: &Path, flake_ref: &str) -> String {
    if flake_ref.starts_with("path:")
        || flake_ref.starts_with("github:")
        || flake_ref.starts_with("git+")
        || flake_ref.starts_with("http:")
        || flake_ref.starts_with("https:")
        || flake_ref.starts_with("gitlab:")
        || flake_ref.starts_with("sourcehut:")
    {
        return flake_ref.to_string();
    }
    let (path_part, frag) = flake_ref.split_once('#').unwrap_or((flake_ref, ""));
    let raw = if path_part.starts_with('/') {
        PathBuf::from(path_part)
    } else {
        project_root.join(path_part)
    };
    let canon = raw.canonicalize().unwrap_or(raw);
    if frag.is_empty() {
        format!("path:{}", canon.display())
    } else {
        format!("path:{}#{}", canon.display(), frag)
    }
}

/// Extract the absolute path from a `path:<abs>#attr` flake ref. Returns `None`
/// for non-path refs (`github:`, `git+`, etc.) — we can't safely override those
/// with a local path without user intent.
fn flake_path_prefix(flake_ref: &str) -> Option<&str> {
    let rest = flake_ref.strip_prefix("path:")?;
    let end = rest.find('#').unwrap_or(rest.len());
    Some(&rest[..end])
}

/// First path component of a relative subpath, e.g. `"cache"` for
/// `"cache/basecamp"`. Returns `None` if the path is empty or absolute.
fn first_path_component(rel: &str) -> Option<String> {
    Path::new(rel).components().find_map(|c| match c {
        Component::Normal(s) => s.to_str().map(|s| s.to_string()),
        _ => None,
    })
}

/// Compute the sibling-overrides list for `target`: every other path-flake in
/// `all` whose path lives under the same parent directory as `target`'s path.
/// Returned as `(input_name, "path:<abs>")` pairs ready for `--override-input`.
fn sibling_overrides_for(target: &BasecampSource, all: &[BasecampSource]) -> Vec<(String, String)> {
    let BasecampSource::Flake(target_ref) = target else {
        return Vec::new();
    };
    let Some(target_abs) = flake_path_prefix(target_ref) else {
        return Vec::new();
    };
    let Some(target_parent) = Path::new(target_abs).parent() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for other in all {
        let BasecampSource::Flake(other_ref) = other else {
            continue;
        };
        if other_ref == target_ref {
            continue;
        }
        let Some(other_abs) = flake_path_prefix(other_ref) else {
            continue;
        };
        let other_path = Path::new(other_abs);
        if other_path.parent() != Some(target_parent) {
            continue;
        }
        let Some(name) = other_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        out.push((name.to_string(), format!("path:{other_abs}")));
    }
    out.sort();
    out
}

/// `(modules_dir, plugins_dir)` under a profile's `XDG_DATA_HOME`. Pinning this
/// to one place keeps the layout knowledge from drifting between install/launch.
fn profile_modules_and_plugins(profiles_root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let xdg_app = profiles_root
        .join(name)
        .join("xdg-data")
        .join(BASECAMP_XDG_APP_SUBPATH);
    (xdg_app.join("modules"), xdg_app.join("plugins"))
}

/// Install every lgx file into every target profile via lgpm. `announce = true`
/// prints per-file progress (install path); `false` is silent (launch replay).
fn run_lgpm_install(
    lgpm_bin: &str,
    profiles_root: &Path,
    profiles: &[String],
    lgx_files: &[PathBuf],
    announce: bool,
) -> DynResult<()> {
    for name in profiles {
        let (modules_dir, plugins_dir) = profile_modules_and_plugins(profiles_root, name);
        fs::create_dir_all(&modules_dir)
            .with_context(|| format!("create {}", modules_dir.display()))?;
        fs::create_dir_all(&plugins_dir)
            .with_context(|| format!("create {}", plugins_dir.display()))?;
        for lgx in lgx_files {
            if announce {
                println!("installing {} into {}", lgx.display(), name);
            }
            let args = lgpm_install_args(&modules_dir, &plugins_dir, lgx);
            run_checked(
                Command::new(lgpm_bin).args(&args),
                &format!("lgpm install {} into {}", lgx.display(), name),
            )?;
        }
    }
    Ok(())
}

/// Used by launch replay: build lgx files from state-recorded sources and hand
/// them to lgpm for the given profile(s). No-op if state has no captured
/// sources. Dependencies are built and installed *before* project sources so a
/// broken companion pin surfaces before we invest nix build time on the dev's
/// own modules.
fn install_sources_into_profiles(
    state: &BasecampState,
    project_root: &Path,
    cache_root: &str,
    profiles_root: &Path,
    profiles: &[String],
) -> DynResult<()> {
    if state.total_sources() == 0 {
        return Ok(());
    }
    let lgx_cache = project_root.join(cache_root).join("basecamp/lgx-links");
    fs::create_dir_all(&lgx_cache).with_context(|| format!("create {}", lgx_cache.display()))?;
    let all: Vec<BasecampSource> = state.all_sources().cloned().collect();
    let lgx_files = collect_lgx_files(project_root, &all, &lgx_cache)?;
    run_lgpm_install(&state.lgpm_bin, profiles_root, profiles, &lgx_files, false)
}

/// Build the argv (after the binary) for `lgpm install --file <lgx>` with the given
/// modules/plugins dirs. Lifted to a pure function so tests can pin the shape.
fn lgpm_install_args(
    modules_dir: &Path,
    plugins_dir: &Path,
    lgx: &Path,
) -> Vec<std::ffi::OsString> {
    vec![
        "--modules-dir".into(),
        modules_dir.as_os_str().to_owned(),
        "--ui-plugins-dir".into(),
        plugins_dir.as_os_str().to_owned(),
        "install".into(),
        "--file".into(),
        lgx.as_os_str().to_owned(),
    ]
}

/// Produce the list of `.lgx` files referenced by a given source.
/// For `Path`, expects a single `.lgx` file — directories are rejected.
/// For `Flake`, runs `nix build <ref>` and collects `*.lgx` at the build result root.
fn materialize_lgx_files(
    project_root: &Path,
    src: &BasecampSource,
    cache: &Path,
    overrides: &[(String, String)],
) -> DynResult<Vec<PathBuf>> {
    match src {
        BasecampSource::Path(p) => {
            let pb = PathBuf::from(p);
            if pb.is_file() && pb.extension().is_some_and(|e| e == "lgx") {
                return Ok(vec![pb]);
            }
            bail!("path `{}` is not a .lgx file", pb.display());
        }
        BasecampSource::Flake(flake_ref) => {
            let link = cache.join(flake_out_link_name(flake_ref));
            let mut cmd = Command::new("nix");
            cmd.arg("build").arg(flake_ref);
            for (name, value) in overrides {
                cmd.arg("--override-input").arg(name).arg(value);
            }
            cmd.arg("--out-link").arg(&link);
            let log = derive_log_path(project_root, "install");
            run_logged(&mut cmd, &format!("building {flake_ref}"), &log)?;
            rotate_logs(project_root, "install", 10);
            list_lgx(&link)
        }
    }
}

/// Out-link filename for a user-supplied flake ref. Slugified for readability, with a
/// short hash suffix so two refs that slugify the same don't clobber each other's build.
/// Uses FNV-1a 64-bit so the suffix is deterministic across Rust versions — unlike
/// `DefaultHasher`, which may rehash differently between releases and invalidate
/// persisted `result` symlinks under the nix store after a compiler bump.
fn flake_out_link_name(flake_ref: &str) -> String {
    let slug: String = flake_ref
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let hash = fnv1a_64(flake_ref.as_bytes());
    format!("{slug}-{hash:016x}-result")
}

/// FNV-1a 64-bit. Stable, dependency-free, good enough for collision-avoidance
/// on short slugs. Not cryptographic.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn list_lgx(dir: &Path) -> DynResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "lgx") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

struct NixLgxProbe;

impl LgxFlakeProbe for NixLgxProbe {
    fn package_names(&self, flake_ref: &str) -> DynResult<Vec<String>> {
        let target = format!("{flake_ref}#packages.{}", nix_current_system());
        let out = Command::new("nix")
            .arg("eval")
            .arg("--json")
            .arg("--apply")
            .arg("x: builtins.attrNames x")
            .arg(&target)
            .output()
            .with_context(|| format!("spawn nix eval {target}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // A flake with no `packages.<system>` attribute is a normal resolver case
            // (e.g., a project whose flake only exposes devShells). Treat as empty.
            // Anything else — lockfile errors, syntax errors, network failures —
            // must propagate so the user sees the real reason instead of the generic
            // "no .lgx sources found" fallback.
            if stderr.contains("does not provide attribute") || stderr.contains("missing attribute")
            {
                return Ok(Vec::new());
            }
            bail!(
                "nix eval {target} failed ({}): {}",
                out.status,
                stderr.trim()
            );
        }
        let text = String::from_utf8(out.stdout).context("nix eval output not utf-8")?;
        let names: Vec<String> =
            serde_json::from_str(text.trim()).context("parse nix eval JSON")?;
        Ok(names)
    }
}

fn nix_current_system() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-darwin"
        } else {
            "x86_64-darwin"
        }
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else {
        "x86_64-linux"
    }
}

/// Map the current host's nix system string to the `.lgx` manifest `main`
/// variant key basecamp resolves against when loading a dev-build plugin.
/// Returns `None` for platforms we can't map (unknown nix systems).
pub(crate) fn platform_dev_variant_key() -> Option<&'static str> {
    match nix_current_system() {
        "x86_64-linux" => Some("linux-amd64-dev"),
        "aarch64-linux" => Some("linux-arm64-dev"),
        "aarch64-darwin" => Some("darwin-arm64-dev"),
        "x86_64-darwin" => Some("darwin-amd64-dev"),
        _ => None,
    }
}

/// A `modules/<name>/manifest.json` whose `main` object lacks the expected
/// dev-variant key for the current platform. Basecamp v0.1.1's variant
/// resolver will silently hang on first click when loading such a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestVariantIssue {
    pub(crate) profile: String,
    pub(crate) module_name: String,
    pub(crate) available_variants: Vec<String>,
}

/// Count installed modules under `<profile_dir>/xdg-data/<BASECAMP_XDG_APP_SUBPATH>/modules/`.
/// Returns 0 if the dir doesn't exist yet.
pub(crate) fn count_installed_modules(profile_dir: &Path) -> usize {
    let modules_root = profile_dir
        .join("xdg-data")
        .join(BASECAMP_XDG_APP_SUBPATH)
        .join("modules");
    let Ok(entries) = fs::read_dir(&modules_root) else {
        return 0;
    };
    entries.flatten().filter(|e| e.path().is_dir()).count()
}

/// Walk `<profile_dir>/xdg-data/<BASECAMP_XDG_APP_SUBPATH>/modules/*/manifest.json`
/// and flag modules missing the expected `-dev` variant key. Non-blocking:
/// callers decide whether to warn or fail based on the returned issues.
/// Silent on parse errors / missing files (not the check's job to police).
pub(crate) fn check_manifest_variants(
    profile_dir: &Path,
    profile_name: &str,
    expected_dev_variant: &str,
) -> Vec<ManifestVariantIssue> {
    let modules_root = profile_dir
        .join("xdg-data")
        .join(BASECAMP_XDG_APP_SUBPATH)
        .join("modules");
    let Ok(entries) = fs::read_dir(&modules_root) else {
        return Vec::new();
    };
    let mut issues = Vec::new();
    for entry in entries.flatten() {
        let module_dir = entry.path();
        if !module_dir.is_dir() {
            continue;
        }
        let manifest_path = module_dir.join("manifest.json");
        let Ok(text) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(json): Result<serde_json::Value, _> = serde_json::from_str(&text) else {
            continue;
        };
        let name = json
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                module_dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<unknown>")
                    .to_string()
            });
        let main = json.get("main");
        let available: Vec<String> = match main {
            Some(serde_json::Value::Object(m)) => m.keys().cloned().collect(),
            // `main` might be a plain string (single-platform modules) —
            // not variant-keyed, can't reason about -dev presence.
            _ => continue,
        };
        if !available.is_empty() && !available.iter().any(|k| k.as_str() == expected_dev_variant) {
            issues.push(ManifestVariantIssue {
                profile: profile_name.to_string(),
                module_name: name,
                available_variants: available,
            });
        }
    }
    issues
}

/// Difference between what `basecamp modules` would auto-discover today
/// and what's captured in `state.sources`. Used by `doctor` to flag stale
/// or missing captures without running the real capture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ModuleDriftReport {
    pub(crate) discovered_not_captured: Vec<BasecampSource>,
    pub(crate) captured_not_discovered: Vec<BasecampSource>,
}

impl ModuleDriftReport {
    pub(crate) fn is_empty(&self) -> bool {
        self.discovered_not_captured.is_empty() && self.captured_not_discovered.is_empty()
    }
}

/// Run the `basecamp modules` auto-discovery algorithm as a dry-run and diff
/// the result against `state` (union of project_sources + dependencies). Uses
/// the real `NixLgxProbe` internally — callers that need a fake probe for
/// testing should call the lower-level helpers directly.
pub(crate) fn compute_module_drift(project: &Project) -> DynResult<ModuleDriftReport> {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let state = read_basecamp_state(&state_path).unwrap_or_default();

    let cache_root_first = first_path_component(&project.config.cache_root);
    let mut skip_subdirs: Vec<&str> = BASECAMP_AUTODISCOVER_SKIP_SUBDIRS.iter().copied().collect();
    if let Some(c) = cache_root_first.as_deref() {
        if !skip_subdirs.contains(&c) {
            skip_subdirs.push(c);
        }
    }

    let probe = NixLgxProbe;
    let discovered_project = resolve_install_sources(
        &project.root,
        &[],
        &[],
        &probe,
        &skip_subdirs,
        "lgx",
        "lgx-portable",
    )
    .unwrap_or_default();

    let overrides = project
        .config
        .basecamp
        .as_ref()
        .map(|bc| bc.dependencies.clone())
        .unwrap_or_default();
    let (discovered_deps, _) = resolve_manifest_dependencies(&discovered_project, &overrides);

    let mut discovered: std::collections::HashSet<BasecampSource> =
        std::collections::HashSet::new();
    discovered.extend(discovered_project.iter().cloned());
    discovered.extend(discovered_deps.iter().cloned());

    let captured: std::collections::HashSet<BasecampSource> =
        state.all_sources().cloned().collect();

    let mut discovered_not_captured: Vec<BasecampSource> =
        discovered.difference(&captured).cloned().collect();
    discovered_not_captured.sort_by_key(flake_ref);
    let mut captured_not_discovered: Vec<BasecampSource> =
        captured.difference(&discovered).cloned().collect();
    captured_not_discovered.sort_by_key(flake_ref);

    Ok(ModuleDriftReport {
        discovered_not_captured,
        captured_not_discovered,
    })
}

/// `basecamp doctor` entry point. Builds a scaffold `DoctorReport` containing
/// only basecamp-specific rows (captured modules summary, manifest variant
/// check, module-set drift) and prints/serializes it via the shared doctor
/// formatting helpers.
///
/// Intentionally separate from top-level `logos-scaffold doctor` so basecamp
/// remains a self-contained subcommand surface. A follow-up PR may merge the
/// rows into the global doctor once the core basecamp feature has landed.
fn cmd_basecamp_doctor(project: Project, as_json: bool) -> DynResult<()> {
    use crate::commands::doctor::{finalize_report, print_report};
    use crate::model::CheckStatus;

    if as_json {
        crate::process::set_command_echo(false);
    }

    let mut rows = Vec::new();
    push_basecamp_doctor_rows(&project, &mut rows);

    // If there are no rows at all, state is absent or empty — emit a single
    // Pass row so the output is never confusingly blank.
    if rows.is_empty() {
        rows.push(crate::model::CheckRow {
            status: CheckStatus::Pass,
            name: "basecamp state".to_string(),
            detail: "not set up yet (no basecamp.state)".to_string(),
            remediation: Some("run `logos-scaffold basecamp setup`".to_string()),
        });
    }

    let report = finalize_report(rows);

    if as_json {
        crate::process::set_command_echo(true);
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }

    if report.summary.fail > 0 {
        bail!("basecamp doctor reported FAIL checks");
    }
    Ok(())
}

/// Build the basecamp-specific doctor rows: captured modules summary (each
/// source's flake ref + commit/tag + installed api headers), manifest variant
/// checks per seeded profile, and a module-set drift check against
/// auto-discovery. No-op when `basecamp.state` is absent.
fn push_basecamp_doctor_rows(project: &Project, rows: &mut Vec<crate::model::CheckRow>) {
    use crate::model::{CheckRow, CheckStatus};

    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let Ok(state) = read_basecamp_state(&state_path) else {
        return;
    };
    if state.basecamp_bin.is_empty() && state.lgpm_bin.is_empty() && state.total_sources() == 0 {
        return;
    }

    let profiles_root = project.root.join(BASECAMP_PROFILES_REL);
    let alice_modules = profiles_root
        .join(BASECAMP_PROFILE_ALICE)
        .join("xdg-data")
        .join(BASECAMP_XDG_APP_SUBPATH)
        .join("modules");

    for src in &state.project_sources {
        rows.push(captured_source_row("basecamp module", src, &alice_modules));
    }
    for src in &state.dependencies {
        rows.push(captured_source_row("basecamp dep", src, &alice_modules));
    }

    // Drift between a captured dep ref and scaffold's default for that dep.
    // Mirrors the `lez standalone pin` warning pattern: surfaces a rev
    // mismatch so the dev knows they're off-scaffold. Matches by module name
    // against `BASECAMP_DEPENDENCIES` entries; captured dep refs that aren't
    // github refs (rare) are skipped silently.
    for (name, default_ref) in BASECAMP_DEPENDENCIES {
        let Some(captured) = state.dependencies.iter().find_map(|src| match src {
            BasecampSource::Flake(f) => {
                let rest = f.strip_prefix("github:")?;
                let before_frag = rest.split_once('#').map_or(rest, |(b, _)| b);
                let repo = before_frag.split('/').nth(1)?;
                let candidate_name = repo.trim_start_matches("logos-").replace('-', "_");
                if candidate_name == *name {
                    Some(f.as_str())
                } else {
                    None
                }
            }
            _ => None,
        }) else {
            continue;
        };
        let captured_rev = github_flake_ref_rev(captured);
        let default_rev = github_flake_ref_rev(default_ref);
        let drifted = match (captured_rev, default_rev) {
            (Some(c), Some(d)) => c != d,
            _ => captured != *default_ref,
        };
        if drifted {
            rows.push(crate::model::CheckRow {
                status: crate::model::CheckStatus::Warn,
                name: format!("basecamp dep pin drift: {name}"),
                detail: format!(
                    "captured `{}` differs from scaffold default `{}`",
                    captured_rev.unwrap_or(captured),
                    default_rev.unwrap_or(default_ref),
                ),
                remediation: Some(format!(
                    "module may not work against a basecamp release built with the scaffold \
                     default. Either update the project's flake.lock to match, or set \
                     `[basecamp.dependencies].{name}` in scaffold.toml to a compatible rev."
                )),
            });
        }
    }

    if let Some(expected_dev) = platform_dev_variant_key() {
        for profile in [BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB] {
            let profile_dir = profiles_root.join(profile);
            if !profile_dir.is_dir() {
                continue;
            }
            for issue in check_manifest_variants(&profile_dir, profile, expected_dev) {
                rows.push(CheckRow {
                    status: CheckStatus::Warn,
                    name: format!(
                        "basecamp variant: {} in profile {}",
                        issue.module_name, issue.profile
                    ),
                    detail: format!(
                        "main=[{}] missing expected `{}`; plugin will hang on click",
                        issue.available_variants.join(","),
                        expected_dev
                    ),
                    remediation: Some(format!(
                        "rebuild `{}` so its manifest.json `main.{}` key is populated \
                         (upstream logos-module-builder issue); then re-run `basecamp install`",
                        issue.module_name, expected_dev
                    )),
                });
            }
        }
    }

    match compute_module_drift(project) {
        Ok(drift) if !drift.is_empty() => {
            for src in &drift.discovered_not_captured {
                rows.push(CheckRow {
                    status: CheckStatus::Warn,
                    name: "basecamp drift: uncaptured".to_string(),
                    detail: format!(
                        "discovered `{}` but not captured in basecamp.state",
                        flake_ref(src)
                    ),
                    remediation: Some(
                        "run `logos-scaffold basecamp modules` to refresh capture".to_string(),
                    ),
                });
            }
            for src in &drift.captured_not_discovered {
                rows.push(CheckRow {
                    status: CheckStatus::Warn,
                    name: "basecamp drift: stale".to_string(),
                    detail: format!(
                        "captured `{}` no longer discoverable (may be stale)",
                        flake_ref(src)
                    ),
                    remediation: Some(
                        "run `logos-scaffold basecamp modules` to refresh; \
                         or `--flake <ref>#lgx` / `--path <file.lgx>` to capture explicitly"
                            .to_string(),
                    ),
                });
            }
        }
        _ => {}
    }
}

/// One doctor row per captured source. Shows the flake ref verbatim plus a
/// tag/commit annotation for github refs and any `*.h` / `*.hpp` headers
/// already installed under alice's profile.
fn captured_source_row(
    label: &str,
    src: &BasecampSource,
    alice_modules: &Path,
) -> crate::model::CheckRow {
    use crate::model::{CheckRow, CheckStatus};
    let ref_text = flake_ref(src);
    let mut detail = ref_text.clone();

    if let BasecampSource::Flake(flake_ref) = src {
        if let Some(label) = github_ref_part_label(flake_ref) {
            detail.push_str(&format!("  ({label})"));
        }
        if let Some(module_name) = infer_module_name_from_flake_ref(flake_ref) {
            let headers = collect_api_headers(alice_modules, &module_name);
            if !headers.is_empty() {
                detail.push_str(&format!(
                    "\n    api headers: {}",
                    headers
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    CheckRow {
        status: CheckStatus::Pass,
        name: label.to_string(),
        detail,
        remediation: None,
    }
}

fn github_ref_part_label(flake_ref: &str) -> Option<String> {
    let rest = flake_ref.strip_prefix("github:")?;
    let before_frag = rest.split_once('#').map_or(rest, |(b, _)| b);
    let parts: Vec<&str> = before_frag.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    let ref_part = parts[2];
    let looks_like_commit = ref_part.len() >= 7
        && ref_part.len() <= 40
        && ref_part.chars().all(|c| c.is_ascii_hexdigit());
    if looks_like_commit {
        Some(format!("commit {}", &ref_part[..ref_part.len().min(12)]))
    } else {
        Some(format!("tag {ref_part}"))
    }
}

fn infer_module_name_from_flake_ref(flake_ref: &str) -> Option<String> {
    let before_frag = flake_ref.split_once('#').map_or(flake_ref, |(b, _)| b);
    if let Some(rest) = before_frag.strip_prefix("github:") {
        let repo = rest.split('/').nth(1)?;
        let trimmed = repo.trim_start_matches("logos-");
        return Some(trimmed.replace('-', "_"));
    }
    if let Some(rest) = before_frag.strip_prefix("path:") {
        return Some(
            Path::new(rest)
                .file_name()?
                .to_string_lossy()
                .replace('-', "_"),
        );
    }
    None
}

fn collect_api_headers(alice_modules: &Path, module_name: &str) -> Vec<PathBuf> {
    let module_dir = alice_modules.join(module_name);
    if !module_dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let candidates = [
        module_dir.clone(),
        module_dir.join("include"),
        module_dir.join("interfaces"),
    ];
    for dir in candidates {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if ext.eq_ignore_ascii_case("h") || ext.eq_ignore_ascii_case("hpp") {
                            out.push(path);
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// Probes a flake ref for the set of package names it exposes under the current system.
/// Returned list is used to detect `.#lgx` / `.#lgx-portable` in source resolution.
trait LgxFlakeProbe {
    fn package_names(&self, flake_ref: &str) -> DynResult<Vec<String>>;
}

/// Resolves the set of `.lgx` sources to install from explicit args and project auto-discovery.
///
/// Precedence (matches §2.2 of the basecamp-profiles spec):
/// 1. Explicit `--path` / `--flake` — wins if supplied.
/// 2. Project root `flake.nix` exposing `packages.<system>.<attr>` — build that attribute.
/// 3. Sub-directories with a `flake.nix` exposing the same.
/// 4. Project exposes only `.#<alt_attr>` — fail with a targeted hint pointing at it.
/// 5. No matching sources found anywhere — fail with a generic hint.
///
/// `attr` selects which flake output to target (`"lgx"` for `install`, `"lgx-portable"`
/// for `build-portable`). `alt_attr` is the variant reported in the fail-hint when only
/// the other one is present.
fn resolve_install_sources(
    project_root: &Path,
    explicit_paths: &[PathBuf],
    explicit_flakes: &[String],
    probe: &dyn LgxFlakeProbe,
    skip_subdirs: &[&str],
    attr: &str,
    alt_attr: &str,
) -> DynResult<Vec<BasecampSource>> {
    if !explicit_paths.is_empty() || !explicit_flakes.is_empty() {
        let mut out = Vec::new();
        for p in explicit_paths {
            out.push(BasecampSource::Path(p.display().to_string()));
        }
        for f in explicit_flakes {
            out.push(BasecampSource::Flake(f.clone()));
        }
        return Ok(out);
    }

    let mut found = Vec::new();
    let mut alt_only_dirs = Vec::new();
    let root_flake = project_root.join("flake.nix");
    if root_flake.is_file() {
        classify_flake_dir(
            project_root,
            probe,
            attr,
            alt_attr,
            &mut found,
            &mut alt_only_dirs,
        )?;
    }

    if found.is_empty() {
        for entry in fs::read_dir(project_root)
            .with_context(|| format!("read {}", project_root.display()))?
        {
            let entry =
                entry.with_context(|| format!("read entry in {}", project_root.display()))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') || skip_subdirs.iter().any(|s| *s == name) {
                continue;
            }
            if path.join("flake.nix").is_file() {
                classify_flake_dir(&path, probe, attr, alt_attr, &mut found, &mut alt_only_dirs)?;
            }
        }
    }

    if !found.is_empty() {
        found.sort_by_key(flake_ref);
        return Ok(found);
    }

    if !alt_only_dirs.is_empty() {
        alt_only_dirs.sort();
        bail!(
            "found `.#{alt_attr}` in {dirs} but no `.#{attr}` output.\n\
             Next step: expose `packages.<system>.{attr}` in your flake, or re-run with \
             `--flake <path>#{alt_attr}` to opt into the {alt_attr} variant explicitly.",
            dirs = alt_only_dirs.join(", "),
        );
    }

    bail!(
        "no `.lgx` sources found in this project.\n\
         Next step: expose `packages.<system>.{attr}` in a `flake.nix` at the project root \
         (or in a sub-directory), or pass `--path <file.lgx>` / `--flake <ref>#{attr}`."
    )
}

fn classify_flake_dir(
    dir: &Path,
    probe: &dyn LgxFlakeProbe,
    attr: &str,
    alt_attr: &str,
    found: &mut Vec<BasecampSource>,
    alt_only_dirs: &mut Vec<String>,
) -> DynResult<()> {
    debug_assert_ne!(
        attr, alt_attr,
        "classify_flake_dir requires distinct attr / alt_attr — the `else if` \
         would be unreachable and alt-only detection silently broken"
    );
    let flake_ref = format!("path:{}", dir.display());
    let names = probe.package_names(&flake_ref)?;
    // A flake that exposes only `alt_attr` (not the requested `attr`) is a failure case
    // handled by the caller with a targeted hint.
    if names.iter().any(|n| n == attr) {
        found.push(BasecampSource::Flake(format!("{flake_ref}#{attr}")));
    } else if names.iter().any(|n| n == alt_attr) {
        alt_only_dirs.push(dir.display().to_string());
    }
    Ok(())
}

pub(crate) fn flake_ref(src: &BasecampSource) -> String {
    match src {
        BasecampSource::Path(p) => p.clone(),
        BasecampSource::Flake(f) => f.clone(),
    }
}

/// Create XDG-rooted profile dirs under `profiles_root` for every named profile.
/// Returns the list of profile names that now exist (idempotent).
fn seed_profiles(profiles_root: &Path, names: &[&str]) -> DynResult<Vec<String>> {
    let mut seeded = Vec::new();
    for name in names {
        let profile_dir = profiles_root.join(name);
        for xdg in ["xdg-config", "xdg-data", "xdg-cache"] {
            let path = profile_dir.join(xdg).join(BASECAMP_XDG_APP_SUBPATH);
            fs::create_dir_all(&path)
                .with_context(|| format!("create profile dir {}", path.display()))?;
        }
        seeded.push(name.to_string());
    }
    Ok(seeded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use tempfile::tempdir;

    struct FakeProbe {
        answers: RefCell<HashMap<String, Vec<String>>>,
    }

    impl FakeProbe {
        fn new(answers: &[(&str, &[&str])]) -> Self {
            let mut map = HashMap::new();
            for (k, v) in answers {
                map.insert(k.to_string(), v.iter().map(|s| s.to_string()).collect());
            }
            Self {
                answers: RefCell::new(map),
            }
        }
    }

    impl LgxFlakeProbe for FakeProbe {
        fn package_names(&self, flake_ref: &str) -> DynResult<Vec<String>> {
            Ok(self
                .answers
                .borrow()
                .get(flake_ref)
                .cloned()
                .unwrap_or_default())
        }
    }

    #[test]
    fn resolve_install_sources_explicit_paths_win() {
        let tmp = tempdir().expect("tempdir");
        let probe = FakeProbe::new(&[]);
        let paths = vec![PathBuf::from("/a/mod.lgx")];
        let flakes = vec!["./sub#lgx".to_string()];
        let got = resolve_install_sources(
            tmp.path(),
            &paths,
            &flakes,
            &probe,
            &[],
            "lgx",
            "lgx-portable",
        )
        .expect("resolve");
        assert_eq!(
            got,
            vec![
                BasecampSource::Path("/a/mod.lgx".to_string()),
                BasecampSource::Flake("./sub#lgx".to_string()),
            ]
        );
    }

    #[test]
    fn resolve_install_sources_uses_root_flake_lgx() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        fs::write(root.join("flake.nix"), b"{}").unwrap();
        let root_ref = format!("path:{}", root.display());
        let probe = FakeProbe::new(&[(root_ref.as_str(), &["lgx", "default"])]);
        let got = resolve_install_sources(root, &[], &[], &probe, &[], "lgx", "lgx-portable")
            .expect("resolve");
        assert_eq!(got, vec![BasecampSource::Flake(format!("{root_ref}#lgx"))]);
    }

    #[test]
    fn resolve_install_sources_selects_portable_when_attr_is_lgx_portable() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        fs::write(root.join("flake.nix"), b"{}").unwrap();
        let root_ref = format!("path:{}", root.display());
        let probe = FakeProbe::new(&[(root_ref.as_str(), &["lgx", "lgx-portable"])]);
        let got = resolve_install_sources(root, &[], &[], &probe, &[], "lgx-portable", "lgx")
            .expect("resolve");
        assert_eq!(
            got,
            vec![BasecampSource::Flake(format!("{root_ref}#lgx-portable"))],
            "when requesting lgx-portable, the portable attr must win even if lgx is also present"
        );
    }

    #[test]
    fn resolve_install_sources_fails_when_requested_attr_absent() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        fs::write(root.join("flake.nix"), b"{}").unwrap();
        let root_ref = format!("path:{}", root.display());
        let probe = FakeProbe::new(&[(root_ref.as_str(), &["lgx"])]);
        let err = resolve_install_sources(root, &[], &[], &probe, &[], "lgx-portable", "lgx")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("lgx-portable") && msg.contains("lgx") && msg.contains("--flake"),
            "expected alt-only hint for requested lgx-portable, got: {msg}"
        );
    }

    #[test]
    fn resolve_install_sources_discovers_subflakes_when_root_missing_lgx() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let sub_a = root.join("tictactoe");
        let sub_b = root.join("tictactoe-ui");
        let hidden = root.join(".not-a-sub");
        for d in [&sub_a, &sub_b, &hidden] {
            fs::create_dir_all(d).unwrap();
            fs::write(d.join("flake.nix"), b"{}").unwrap();
        }
        let refs = [
            (format!("path:{}", sub_a.display()), vec!["lgx"]),
            (format!("path:{}", sub_b.display()), vec!["lgx"]),
            (format!("path:{}", hidden.display()), vec!["lgx"]),
        ];
        let answers: Vec<(&str, &[&str])> = refs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
            .collect();
        let probe = FakeProbe::new(&answers);
        let got = resolve_install_sources(root, &[], &[], &probe, &[], "lgx", "lgx-portable")
            .expect("resolve");
        assert_eq!(
            got,
            vec![
                BasecampSource::Flake(format!("path:{}#lgx", sub_a.display())),
                BasecampSource::Flake(format!("path:{}#lgx", sub_b.display())),
            ],
            "hidden dotdirs must be skipped; results must be sorted"
        );
    }

    #[test]
    fn resolve_install_sources_portable_only_fails_with_hint() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        fs::write(root.join("flake.nix"), b"{}").unwrap();
        let root_ref = format!("path:{}", root.display());
        let probe = FakeProbe::new(&[(root_ref.as_str(), &["lgx-portable"])]);
        let err = resolve_install_sources(root, &[], &[], &probe, &[], "lgx", "lgx-portable")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("lgx-portable") && msg.contains("--flake"),
            "expected portable-only hint, got: {msg}"
        );
    }

    #[test]
    fn resolve_install_sources_skips_named_subdirs() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        for name in ["target", "cache"] {
            let d = root.join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("flake.nix"), b"{}").unwrap();
        }
        let real = root.join("real-mod");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("flake.nix"), b"{}").unwrap();
        let answers_owned = vec![
            (
                format!("path:{}", root.join("target").display()),
                vec!["lgx"],
            ),
            (
                format!("path:{}", root.join("cache").display()),
                vec!["lgx"],
            ),
            (format!("path:{}", real.display()), vec!["lgx"]),
        ];
        let answers: Vec<(&str, &[&str])> = answers_owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
            .collect();
        let probe = FakeProbe::new(&answers);
        let got = resolve_install_sources(
            root,
            &[],
            &[],
            &probe,
            &["target", "cache"],
            "lgx",
            "lgx-portable",
        )
        .expect("resolve");
        assert_eq!(
            got,
            vec![BasecampSource::Flake(format!(
                "path:{}#lgx",
                real.display()
            ))],
            "skip_subdirs must prune target/cache even if they contain flake.nix"
        );
    }

    #[test]
    fn resolve_install_sources_no_lgx_anywhere_fails_with_generic_hint() {
        let tmp = tempdir().expect("tempdir");
        let probe = FakeProbe::new(&[]);
        let err = resolve_install_sources(tmp.path(), &[], &[], &probe, &[], "lgx", "lgx-portable")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("--path") && msg.contains("--flake"),
            "expected generic hint, got: {msg}"
        );
    }

    #[test]
    fn lgpm_install_args_pins_global_flags_before_subcommand() {
        let args = lgpm_install_args(
            Path::new("/p/modules"),
            Path::new("/p/plugins"),
            Path::new("/p/mod.lgx"),
        );
        let rendered: Vec<String> = args
            .iter()
            .map(|o| o.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            rendered,
            vec![
                "--modules-dir",
                "/p/modules",
                "--ui-plugins-dir",
                "/p/plugins",
                "install",
                "--file",
                "/p/mod.lgx",
            ],
            "lgpm expects global --modules-dir / --ui-plugins-dir BEFORE the `install` subcommand"
        );
    }

    #[test]
    fn sibling_overrides_pair_path_flakes_under_a_shared_parent() {
        let target = BasecampSource::Flake("path:/repo/tictactoe-ui-cpp#lgx".to_string());
        let all = vec![
            BasecampSource::Flake("path:/repo/tictactoe#lgx".to_string()),
            BasecampSource::Flake("path:/repo/tictactoe-ui-cpp#lgx".to_string()),
            // An unrelated path-flake under a different parent must NOT be paired.
            BasecampSource::Flake("path:/elsewhere/other#lgx".to_string()),
            // A .lgx path source is not a flake and must be ignored.
            BasecampSource::Path("/repo/prebuilt.lgx".to_string()),
            // A remote flake ref can't be overridden with a local path.
            BasecampSource::Flake("github:foo/bar#lgx".to_string()),
        ];
        let got = sibling_overrides_for(&target, &all);
        assert_eq!(
            got,
            vec![("tictactoe".to_string(), "path:/repo/tictactoe".to_string())]
        );
    }

    #[test]
    fn retain_declared_overrides_drops_unknown_names() {
        let declared: std::collections::HashSet<String> =
            ["tictactoe".to_string()].into_iter().collect();
        let kept = retain_declared_overrides(
            vec![
                ("tictactoe".to_string(), "path:/a".to_string()),
                ("tictactoe-ui-cpp".to_string(), "path:/b".to_string()),
                ("tictactoe-ui-qml".to_string(), "path:/c".to_string()),
            ],
            &declared,
        );
        assert_eq!(kept, vec![("tictactoe".to_string(), "path:/a".to_string())]);
    }

    #[test]
    fn retain_declared_overrides_empty_declared_drops_all() {
        let declared = std::collections::HashSet::new();
        let kept =
            retain_declared_overrides(vec![("x".to_string(), "path:/x".to_string())], &declared);
        assert!(kept.is_empty());
    }

    #[test]
    fn sibling_overrides_returns_empty_for_path_sources_and_remote_flakes() {
        let all = vec![
            BasecampSource::Flake("path:/repo/a#lgx".to_string()),
            BasecampSource::Flake("path:/repo/b#lgx".to_string()),
        ];
        assert!(
            sibling_overrides_for(&BasecampSource::Path("/anywhere.lgx".to_string()), &all)
                .is_empty(),
            "Path sources don't need overrides — they aren't built with nix"
        );
        assert!(
            sibling_overrides_for(&BasecampSource::Flake("github:x/y#lgx".to_string()), &all)
                .is_empty(),
            "remote flakes don't get sibling-override treatment (no shared local parent)"
        );
    }

    #[test]
    fn flake_out_link_name_avoids_slug_collisions() {
        // Two refs that would slugify to the same base name must not produce the same
        // out-link file, or one `nix build` will silently clobber the other.
        let a = flake_out_link_name("github:logos-co/x#lgx");
        let b = flake_out_link_name("github_logos_co_x_lgx");
        assert_ne!(a, b);
        assert!(a.ends_with("-result") && b.ends_with("-result"));
    }

    #[test]
    fn seed_profiles_creates_xdg_subdirs_for_each_name() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("profiles");

        let names = ["alice", "bob"];
        let seeded = seed_profiles(&root, &names).expect("seed");
        assert_eq!(seeded, vec!["alice".to_string(), "bob".to_string()]);

        for name in names {
            for xdg in ["xdg-config", "xdg-data", "xdg-cache"] {
                let dir = root.join(name).join(xdg).join(BASECAMP_XDG_APP_SUBPATH);
                assert!(dir.is_dir(), "expected XDG subdir at {}", dir.display());
            }
        }
    }

    #[test]
    fn launch_env_exports_xdg_under_profile_dir_and_profile_name() {
        let profile_dir = Path::new("/p/alice");
        let env = launch_env(profile_dir, "alice");
        assert_eq!(
            env.get("XDG_CONFIG_HOME").unwrap(),
            &OsString::from("/p/alice/xdg-config")
        );
        assert_eq!(
            env.get("XDG_DATA_HOME").unwrap(),
            &OsString::from("/p/alice/xdg-data")
        );
        assert_eq!(
            env.get("XDG_CACHE_HOME").unwrap(),
            &OsString::from("/p/alice/xdg-cache")
        );
        assert_eq!(env.get("LOGOS_PROFILE").unwrap(), &OsString::from("alice"));
    }

    #[test]
    fn scrub_removes_xdg_data_and_cache_but_keeps_config() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let profile_dir = root.join(".scaffold/basecamp/profiles/alice");
        for xdg in ["xdg-data", "xdg-cache", "xdg-config"] {
            let d = profile_dir.join(xdg);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("sentinel"), b"x").unwrap();
        }

        scrub_profile_data_and_cache(root, &profile_dir).expect("scrub");

        assert!(!profile_dir.join("xdg-data").exists());
        assert!(!profile_dir.join("xdg-cache").exists());
        assert!(
            profile_dir.join("xdg-config/sentinel").exists(),
            "xdg-config must be preserved (profile state the user may have edited)"
        );
    }

    #[test]
    fn scrub_refuses_paths_outside_profiles_root() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        fs::create_dir_all(root.join(".scaffold/basecamp/profiles")).unwrap();
        // Adversarial: profile_dir points somewhere else entirely.
        let outside = tmp.path().join("not-a-profile");
        fs::create_dir_all(outside.join("xdg-data")).unwrap();
        fs::write(outside.join("xdg-data/precious"), b"keep").unwrap();

        let err = scrub_profile_data_and_cache(root, &outside).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("refusing to scrub"), "got: {msg}");
        assert!(
            outside.join("xdg-data/precious").exists(),
            "scrub must not touch anything when the safety check fails"
        );
    }

    #[test]
    fn launch_pid_roundtrips_and_missing_returns_none() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("nested/launch.state");
        assert!(read_launch_pid(&path).is_none(), "missing file → None");

        write_launch_pid(&path, 42).expect("write");
        assert_eq!(read_launch_pid(&path), Some(42));

        fs::write(&path, "garbage\n").unwrap();
        assert!(read_launch_pid(&path).is_none(), "malformed file → None");
    }

    #[test]
    fn write_launch_pid_leaves_no_tmp_file_on_success() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("launch.state");
        write_launch_pid(&path, 99).expect("write");
        // The atomic-write helper uses `<path>.state.tmp` as its staging file.
        assert!(
            !path.with_extension("state.tmp").exists(),
            "tmp file must be renamed, not left behind"
        );
    }

    #[test]
    fn scrub_succeeds_on_first_run_when_safe_root_doesnt_exist_yet() {
        // Regression: canonicalize() requires every component to exist. Before the
        // fix, a first-run scrub against a fresh project errored out because
        // `.scaffold/basecamp/profiles` hadn't been created yet.
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let profile_dir = root.join(".scaffold/basecamp/profiles/alice");
        fs::create_dir_all(profile_dir.join("xdg-data")).unwrap();
        fs::create_dir_all(profile_dir.join("xdg-cache")).unwrap();
        scrub_profile_data_and_cache(root, &profile_dir).expect("scrub");
        assert!(!profile_dir.join("xdg-data").exists());
    }

    #[test]
    fn canonicalize_under_errors_when_parent_is_missing() {
        // Silent fallback would be a safety-check bypass — verify we fail loudly.
        let tmp = tempdir().expect("tempdir");
        let err = canonicalize_under(&tmp.path().join("no/such/parent/alice")).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("canonicalize"), "got: {msg}");
    }

    #[test]
    fn basecamp_comm_name_truncates_to_15_bytes_like_proc_comm() {
        assert_eq!(
            basecamp_comm_name("/nix/store/xyz/bin/basecamp"),
            "basecamp"
        );
        assert_eq!(
            basecamp_comm_name("/x/extremely-long-binary-name"),
            "extremely-long-"
        );
    }

    #[test]
    fn pid_comm_matches_returns_false_for_reserved_pid_0() {
        // PID 0 is reserved and `ps -p 0` reliably fails, standing in for any PID
        // where we can't recover a comm — the helper must fail closed so the kill
        // path is skipped rather than firing at a wrong target.
        assert!(!pid_comm_matches(0, "basecamp"));
    }

    #[test]
    fn seed_profiles_is_idempotent() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("profiles");
        seed_profiles(&root, &["alice"]).expect("first");
        // Drop a sentinel file inside the xdg-data dir; a second seed must not delete it.
        let sentinel = root
            .join("alice/xdg-data")
            .join(BASECAMP_XDG_APP_SUBPATH)
            .join("keep-me.txt");
        fs::write(&sentinel, b"hi").expect("write sentinel");
        seed_profiles(&root, &["alice"]).expect("second");
        assert!(
            sentinel.exists(),
            "second seed must not scrub existing contents"
        );
    }

    // ---- reset helpers ----

    #[test]
    fn plan_reset_lines_snapshot() {
        let tmp = tempdir().expect("tempdir");
        let profiles_root = tmp.path().join(BASECAMP_PROFILES_REL);
        let live = vec![
            ("alice".to_string(), 1234u32, "logos-basecamp".to_string()),
            ("bob".to_string(), 5678u32, "logos-basecamp".to_string()),
        ];
        let lines = plan_reset_lines(&live, &profiles_root, 3, &["alice", "bob"]);
        let joined = lines.join("\n");
        assert!(
            joined.contains("reset plan:"),
            "plan must lead with `reset plan:`, got:\n{joined}"
        );
        assert!(
            joined.contains("kill pid 1234 (profile: alice"),
            "missing alice kill line in:\n{joined}"
        );
        assert!(
            joined.contains("kill pid 5678 (profile: bob"),
            "missing bob kill line in:\n{joined}"
        );
        assert!(
            joined.contains(&format!("rm -rf {}", profiles_root.display())),
            "missing rm line for {} in:\n{joined}",
            profiles_root.display()
        );
        assert!(
            joined.contains("clear basecamp.state sources (3 recorded)"),
            "missing source-clear line in:\n{joined}"
        );
        assert!(
            joined.contains("re-seed profiles: alice, bob"),
            "missing re-seed line in:\n{joined}"
        );
    }

    #[test]
    fn plan_reset_lines_shows_no_kills_when_none_live() {
        let tmp = tempdir().expect("tempdir");
        let profiles_root = tmp.path().join(BASECAMP_PROFILES_REL);
        let lines = plan_reset_lines(&[], &profiles_root, 0, &["alice", "bob"]);
        let joined = lines.join("\n");
        assert!(
            joined.contains("kill: none"),
            "empty kill list must be explicit, got:\n{joined}"
        );
        assert!(
            joined.contains("clear basecamp.state sources (0 recorded)"),
            "zero-sources count must still be printed for transparency, got:\n{joined}"
        );
    }

    #[test]
    fn remove_profiles_root_succeeds_on_missing_dir() {
        let tmp = tempdir().expect("tempdir");
        let project_root = tmp.path();
        // .scaffold/basecamp/profiles/ absent — must be a no-op.
        remove_profiles_root(project_root).expect("idempotent on missing dir");
        assert!(!project_root.join(BASECAMP_PROFILES_REL).exists());
    }

    #[test]
    fn remove_profiles_root_wipes_existing_tree() {
        let tmp = tempdir().expect("tempdir");
        let project_root = tmp.path();
        let profiles_root = project_root.join(BASECAMP_PROFILES_REL);
        let inner = profiles_root.join("alice/xdg-data/junk");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("file.txt"), b"contents").unwrap();

        remove_profiles_root(project_root).expect("remove");
        assert!(!profiles_root.exists(), "profiles root must be gone");
    }

    #[cfg(unix)]
    #[test]
    fn remove_profiles_root_refuses_symlink_escape() {
        // If .scaffold/basecamp/profiles/ is a symlink pointing outside the
        // project, the safety guard must refuse rather than follow it.
        let tmp = tempdir().expect("tempdir");
        let project_root = tmp.path();
        let scaffold_dir = project_root.join(".scaffold/basecamp");
        fs::create_dir_all(&scaffold_dir).unwrap();

        let outside = tempdir().expect("outside tempdir");
        let outside_file = outside.path().join("important.txt");
        fs::write(&outside_file, b"do not delete").unwrap();

        std::os::unix::fs::symlink(outside.path(), scaffold_dir.join("profiles"))
            .expect("make symlink");

        let err = remove_profiles_root(project_root).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.to_lowercase().contains("refus") || msg.to_lowercase().contains("outside"),
            "expected safety rejection, got: {msg}"
        );
        assert!(
            outside_file.exists(),
            "symlinked-to file must not be deleted"
        );
    }

    // ---- build-portable helpers ----

    #[test]
    fn build_portable_nix_invocation_path_ref_cds_into_flake_dir() {
        let inv = build_portable_nix_invocation("path:/abs/to/foo#lgx-portable", &[]);
        assert_eq!(inv.cwd_override.as_deref(), Some(Path::new("/abs/to/foo")));
        assert_eq!(
            inv.args,
            vec!["build", ".#lgx-portable", "--print-out-paths"]
        );
    }

    #[test]
    fn build_portable_nix_invocation_remote_ref_stays_in_project_root() {
        let inv = build_portable_nix_invocation("github:foo/bar#lgx-portable", &[]);
        assert!(
            inv.cwd_override.is_none(),
            "remote refs must not override cwd"
        );
        assert_eq!(
            inv.args,
            vec!["build", "github:foo/bar#lgx-portable", "--print-out-paths"]
        );
    }

    #[test]
    fn build_portable_nix_invocation_does_not_use_out_link() {
        // Spec: `nix build` without `-o`, so the default `./result-<attr>` symlink
        // lands next to the flake. No `--out-link`, no `--no-link`.
        let inv = build_portable_nix_invocation("path:/abs/a#lgx-portable", &[]);
        for forbidden in ["-o", "--out-link", "--no-link"] {
            assert!(
                !inv.args.iter().any(|a| a == forbidden),
                "argv must not contain `{forbidden}`: {:?}",
                inv.args
            );
        }
    }

    #[test]
    fn build_portable_nix_invocation_inserts_overrides_before_print_out_paths() {
        let inv = build_portable_nix_invocation(
            "path:/abs/ui#lgx-portable",
            &[("core".to_string(), "path:/abs/core".to_string())],
        );
        assert_eq!(
            inv.args,
            vec![
                "build",
                ".#lgx-portable",
                "--override-input",
                "core",
                "path:/abs/core",
                "--print-out-paths",
            ]
        );
    }

    #[test]
    fn build_portable_resolve_path_returns_canonical_absolute_lgx() {
        let tmp = tempdir().expect("tempdir");
        let p = tmp.path().join("module.lgx");
        fs::write(&p, b"fake lgx").unwrap();
        let got = build_portable_resolve_path(&p).expect("ok");
        assert!(got.is_absolute(), "got: {}", got.display());
        assert_eq!(got.canonicalize().unwrap(), p.canonicalize().unwrap());
    }

    #[test]
    fn build_portable_resolve_path_rejects_non_lgx_extension() {
        let tmp = tempdir().expect("tempdir");
        let p = tmp.path().join("module.txt");
        fs::write(&p, b"not lgx").unwrap();
        let err = build_portable_resolve_path(&p).unwrap_err();
        assert!(
            format!("{err}").contains("not a .lgx file"),
            "expected extension-rejection hint, got: {err}"
        );
    }

    // ---- normalize_flake_ref (I2 fix) ----

    #[test]
    fn normalize_flake_ref_rewrites_relative_path_to_path_scheme() {
        let tmp = tempdir().expect("tempdir");
        let sub = tmp.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        let got = normalize_flake_ref(tmp.path(), "./sub#lgx-portable");
        assert!(got.starts_with("path:"), "got: {got}");
        assert!(got.ends_with("#lgx-portable"), "got: {got}");
        let canon = sub.canonicalize().unwrap();
        assert!(
            got.contains(canon.to_str().unwrap()),
            "expected canonical sub path in {got}"
        );
    }

    #[test]
    fn normalize_flake_ref_rewrites_absolute_path_to_path_scheme() {
        let tmp = tempdir().expect("tempdir");
        let sub = tmp.path().join("abs-sub");
        fs::create_dir_all(&sub).unwrap();
        let abs_spec = format!("{}#lgx", sub.display());
        let got = normalize_flake_ref(Path::new("/"), &abs_spec);
        assert!(got.starts_with("path:"), "got: {got}");
        assert!(got.ends_with("#lgx"), "got: {got}");
    }

    #[test]
    fn normalize_flake_ref_passes_through_scheme_refs() {
        let tmp = tempdir().expect("tempdir");
        for input in [
            "path:/abs/p#lgx",
            "github:foo/bar#lgx-portable",
            "git+https://example/r#lgx",
            "https://example/archive.tar.gz#lgx",
            "gitlab:foo/bar#lgx",
        ] {
            assert_eq!(
                normalize_flake_ref(tmp.path(), input),
                input,
                "scheme ref must pass through unchanged"
            );
        }
    }

    #[test]
    fn normalize_flake_ref_preserves_missing_fragment() {
        let tmp = tempdir().expect("tempdir");
        let sub = tmp.path().join("nofrag");
        fs::create_dir_all(&sub).unwrap();
        let got = normalize_flake_ref(tmp.path(), "./nofrag");
        assert!(
            got.starts_with("path:") && !got.contains('#'),
            "no fragment means no `#` in output: {got}"
        );
    }

    // ---- resolve_sibling_overrides (I1 fix) — exercised via the command paths;
    //      a focused test pins the helper's Path-source short-circuit ----

    #[test]
    fn resolve_sibling_overrides_returns_empty_for_isolated_flake() {
        let only = BasecampSource::Flake("path:/abs/alone#lgx".to_string());
        let got =
            resolve_sibling_overrides(&only, std::slice::from_ref(&only), "path:/abs/alone#lgx");
        assert!(got.is_empty(), "no siblings → no overrides");
    }

    #[test]
    fn clear_basecamp_sources_preserves_binaries_and_pin() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("basecamp.state");
        let original = BasecampState {
            pin: "deadbeef".to_string(),
            basecamp_bin: "/nix/store/a/bin/basecamp".to_string(),
            lgpm_bin: "/nix/store/b/bin/lgpm".to_string(),
            project_sources: vec![
                BasecampSource::Flake("path:/abs/tictactoe#lgx".to_string()),
                BasecampSource::Path("/mod.lgx".to_string()),
            ],
            dependencies: vec![BasecampSource::Flake(
                "github:logos-co/logos-delivery-module/1.0.0#lgx".to_string(),
            )],
        };
        write_basecamp_state(&path, &original).expect("seed state");

        clear_basecamp_sources(&path).expect("clear");

        let loaded = read_basecamp_state(&path).expect("reload");
        assert_eq!(loaded.pin, "deadbeef");
        assert_eq!(loaded.basecamp_bin, "/nix/store/a/bin/basecamp");
        assert_eq!(loaded.lgpm_bin, "/nix/store/b/bin/lgpm");
        assert!(
            loaded.project_sources.is_empty() && loaded.dependencies.is_empty(),
            "both project_sources and dependencies must be cleared"
        );
    }

    // ---- `basecamp modules` helpers (manifest walking + dep resolution) ----

    fn seed_module_metadata(dir: &Path, name: &str, deps: &[&str]) {
        let metadata = serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "type": "core",
            "main": "plugin",
            "dependencies": deps,
        });
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("metadata.json"), metadata.to_string()).unwrap();
    }

    #[test]
    fn read_source_metadata_dependencies_parses_array() {
        let tmp = tempdir().expect("tempdir");
        seed_module_metadata(tmp.path(), "mymod", &["delivery_module", "storage_module"]);
        let src = BasecampSource::Flake(format!("path:{}#lgx", tmp.path().display()));
        let deps = read_source_metadata_dependencies(&src);
        assert_eq!(
            deps,
            vec!["delivery_module".to_string(), "storage_module".to_string()]
        );
    }

    #[test]
    fn read_source_metadata_dependencies_returns_empty_for_missing_metadata() {
        let tmp = tempdir().expect("tempdir");
        let src = BasecampSource::Flake(format!("path:{}#lgx", tmp.path().display()));
        assert!(read_source_metadata_dependencies(&src).is_empty());
    }

    #[test]
    fn read_source_metadata_dependencies_returns_empty_for_path_sources() {
        let src = BasecampSource::Path("/abs/prebuilt.lgx".to_string());
        assert!(read_source_metadata_dependencies(&src).is_empty());
    }

    #[test]
    fn read_source_metadata_dependencies_returns_empty_for_remote_flakes() {
        let src = BasecampSource::Flake("github:foo/bar#lgx".to_string());
        assert!(read_source_metadata_dependencies(&src).is_empty());
    }

    #[test]
    fn resolve_manifest_dependencies_uses_project_override_over_default() {
        let tmp = tempdir().expect("tempdir");
        seed_module_metadata(tmp.path(), "mymod", &["delivery_module"]);
        let project = BasecampSource::Flake(format!("path:{}#lgx", tmp.path().display()));

        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert(
            "delivery_module".to_string(),
            "github:custom/delivery-fork#lgx".to_string(),
        );

        let (deps, origins) = resolve_manifest_dependencies(&[project], &overrides);
        assert_eq!(
            deps,
            vec![BasecampSource::Flake(
                "github:custom/delivery-fork#lgx".to_string()
            )]
        );
        assert!(matches!(origins[0], SourceOrigin::DepOverride { .. }));
    }

    #[test]
    fn resolve_manifest_dependencies_falls_back_to_scaffold_default() {
        let tmp = tempdir().expect("tempdir");
        seed_module_metadata(tmp.path(), "mymod", &["delivery_module"]);
        let project = BasecampSource::Flake(format!("path:{}#lgx", tmp.path().display()));

        let overrides = std::collections::BTreeMap::new();
        let (deps, origins) = resolve_manifest_dependencies(&[project], &overrides);
        assert_eq!(deps.len(), 1);
        // Scaffold default for delivery_module (see constants::BASECAMP_DEPENDENCIES).
        match &deps[0] {
            BasecampSource::Flake(f) => assert!(
                f.contains("logos-delivery-module"),
                "expected scaffold default delivery ref, got {f}"
            ),
            _ => panic!("expected Flake"),
        }
        assert!(matches!(origins[0], SourceOrigin::DepDefault { .. }));
    }

    #[test]
    fn resolve_manifest_dependencies_silently_skips_preinstalled_modules() {
        let tmp = tempdir().expect("tempdir");
        // `capability_module` is preinstalled by basecamp; must not be captured.
        seed_module_metadata(
            tmp.path(),
            "mymod",
            &["capability_module", "package_manager"],
        );
        let project = BasecampSource::Flake(format!("path:{}#lgx", tmp.path().display()));
        let (deps, origins) =
            resolve_manifest_dependencies(&[project], &std::collections::BTreeMap::new());
        assert!(deps.is_empty(), "preinstalled modules must not be captured");
        assert!(origins.is_empty());
    }

    #[test]
    fn resolve_manifest_dependencies_skips_project_internal_deps() {
        // Two sibling project sources where one declares a dep on the other.
        let tmp = tempdir().expect("tempdir");
        let core = tmp.path().join("core");
        let ui = tmp.path().join("ui");
        seed_module_metadata(&core, "core", &[]);
        seed_module_metadata(&ui, "ui", &["core"]);

        let project = vec![
            BasecampSource::Flake(format!("path:{}#lgx", core.display())),
            BasecampSource::Flake(format!("path:{}#lgx", ui.display())),
        ];

        let (deps, _) = resolve_manifest_dependencies(&project, &std::collections::BTreeMap::new());
        assert!(
            deps.is_empty(),
            "project-internal deps should not produce external captures"
        );
    }

    #[test]
    fn github_flake_ref_rev_extracts_middle_segment() {
        assert_eq!(
            github_flake_ref_rev("github:logos-co/logos-delivery-module/1fde1566#lgx"),
            Some("1fde1566")
        );
        assert_eq!(
            github_flake_ref_rev("github:foo/bar/1.0.0#lgx"),
            Some("1.0.0")
        );
    }

    #[test]
    fn github_flake_ref_rev_returns_none_for_non_github() {
        assert_eq!(github_flake_ref_rev("path:/abs#lgx"), None);
        assert_eq!(github_flake_ref_rev("github:foo/bar#lgx"), None);
    }

    #[test]
    fn swap_flake_attr_rewrites_matching_suffix() {
        assert_eq!(
            swap_flake_attr("path:/abs/foo#lgx", "lgx", "lgx-portable"),
            "path:/abs/foo#lgx-portable"
        );
        assert_eq!(
            swap_flake_attr(
                "github:logos-co/logos-delivery-module/1.0.0#lgx",
                "lgx",
                "lgx-portable"
            ),
            "github:logos-co/logos-delivery-module/1.0.0#lgx-portable"
        );
    }

    #[test]
    fn swap_flake_attr_leaves_non_matching_attr_untouched() {
        assert_eq!(
            swap_flake_attr("path:/abs#lgx-dev", "lgx", "lgx-portable"),
            "path:/abs#lgx-dev"
        );
        assert_eq!(
            swap_flake_attr("path:/abs", "lgx", "lgx-portable"),
            "path:/abs"
        );
    }

    // ---- doctor helpers (github_ref_part_label, infer_module_name_from_flake_ref) ----

    #[test]
    fn github_ref_part_label_recognizes_commit() {
        assert_eq!(
            github_ref_part_label("github:owner/repo/a746cdbc521f72ee22c5a4856fd17a9802bb9d69#lgx"),
            Some("commit a746cdbc521f".to_string())
        );
    }

    #[test]
    fn github_ref_part_label_recognizes_tag() {
        assert_eq!(
            github_ref_part_label("github:logos-co/logos-delivery-module/1.0.0#lgx"),
            Some("tag 1.0.0".to_string())
        );
        assert_eq!(
            github_ref_part_label("github:logos-co/logos-delivery-module/tutorial-v1#lgx"),
            Some("tag tutorial-v1".to_string())
        );
    }

    #[test]
    fn github_ref_part_label_returns_none_for_non_github() {
        assert_eq!(github_ref_part_label("path:/abs/sub#lgx"), None);
        assert_eq!(github_ref_part_label("git+https://example#lgx"), None);
    }

    #[test]
    fn infer_module_name_from_github() {
        assert_eq!(
            infer_module_name_from_flake_ref("github:logos-co/logos-delivery-module/1.0.0#lgx"),
            Some("delivery_module".to_string())
        );
    }

    #[test]
    fn infer_module_name_from_path() {
        assert_eq!(
            infer_module_name_from_flake_ref("path:/abs/tictactoe-ui-cpp#lgx"),
            Some("tictactoe_ui_cpp".to_string())
        );
    }

    #[test]
    fn source_origin_annotations_render_distinctly() {
        assert_eq!(SourceOrigin::Explicit.annotation(), "[explicit]");
        assert_eq!(
            SourceOrigin::AutoDiscovered.annotation(),
            "[auto-discovered]"
        );
        let def = SourceOrigin::DepDefault {
            name: "delivery_module".to_string(),
            via: "path:/abs/ui#lgx".to_string(),
        };
        assert!(def.annotation().contains("delivery_module"));
        assert!(def.annotation().contains("scaffold default"));
        let over = SourceOrigin::DepOverride {
            name: "delivery_module".to_string(),
            via: "path:/abs/ui#lgx".to_string(),
        };
        assert!(over.annotation().contains("scaffold.toml override"));
    }
}
