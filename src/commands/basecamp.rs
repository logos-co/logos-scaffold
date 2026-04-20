use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        // Phase 4/5 stubs: load_project() above is intentional so "outside project"
        // errors precede "not implemented" — future implementers must preserve that order.
        BasecampAction::Launch { .. } => bail!("basecamp launch is not yet implemented"),
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

    let mut lgx_files: Vec<PathBuf> = Vec::new();
    for src in &sources {
        lgx_files.extend(materialize_lgx_files(src, &lgx_cache)?);
    }
    if lgx_files.is_empty() {
        bail!("no .lgx files produced by the resolved sources");
    }

    for name in &target_profiles {
        let xdg_app = profiles_root
            .join(name)
            .join("xdg-data")
            .join(BASECAMP_XDG_APP_SUBPATH);
        let modules_dir = xdg_app.join("modules");
        let plugins_dir = xdg_app.join("plugins");
        fs::create_dir_all(&modules_dir)
            .with_context(|| format!("create {}", modules_dir.display()))?;
        fs::create_dir_all(&plugins_dir)
            .with_context(|| format!("create {}", plugins_dir.display()))?;
        for lgx in &lgx_files {
            println!("installing {} into {}", lgx.display(), name);
            let args = lgpm_install_args(&modules_dir, &plugins_dir, lgx);
            run_checked(
                Command::new(&state.lgpm_bin).args(&args),
                &format!("lgpm install {} into {}", lgx.display(), name),
            )?;
        }
    }

    println!("install complete");
    Ok(())
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
fn materialize_lgx_files(src: &BasecampSource, cache: &Path) -> DynResult<Vec<PathBuf>> {
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
            run_checked(
                Command::new("nix")
                    .arg("build")
                    .arg(flake_ref)
                    .arg("--out-link")
                    .arg(&link),
                &format!("nix build {flake_ref}"),
            )?;
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
