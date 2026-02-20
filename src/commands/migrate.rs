use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    FRAMEWORK_KIND_LSSA_LANG, LSSA_URL,
};
use crate::process::run_capture;
use crate::project::{load_project, run_in_project_dir, save_project_config};
use crate::template::project::{
    apply_overlay_if_missing, overlay_variant_files, OverlayRenderContext,
};
use crate::DynResult;

pub(crate) fn cmd_migrate(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err(
            "usage: logos-scaffold migrate --to lssa-lang [project-path] [--dry-run]".into(),
        );
    }

    let mut dry_run = false;
    let mut target_framework: Option<String> = None;
    let mut project_dir: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            "--to" => {
                let value = args.get(i + 1).ok_or("--to requires value")?;
                target_framework = Some(value.clone());
                i += 2;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag for migrate: {other}").into());
            }
            path => {
                if project_dir.is_none() {
                    project_dir = Some(PathBuf::from(path));
                } else {
                    return Err(format!("unexpected argument `{path}` for migrate").into());
                }
                i += 1;
            }
        }
    }

    let to = target_framework.ok_or("missing `--to` target. Usage: --to lssa-lang")?;
    if to != FRAMEWORK_KIND_LSSA_LANG {
        return Err(
            format!("unsupported migration target `{to}`. Supported target: `lssa-lang`.").into(),
        );
    }

    run_in_project_dir(project_dir.as_deref(), || {
        migrate_to_lssa_lang_current_project(dry_run)
    })
}

fn migrate_to_lssa_lang_current_project(dry_run: bool) -> DynResult<()> {
    let mut project = load_project()?;
    assert_no_merge_conflicts(&project.root)?;

    let dirty_files = collect_dirty_files(&project.root);
    if !dirty_files.is_empty() {
        println!("WARN: git working tree has uncommitted changes:");
        for file in &dirty_files {
            println!("  - {file}");
        }
    }

    let overlay_ctx = OverlayRenderContext {
        crate_name: project
            .root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("program_deployment"),
        lssa_pin: &project.config.lssa.pin,
    };

    let overlay_files = overlay_variant_files(FRAMEWORK_KIND_LSSA_LANG)?;
    let files_to_add: Vec<PathBuf> = overlay_files
        .into_iter()
        .filter(|relative| !project.root.join(relative).exists())
        .collect();
    let files_to_patch = planned_patch_files(&project.root, &project.config.lssa.pin)?;

    println!(
        "Migration target: {} (dry-run={})",
        FRAMEWORK_KIND_LSSA_LANG, dry_run
    );
    println!("Files to add: {}", files_to_add.len());
    for rel in &files_to_add {
        println!("  + {}", rel.display());
    }
    println!("Config to update: scaffold.toml");
    println!("Files to patch: {}", files_to_patch.len());
    for rel in &files_to_patch {
        println!("  ~ {rel}");
    }

    if dry_run {
        return Ok(());
    }

    let backup_root = project
        .root
        .join(".scaffold/migrations")
        .join(migration_timestamp());
    backup_files(
        &project.root,
        &backup_root,
        &[
            project.root.join("scaffold.toml"),
            project.root.join("Cargo.toml"),
            project.root.join("methods/guest/Cargo.toml"),
            project.root.join("src/lib.rs"),
        ],
    )?;

    apply_overlay_if_missing(&project.root, FRAMEWORK_KIND_LSSA_LANG, &overlay_ctx)?;
    patch_existing_files_for_lssa_lang(&project.root, &project.config.lssa.pin)?;

    project.config.framework.kind = FRAMEWORK_KIND_LSSA_LANG.to_string();
    project.config.framework.version = DEFAULT_FRAMEWORK_VERSION.to_string();
    project.config.framework.idl.spec = DEFAULT_FRAMEWORK_IDL_SPEC.to_string();
    project.config.framework.idl.path = DEFAULT_FRAMEWORK_IDL_PATH.to_string();
    save_project_config(&project)?;

    println!("Migration complete.");
    println!("Backup snapshot: {}", backup_root.display());
    Ok(())
}

fn migration_timestamp() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", since_epoch.as_secs())
}

fn backup_files(project_root: &Path, backup_root: &Path, files: &[PathBuf]) -> DynResult<()> {
    for path in files {
        if !path.exists() {
            continue;
        }
        let relative = path
            .strip_prefix(project_root)
            .map_err(|_| format!("cannot compute backup relative path for {}", path.display()))?;
        let backup_path = backup_root.join(relative);
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(path, backup_path)?;
    }
    Ok(())
}

fn patch_existing_files_for_lssa_lang(project_root: &Path, lssa_pin: &str) -> DynResult<()> {
    patch_root_cargo_toml(&project_root.join("Cargo.toml"), lssa_pin)?;
    patch_methods_guest_cargo_toml(&project_root.join("methods/guest/Cargo.toml"))?;
    patch_src_lib(&project_root.join("src/lib.rs"))?;
    Ok(())
}

fn patch_root_cargo_toml(path: &Path, lssa_pin: &str) -> DynResult<()> {
    let mut content = fs::read_to_string(path)?;
    let changed = patch_root_cargo_content(&mut content, lssa_pin)?;

    if changed {
        write_text_file(path, &content)?;
    }

    Ok(())
}

fn patch_methods_guest_cargo_toml(path: &Path) -> DynResult<()> {
    let mut content = fs::read_to_string(path)?;
    let changed = patch_methods_guest_cargo_content(&mut content)?;

    if changed {
        write_text_file(path, &content)?;
    }

    Ok(())
}

fn patch_src_lib(path: &Path) -> DynResult<()> {
    let content = fs::read_to_string(path)?;
    let Some(updated) = patch_src_lib_content(&content) else {
        return Ok(());
    };
    write_text_file(path, &updated)
}

fn patch_root_cargo_content(content: &mut String, lssa_pin: &str) -> DynResult<bool> {
    let mut changed = false;

    if ensure_workspace_members(
        content,
        &[
            "crates/lssa-idl-spec",
            "crates/lssa-lang-macros",
            "crates/lssa-lang",
            "crates/lssa-client-gen",
        ],
    )? {
        changed = true;
    }

    let workspace_common = format!("common = {{ git = \"{LSSA_URL}\", rev = \"{lssa_pin}\" }}");
    if ensure_section_lines(
        content,
        "workspace.dependencies",
        &[
            workspace_common.as_str(),
            "serde = { version = \"1.0.228\", features = [\"derive\"] }",
            "serde_json = \"1.0.149\"",
            "sha2 = \"0.10.9\"",
            "lssa_idl_spec = { path = \"crates/lssa-idl-spec\" }",
            "lssa_lang_macros = { path = \"crates/lssa-lang-macros\" }",
            "lssa_lang = { path = \"crates/lssa-lang\" }",
        ],
    )? {
        changed = true;
    }

    if ensure_section_lines(
        content,
        "dependencies",
        &["common.workspace = true", "serde.workspace = true"],
    )? {
        changed = true;
    }

    Ok(changed)
}

fn patch_methods_guest_cargo_content(content: &mut String) -> DynResult<bool> {
    ensure_section_lines(
        content,
        "dependencies",
        &[
            "serde.workspace = true",
            "serde_json.workspace = true",
            "lssa_lang.workspace = true",
        ],
    )
}

fn patch_src_lib_content(content: &str) -> Option<String> {
    if content.contains("pub mod generated;") {
        return None;
    }

    let mut updated = String::new();
    updated.push_str("#[allow(dead_code)]\npub mod generated;\n\n");
    updated.push_str(content);
    Some(updated)
}

fn planned_patch_files(project_root: &Path, lssa_pin: &str) -> DynResult<Vec<String>> {
    let mut out = Vec::new();

    let root_cargo = project_root.join("Cargo.toml");
    if root_cargo.exists() {
        let mut content = fs::read_to_string(&root_cargo)?;
        if patch_root_cargo_content(&mut content, lssa_pin)? {
            out.push("Cargo.toml".to_string());
        }
    }

    let methods_guest_cargo = project_root.join("methods/guest/Cargo.toml");
    if methods_guest_cargo.exists() {
        let mut content = fs::read_to_string(&methods_guest_cargo)?;
        if patch_methods_guest_cargo_content(&mut content)? {
            out.push("methods/guest/Cargo.toml".to_string());
        }
    }

    let src_lib = project_root.join("src/lib.rs");
    if src_lib.exists() {
        let content = fs::read_to_string(&src_lib)?;
        if patch_src_lib_content(&content).is_some() {
            out.push("src/lib.rs".to_string());
        }
    }

    Ok(out)
}

fn ensure_workspace_members(content: &mut String, members: &[&str]) -> DynResult<bool> {
    let mut lines: Vec<String> = content.lines().map(ToOwned::to_owned).collect();
    let members_line = lines
        .iter()
        .position(|line| line.trim() == "members = [")
        .ok_or("failed to find `members = [` in Cargo.toml during migration")?;

    let closing_line = lines
        .iter()
        .enumerate()
        .skip(members_line + 1)
        .find_map(|(idx, line)| (line.trim() == "]").then_some(idx))
        .ok_or("failed to find closing `]` for workspace members in Cargo.toml")?;

    let mut insert_at = closing_line;
    let mut changed = false;

    for member in members {
        let entry = format!("\"{member}\"");
        let exists = lines[members_line + 1..insert_at]
            .iter()
            .any(|line| line.contains(&entry));
        if exists {
            continue;
        }
        lines.insert(insert_at, format!("  \"{member}\","));
        insert_at += 1;
        changed = true;
    }

    if changed {
        *content = join_lines(&lines, content.ends_with('\n'));
    }

    Ok(changed)
}

fn ensure_section_lines(
    content: &mut String,
    section: &str,
    required_lines: &[&str],
) -> DynResult<bool> {
    let header = format!("[{section}]");
    let mut lines: Vec<String> = content.lines().map(ToOwned::to_owned).collect();

    let mut section_start = lines.iter().position(|line| line.trim() == header);
    if section_start.is_none() {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        section_start = Some(lines.len() - 1);
    }
    let section_start = section_start.expect("section start should be set");

    let mut section_end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(section_start + 1) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section_end = idx;
            break;
        }
    }

    let mut changed = false;
    for required in required_lines {
        let exists = lines[section_start + 1..section_end]
            .iter()
            .any(|line| line.trim() == required.trim());
        if exists {
            continue;
        }
        lines.insert(section_end, required.to_string());
        section_end += 1;
        changed = true;
    }

    if changed {
        *content = join_lines(&lines, content.ends_with('\n'));
    }

    Ok(changed)
}

fn join_lines(lines: &[String], had_trailing_newline: bool) -> String {
    let mut out = lines.join("\n");
    if had_trailing_newline || !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn write_text_file(path: &Path, text: &str) -> DynResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text.as_bytes())?;
    Ok(())
}

fn assert_no_merge_conflicts(root: &Path) -> DynResult<()> {
    let out = run_capture(
        Command::new("git")
            .current_dir(root)
            .arg("diff")
            .arg("--name-only")
            .arg("--diff-filter=U"),
        "git diff --name-only --diff-filter=U",
    );

    let Ok(out) = out else {
        return Ok(());
    };

    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !lines.is_empty() {
        return Err(format!(
            "merge conflicts detected; aborting migration. Resolve conflicts first: {}",
            lines.join(", ")
        )
        .into());
    }

    Ok(())
}

fn collect_dirty_files(root: &Path) -> Vec<String> {
    let out = run_capture(
        Command::new("git")
            .current_dir(root)
            .arg("status")
            .arg("--porcelain"),
        "git status --porcelain",
    );
    let Ok(out) = out else {
        return Vec::new();
    };

    out.stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        patch_methods_guest_cargo_content, patch_root_cargo_content, patch_src_lib_content,
    };

    #[test]
    fn patch_root_cargo_is_idempotent() {
        let mut text = r#"
[workspace]
members = [
  ".",
  "methods",
  "methods/guest",
]

[workspace.dependencies]
nssa = { git = "https://github.com/logos-blockchain/lssa.git", rev = "pin" }
nssa_core = { git = "https://github.com/logos-blockchain/lssa.git", rev = "pin" }
wallet = { git = "https://github.com/logos-blockchain/lssa.git", rev = "pin" }

[dependencies]
nssa.workspace = true
nssa_core.workspace = true
wallet.workspace = true
"#
        .trim_start()
        .to_string();

        let changed = patch_root_cargo_content(&mut text, "pin").expect("patch should work");
        assert!(changed);
        assert!(text.contains("crates/lssa-lang"));
        assert!(text.contains("common.workspace = true"));
        assert!(text.contains("serde.workspace = true"));

        let changed_again =
            patch_root_cargo_content(&mut text, "pin").expect("second patch should work");
        assert!(!changed_again);
    }

    #[test]
    fn patch_methods_guest_cargo_is_idempotent() {
        let mut text = r#"
[dependencies]
nssa_core.workspace = true
"#
        .trim_start()
        .to_string();

        let changed =
            patch_methods_guest_cargo_content(&mut text).expect("patch should be successful");
        assert!(changed);
        assert!(text.contains("serde.workspace = true"));
        assert!(text.contains("serde_json.workspace = true"));
        assert!(text.contains("lssa_lang.workspace = true"));

        let changed_again =
            patch_methods_guest_cargo_content(&mut text).expect("second patch should work");
        assert!(!changed_again);
    }

    #[test]
    fn patch_src_lib_is_idempotent() {
        let input = "#[allow(dead_code)]\npub mod runner_support {}\n";
        let patched = patch_src_lib_content(input).expect("generated module should be added");
        assert!(patched.starts_with("#[allow(dead_code)]\npub mod generated;\n"));

        assert!(patch_src_lib_content(&patched).is_none());
    }
}
