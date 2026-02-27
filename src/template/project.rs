use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};
use include_dir::{include_dir, Dir};

use crate::state::write_text;
use crate::DynResult;

static TEMPLATES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

pub(crate) struct OverlayRenderContext<'a> {
    pub(crate) crate_name: &'a str,
    pub(crate) lssa_pin: &'a str,
}

pub(crate) fn apply_overlay(
    target: &Path,
    variant: &str,
    ctx: &OverlayRenderContext<'_>,
) -> DynResult<()> {
    apply_overlay_variant(target, variant, ctx)?;
    ensure_scaffold_in_gitignore(target)
}

fn ensure_scaffold_in_gitignore(target: &Path) -> DynResult<()> {
    let gitignore_path = target.join(".gitignore");
    let mut content = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };

    let already_present = content.lines().any(|l| l.trim() == ".scaffold");
    if !already_present {
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str(".scaffold\n");
        write_text(&gitignore_path, &content)?;
    }
    Ok(())
}

fn apply_overlay_variant(
    target: &Path,
    variant: &str,
    ctx: &OverlayRenderContext<'_>,
) -> DynResult<()> {
    let variant_dir = TEMPLATES_DIR
        .get_dir(variant)
        .ok_or_else(|| anyhow!("template variant not found: {variant}"))?;

    apply_dir_recursive(variant_dir, target, &PathBuf::new(), ctx)
}

fn apply_dir_recursive(
    dir: &Dir<'_>,
    target_root: &Path,
    relative: &Path,
    ctx: &OverlayRenderContext<'_>,
) -> DynResult<()> {
    for file in dir.files() {
        let file_name = file
            .path()
            .file_name()
            .ok_or_else(|| anyhow!("invalid template file path: {}", file.path().display()))?;

        let rel_path = relative.join(file_name);
        let output_path = target_root.join(&rel_path);

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let raw = file
            .contents_utf8()
            .ok_or_else(|| anyhow!("template is not valid UTF-8: {}", file.path().display()))?;
        let rendered = render_template_text(raw, ctx)?;

        write_text(&output_path, &rendered)?;
    }

    for child in dir.dirs() {
        let dir_name = child
            .path()
            .file_name()
            .ok_or_else(|| anyhow!("invalid template dir path: {}", child.path().display()))?;
        let child_relative = relative.join(dir_name);
        apply_dir_recursive(child, target_root, &child_relative, ctx)?;
    }

    Ok(())
}

fn render_template_text(raw: &str, ctx: &OverlayRenderContext<'_>) -> DynResult<String> {
    let rendered = raw
        .replace("{{crate_name}}", ctx.crate_name)
        .replace("{{lssa_pin}}", ctx.lssa_pin);

    if let Some(token) = find_unresolved_placeholder(&rendered) {
        bail!("unresolved template token `{token}`");
    }

    Ok(rendered)
}

fn find_unresolved_placeholder(text: &str) -> Option<&str> {
    let start = text.find("{{")?;
    let after_open = &text[start + 2..];

    if let Some(end_rel) = after_open.find("}}") {
        Some(&text[start..start + 2 + end_rel + 2])
    } else {
        Some(&text[start..])
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{apply_overlay, render_template_text, OverlayRenderContext};

    fn mk_temp_dir(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "logos-scaffold-overlay-{suffix}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("failed to create temporary test directory");
        path
    }

    #[test]
    fn overlay_writes_expected_files() {
        let target = mk_temp_dir("files");
        let ctx = OverlayRenderContext {
            crate_name: "my-app",
            lssa_pin: "abc123",
        };

        apply_overlay(&target, "default", &ctx).expect("failed to apply default overlay");

        let expected = [
            "Cargo.toml",
            "README.md",
            ".gitignore",
            ".env.local",
            ".scaffold/commands.md",
            "src/lib.rs",
            "src/bin/run_hello_world.rs",
            "src/bin/run_hello_world_private.rs",
            "src/bin/run_hello_world_with_authorization.rs",
            "src/bin/run_hello_world_through_tail_call.rs",
            "src/bin/run_hello_world_through_tail_call_private.rs",
            "src/bin/run_hello_world_with_authorization_through_tail_call_with_pda.rs",
            "src/bin/run_hello_world_with_move_function.rs",
        ];

        for path in expected {
            assert!(target.join(path).exists(), "missing expected file: {path}");
        }

        fs::remove_dir_all(&target).expect("failed to cleanup temporary test directory");
    }

    #[test]
    fn overlay_renders_tokens_and_leaves_no_unresolved_placeholders() {
        let target = mk_temp_dir("tokens");
        let ctx = OverlayRenderContext {
            crate_name: "example-name",
            lssa_pin: "deadbeef",
        };

        apply_overlay(&target, "default", &ctx).expect("failed to apply default overlay");

        let cargo = fs::read_to_string(target.join("Cargo.toml"))
            .expect("failed to read generated Cargo.toml");
        assert!(cargo.contains("name = \"example-name\""));
        assert!(cargo.contains("rev = \"deadbeef\""));
        assert!(!cargo.contains("{{"));

        fs::remove_dir_all(&target).expect("failed to cleanup temporary test directory");
    }

    #[test]
    fn static_files_match_template_content_after_overlay() {
        let target = mk_temp_dir("parity");
        let ctx = OverlayRenderContext {
            crate_name: "my-app",
            lssa_pin: "abc123",
        };

        apply_overlay(&target, "default", &ctx).expect("failed to apply default overlay");

        let env_text = fs::read_to_string(target.join(".env.local"))
            .expect("failed to read generated .env.local");
        assert_eq!(env_text, include_str!("../../templates/default/.env.local"));

        let commands_text = fs::read_to_string(target.join(".scaffold/commands.md"))
            .expect("failed to read generated .scaffold/commands.md");
        assert_eq!(
            commands_text,
            include_str!("../../templates/default/.scaffold/commands.md")
        );

        let runner_text = fs::read_to_string(target.join("src/bin/run_hello_world.rs"))
            .expect("failed to read generated run_hello_world.rs");
        assert_eq!(
            runner_text,
            include_str!("../../templates/default/src/bin/run_hello_world.rs")
        );

        fs::remove_dir_all(&target).expect("failed to cleanup temporary test directory");
    }

    #[test]
    fn gitignore_includes_scaffold_and_is_idempotent() {
        let target = mk_temp_dir("gitignore");
        let ctx = OverlayRenderContext {
            crate_name: "my-app",
            lssa_pin: "abc123",
        };

        apply_overlay(&target, "default", &ctx).expect("failed to apply default overlay");

        let gitignore = fs::read_to_string(target.join(".gitignore"))
            .expect("failed to read generated .gitignore");
        assert!(
            gitignore.lines().any(|l| l.trim() == ".scaffold"),
            ".gitignore should contain .scaffold, got: {gitignore:?}"
        );
        assert!(
            gitignore.lines().any(|l| l.trim() == ".env.local"),
            ".gitignore should contain .env.local, got: {gitignore:?}"
        );

        apply_overlay(&target, "default", &ctx).expect("second overlay should succeed");
        let gitignore_after = fs::read_to_string(target.join(".gitignore"))
            .expect("failed to read .gitignore after second overlay");
        let scaffold_count = gitignore_after
            .lines()
            .filter(|l| l.trim() == ".scaffold")
            .count();
        let env_local_count = gitignore_after
            .lines()
            .filter(|l| l.trim() == ".env.local")
            .count();
        assert_eq!(
            scaffold_count, 1,
            "idempotent overlay must not duplicate .scaffold"
        );
        assert_eq!(
            env_local_count, 1,
            "idempotent overlay must not duplicate .env.local"
        );

        fs::remove_dir_all(&target).expect("failed to cleanup temporary test directory");
    }

    #[test]
    fn render_fails_on_unresolved_placeholder() {
        let ctx = OverlayRenderContext {
            crate_name: "my-app",
            lssa_pin: "abc123",
        };

        let err = render_template_text("name = \"{{unknown_token}}\"", &ctx)
            .expect_err("expected unresolved placeholder to fail");
        assert!(
            err.to_string().contains("unresolved template token"),
            "unexpected error: {err}"
        );
    }
}
