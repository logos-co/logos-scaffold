use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};

use crate::constants::{
    BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB, BASECAMP_URL, BASECAMP_XDG_APP_SUBPATH,
    DEFAULT_BASECAMP_PIN, DEFAULT_LGPM_FLAKE,
};
use crate::model::{BasecampSource, BasecampState, Project, RepoRef};
use crate::process::run_checked;
use crate::project::{load_project, save_project_config};
use crate::repo::{sync_repo_to_pin, RepoSyncOptions};
use crate::state::{read_basecamp_state, write_basecamp_state};
use crate::DynResult;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields wired up in later phases
pub(crate) enum BasecampAction {
    Setup,
    Install {
        paths: Vec<PathBuf>,
        flakes: Vec<String>,
        profile: Option<String>,
    },
    Launch {
        profile: String,
        no_clean: bool,
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
        BasecampAction::Install {
            paths,
            flakes,
            profile,
        } => cmd_basecamp_install(project, paths, flakes, profile, &NixLgxProbe),
        BasecampAction::Launch { profile, no_clean } => {
            cmd_basecamp_launch(project, profile, no_clean)
        }
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

    let basecamp_bin = build_basecamp_app(&basecamp_repo_path, &pin_artifacts)?;
    let lgpm_bin = build_lgpm(&pin_artifacts, bc.lgpm_flake.as_str())?;

    let profiles_root = project.root.join(".scaffold/basecamp/profiles");
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
        sources: existing.sources,
    };
    write_basecamp_state(&state_path, &state)?;

    project.config.basecamp = Some(bc);
    save_project_config(&project)?;

    println!("setup complete");
    Ok(())
}

fn build_basecamp_app(repo: &Path, out_dir: &Path) -> DynResult<PathBuf> {
    println!("building basecamp");
    let link = out_dir.join("app-result");
    run_checked(
        Command::new("nix")
            .current_dir(repo)
            .arg("build")
            .arg(".#app")
            .arg("--out-link")
            .arg(&link),
        "nix build .#app (basecamp)",
    )?;
    resolve_basecamp_binary(&link)
}

fn build_lgpm(out_dir: &Path, override_flake: &str) -> DynResult<PathBuf> {
    println!("building lgpm");
    let link = out_dir.join("lgpm-result");
    let flake_ref = if override_flake.is_empty() {
        DEFAULT_LGPM_FLAKE.to_string()
    } else {
        override_flake.to_string()
    };
    run_checked(
        Command::new("nix")
            .arg("build")
            .arg(&flake_ref)
            .arg("--out-link")
            .arg(&link),
        &format!("nix build {flake_ref} (lgpm)"),
    )?;
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
fn cmd_basecamp_launch(
    project: Project,
    profile: String,
    no_clean: bool,
) -> DynResult<()> {
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

    let profiles_root = project.root.join(".scaffold/basecamp/profiles");
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
        // Only issue signals if the PID's current comm matches; otherwise skip and
        // clear the stale record so the next launch doesn't keep checking.
        if pid_comm_matches(pid, &expected_comm) {
            kill_process_tree(pid);
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

    let env = launch_env(&profile_dir, &profile);
    write_launch_pid(&launch_state_path, std::process::id())?;

    println!("launching basecamp for profile {profile}");
    let mut cmd = Command::new(&state.basecamp_bin);
    for (k, v) in &env {
        cmd.env(k, v);
    }
    // Per-module port-override env vars (spec §3.4) are owned by each module and
    // flow in via a registry — empty in v1 since no modules have published names.
    // Concurrent alice/bob on the same host may collide on module-level ports
    // until upstreams adopt overrides; see basecamp-profiles §3.4.
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
    let safe_root = project_root.join(".scaffold/basecamp/profiles");
    // canonicalize() requires every path component to exist. Create safe_root up
    // front so the canonical form is well-defined before the prefix check. For
    // profile_dir we canonicalize the parent (which must exist — we just made it)
    // and append the final component, matching how the path would resolve.
    fs::create_dir_all(&safe_root)
        .with_context(|| format!("create {}", safe_root.display()))?;
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
            fs::remove_dir_all(&dir)
                .with_context(|| format!("scrub {}", dir.display()))?;
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
        fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let tmp = path.with_extension("state.tmp");
    fs::write(&tmp, format!("pid={pid}\n"))
        .with_context(|| format!("write {}", tmp.display()))?;
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
    // ps may print a leading path (BSD) or just the comm (Linux). Compare
    // basenames and tolerate the 15-byte comm truncation either side.
    let comm_base = Path::new(&comm)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&comm);
    let len = expected.len().min(comm_base.len()).min(15);
    len > 0 && comm_base[..len] == expected[..len]
}

/// Kill the process tree rooted at `pid`. Sends SIGTERM to descendants and the
/// process itself, waits briefly, then escalates with SIGKILL. Always KILLs
/// descendants on the second pass — the parent may exit before its children,
/// leaving them orphaned to init while still bound to profile ports.
fn kill_process_tree(pid: u32) {
    let _ = Command::new("pkill")
        .arg("-TERM")
        .arg("-P")
        .arg(pid.to_string())
        .status();
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
    let parent_exited = wait_for_exit(pid, Duration::from_millis(1500));
    let _ = Command::new("pkill")
        .arg("-KILL")
        .arg("-P")
        .arg(pid.to_string())
        .status();
    if !parent_exited {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
        let _ = wait_for_exit(pid, Duration::from_millis(500));
    }
}

fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        // `kill -0` returns non-zero when the PID no longer exists (or we lack perms).
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status();
        if matches!(status, Ok(s) if !s.success()) || status.is_err() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn cmd_basecamp_install(
    project: Project,
    paths: Vec<PathBuf>,
    flakes: Vec<String>,
    profile: Option<String>,
    probe: &dyn LgxFlakeProbe,
) -> DynResult<()> {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let state = read_basecamp_state(&state_path)
        .ok()
        .filter(|s| !s.basecamp_bin.is_empty() && !s.lgpm_bin.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "basecamp not set up yet; run: logos-scaffold basecamp setup"
            )
        })?;

    if !Path::new(&state.basecamp_bin).exists() || !Path::new(&state.lgpm_bin).exists() {
        bail!("basecamp not set up yet; run: logos-scaffold basecamp setup");
    }

    let cache_root = project.root.join(&project.config.cache_root);
    let lgx_cache = cache_root.join("basecamp/lgx-links");
    let skip_subdirs: Vec<&str> = vec![project.config.cache_root.as_str(), "target", "node_modules"];
    let sources = resolve_install_sources(&project.root, &paths, &flakes, probe, &skip_subdirs)?;

    fs::create_dir_all(&lgx_cache)
        .with_context(|| format!("create {}", lgx_cache.display()))?;

    let profiles_root = project.root.join(".scaffold/basecamp/profiles");
    let target_profiles: Vec<String> = match profile.as_deref() {
        Some(name) => {
            let profile_dir = profiles_root.join(name);
            if !profile_dir.is_dir() {
                bail!(
                    "profile `{name}` does not exist under {}; run `logos-scaffold basecamp setup` first",
                    profiles_root.display()
                );
            }
            vec![name.to_string()]
        }
        None => vec![
            BASECAMP_PROFILE_ALICE.to_string(),
            BASECAMP_PROFILE_BOB.to_string(),
        ],
    };

    // Commit resolved sources to state *before* invoking lgpm. If an install later
    // fails mid-loop, the sources list still reflects what the user asked for, so
    // `launch` (which scrubs + reinstalls) can reproduce the intended state.
    let mut merged = state.sources.clone();
    for src in &sources {
        if !merged.iter().any(|e| e == src) {
            merged.push(src.clone());
        }
    }
    let new_state = BasecampState {
        sources: merged,
        ..state.clone()
    };
    write_basecamp_state(&state_path, &new_state)?;

    let lgx_files = collect_lgx_files(&sources, &lgx_cache)?;
    if lgx_files.is_empty() {
        bail!("no .lgx files produced by the resolved sources");
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
fn collect_lgx_files(sources: &[BasecampSource], lgx_cache: &Path) -> DynResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    for src in sources {
        let mut overrides = sibling_overrides_for(src, sources);
        if let (false, BasecampSource::Flake(target_ref)) = (overrides.is_empty(), src) {
            // On failure (malformed flake, transient nix issue, …) we pass the
            // unfiltered list — nix will warn on unknown inputs rather than fail.
            if let Ok(declared) = flake_declared_inputs(target_ref) {
                overrides = retain_declared_overrides(overrides, &declared);
            }
        }
        out.extend(materialize_lgx_files(src, lgx_cache, &overrides)?);
    }
    Ok(out)
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

/// Extract the absolute path from a `path:<abs>#attr` flake ref. Returns `None`
/// for non-path refs (`github:`, `git+`, etc.) — we can't safely override those
/// with a local path without user intent.
fn flake_path_prefix(flake_ref: &str) -> Option<&str> {
    let rest = flake_ref.strip_prefix("path:")?;
    let end = rest.find('#').unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Compute the sibling-overrides list for `target`: every other path-flake in
/// `all` whose path lives under the same parent directory as `target`'s path.
/// Returned as `(input_name, "path:<abs>")` pairs ready for `--override-input`.
fn sibling_overrides_for(
    target: &BasecampSource,
    all: &[BasecampSource],
) -> Vec<(String, String)> {
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
/// them to lgpm for the given profile(s). No-op if `state.sources` is empty.
fn install_sources_into_profiles(
    state: &BasecampState,
    project_root: &Path,
    cache_root: &str,
    profiles_root: &Path,
    profiles: &[String],
) -> DynResult<()> {
    if state.sources.is_empty() {
        return Ok(());
    }
    let lgx_cache = project_root.join(cache_root).join("basecamp/lgx-links");
    fs::create_dir_all(&lgx_cache)
        .with_context(|| format!("create {}", lgx_cache.display()))?;
    let lgx_files = collect_lgx_files(&state.sources, &lgx_cache)?;
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
/// For `Path`, accepts either a single `.lgx` file or a directory (all `*.lgx` inside).
/// For `Flake`, runs `nix build <ref>` and collects `*.lgx` at the build result root.
fn materialize_lgx_files(
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
            println!("building {flake_ref}");
            let mut cmd = Command::new("nix");
            cmd.arg("build").arg(flake_ref);
            for (name, value) in overrides {
                cmd.arg("--override-input").arg(name).arg(value);
            }
            cmd.arg("--out-link").arg(&link);
            run_checked(&mut cmd, &format!("nix build {flake_ref}"))?;
            list_lgx(&link)
        }
    }
}

/// Out-link filename for a user-supplied flake ref. Slugified for readability, with a
/// short hash suffix so two refs that slugify the same don't clobber each other's build.
fn flake_out_link_name(flake_ref: &str) -> String {
    let slug: String = flake_ref
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&flake_ref, &mut hasher);
    let hash = std::hash::Hasher::finish(&hasher);
    format!("{slug}-{hash:016x}-result")
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
            if stderr.contains("does not provide attribute")
                || stderr.contains("missing attribute")
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

/// Probes a flake ref for the set of package names it exposes under the current system.
/// Returned list is used to detect `.#lgx` vs `.#lgx-portable` in source resolution.
trait LgxFlakeProbe {
    fn package_names(&self, flake_ref: &str) -> DynResult<Vec<String>>;
}

/// Resolves the set of `.lgx` sources to install from explicit args and project auto-discovery.
///
/// Precedence (matches §2.2 of the basecamp-profiles spec):
/// 1. Explicit `--path` / `--flake` — wins if supplied.
/// 2. Project root `flake.nix` exposing `packages.<system>.lgx` — build `.#lgx`.
/// 3. Sub-directories with a `flake.nix` exposing `packages.<system>.lgx`.
/// 4. Project exposes only `.#lgx-portable` — fail with a targeted hint.
/// 5. No `.lgx` sources found anywhere — fail with a generic hint.
fn resolve_install_sources(
    project_root: &Path,
    explicit_paths: &[PathBuf],
    explicit_flakes: &[String],
    probe: &dyn LgxFlakeProbe,
    skip_subdirs: &[&str],
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
    let mut portable_only_dirs = Vec::new();
    let root_flake = project_root.join("flake.nix");
    if root_flake.is_file() {
        classify_flake_dir(project_root, probe, &mut found, &mut portable_only_dirs)?;
    }

    if found.is_empty() {
        for entry in fs::read_dir(project_root)
            .with_context(|| format!("read {}", project_root.display()))?
        {
            let entry = entry.with_context(|| format!("read entry in {}", project_root.display()))?;
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
                classify_flake_dir(&path, probe, &mut found, &mut portable_only_dirs)?;
            }
        }
    }

    if !found.is_empty() {
        found.sort_by_key(flake_ref);
        return Ok(found);
    }

    if !portable_only_dirs.is_empty() {
        portable_only_dirs.sort();
        bail!(
            "found `.#lgx-portable` in {} but no `.#lgx` output.\n\
             Next step: expose `packages.<system>.lgx` in your flake, or re-run with \
             `--flake <path>#lgx-portable` to opt into the portable variant explicitly.",
            portable_only_dirs.join(", ")
        );
    }

    bail!(
        "no `.lgx` sources found in this project.\n\
         Next step: expose `packages.<system>.lgx` in a `flake.nix` at the project root \
         (or in a sub-directory), or pass `--path <file.lgx>` / `--flake <ref>#lgx`."
    )
}

fn classify_flake_dir(
    dir: &Path,
    probe: &dyn LgxFlakeProbe,
    found: &mut Vec<BasecampSource>,
    portable_only_dirs: &mut Vec<String>,
) -> DynResult<()> {
    let flake_ref = format!("path:{}", dir.display());
    let names = probe.package_names(&flake_ref)?;
    let has_lgx = names.iter().any(|n| n == "lgx");
    let has_portable = names.iter().any(|n| n == "lgx-portable");
    if has_lgx {
        found.push(BasecampSource::Flake(format!("{flake_ref}#lgx")));
    } else if has_portable {
        portable_only_dirs.push(dir.display().to_string());
    }
    Ok(())
}

fn flake_ref(src: &BasecampSource) -> String {
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
                map.insert(
                    k.to_string(),
                    v.iter().map(|s| s.to_string()).collect(),
                );
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
        let got = resolve_install_sources(tmp.path(), &paths, &flakes, &probe, &[])
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
    fn resolve_install_sources_prefers_root_flake_lgx() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        fs::write(root.join("flake.nix"), b"{}").unwrap();
        let root_ref = format!("path:{}", root.display());
        let probe = FakeProbe::new(&[(root_ref.as_str(), &["lgx", "default"])]);
        let got = resolve_install_sources(root, &[], &[], &probe, &[]).expect("resolve");
        assert_eq!(got, vec![BasecampSource::Flake(format!("{root_ref}#lgx"))]);
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
        let got = resolve_install_sources(root, &[], &[], &probe, &[]).expect("resolve");
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
        let err = resolve_install_sources(root, &[], &[], &probe, &[]).unwrap_err();
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
            (format!("path:{}", root.join("target").display()), vec!["lgx"]),
            (format!("path:{}", root.join("cache").display()), vec!["lgx"]),
            (format!("path:{}", real.display()), vec!["lgx"]),
        ];
        let answers: Vec<(&str, &[&str])> = answers_owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
            .collect();
        let probe = FakeProbe::new(&answers);
        let got = resolve_install_sources(root, &[], &[], &probe, &["target", "cache"])
            .expect("resolve");
        assert_eq!(
            got,
            vec![BasecampSource::Flake(format!("path:{}#lgx", real.display()))],
            "skip_subdirs must prune target/cache even if they contain flake.nix"
        );
    }

    #[test]
    fn resolve_install_sources_no_lgx_anywhere_fails_with_generic_hint() {
        let tmp = tempdir().expect("tempdir");
        let probe = FakeProbe::new(&[]);
        let err = resolve_install_sources(tmp.path(), &[], &[], &probe, &[]).unwrap_err();
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
        let kept = retain_declared_overrides(
            vec![("x".to_string(), "path:/x".to_string())],
            &declared,
        );
        assert!(kept.is_empty());
    }

    #[test]
    fn sibling_overrides_returns_empty_for_path_sources_and_remote_flakes() {
        let all = vec![
            BasecampSource::Flake("path:/repo/a#lgx".to_string()),
            BasecampSource::Flake("path:/repo/b#lgx".to_string()),
        ];
        assert!(
            sibling_overrides_for(
                &BasecampSource::Path("/anywhere.lgx".to_string()),
                &all
            )
            .is_empty(),
            "Path sources don't need overrides — they aren't built with nix"
        );
        assert!(
            sibling_overrides_for(
                &BasecampSource::Flake("github:x/y#lgx".to_string()),
                &all
            )
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
                let dir = root
                    .join(name)
                    .join(xdg)
                    .join(BASECAMP_XDG_APP_SUBPATH);
                assert!(
                    dir.is_dir(),
                    "expected XDG subdir at {}",
                    dir.display()
                );
            }
        }
    }

    #[test]
    fn launch_env_exports_xdg_under_profile_dir_and_profile_name() {
        let profile_dir = Path::new("/p/alice");
        let env = launch_env(profile_dir, "alice");
        assert_eq!(env.get("XDG_CONFIG_HOME").unwrap(), &OsString::from("/p/alice/xdg-config"));
        assert_eq!(env.get("XDG_DATA_HOME").unwrap(), &OsString::from("/p/alice/xdg-data"));
        assert_eq!(env.get("XDG_CACHE_HOME").unwrap(), &OsString::from("/p/alice/xdg-cache"));
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
        assert!(!path.with_extension("state.tmp").exists(), "tmp file must be renamed, not left behind");
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
        assert_eq!(basecamp_comm_name("/nix/store/xyz/bin/basecamp"), "basecamp");
        assert_eq!(basecamp_comm_name("/x/extremely-long-binary-name"), "extremely-long-");
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
        assert!(sentinel.exists(), "second seed must not scrub existing contents");
    }
}
