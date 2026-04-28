use anyhow::bail;

use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    FRAMEWORK_KIND_DEFAULT, LEZ_URL,
};
use crate::model::{
    Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, RepoRef, RunConfig,
};
use crate::DynResult;

/// Joins multi-line TOML inline arrays onto a single line so the
/// line-oriented parser below can read them as one logical entry.
fn fold_multiline_arrays(text: &str) -> String {
    let mut out = String::new();
    let mut accumulator = String::new();
    let mut accumulating = false;

    for raw in text.lines() {
        if accumulating {
            accumulator.push(' ');
            accumulator.push_str(raw.trim());
            if raw.contains(']') {
                out.push_str(&accumulator);
                out.push('\n');
                accumulator.clear();
                accumulating = false;
            }
            continue;
        }

        let trimmed = raw.trim_start();
        // Detect "<key> = [" where the closing `]` is not on the same line.
        // Section headers like `[run]` already balance on a single line.
        if let Some(eq_idx) = trimmed.find('=') {
            let after_eq = trimmed[eq_idx + 1..].trim_start();
            if after_eq.starts_with('[') && !raw.contains(']') {
                accumulator.clear();
                accumulator.push_str(raw);
                accumulating = true;
                continue;
            }
        }

        out.push_str(raw);
        out.push('\n');
    }

    if accumulating {
        // Unterminated array — fall through; downstream parser will report.
        out.push_str(&accumulator);
        out.push('\n');
    }

    out
}

pub(crate) fn parse_config(text: &str) -> DynResult<Config> {
    let folded = fold_multiline_arrays(text);
    let text = folded.as_str();
    let mut section = String::new();

    let mut version = String::new();
    let mut cache_root = String::new();

    let mut lez_url = String::new();
    let mut lez_source = String::new();
    let mut lez_path = String::new();
    let mut lez_pin = String::new();

    let mut wallet_home_dir = String::new();

    let mut localnet_port: u16 = 3040;
    let mut localnet_risc0_dev_mode: bool = true;

    let mut framework_kind = String::new();
    let mut framework_version = String::new();
    let mut framework_idl_spec = String::new();
    let mut framework_idl_path = String::new();

    let mut run_restart_localnet: Option<bool> = None;
    let mut run_post_deploy: Vec<String> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            continue;
        }

        let mut parts = line.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let raw_value = parts.next().unwrap_or("").trim().to_string();
        let value = unquote(&raw_value);

        match section.as_str() {
            "scaffold" => {
                if key == "version" {
                    version = value;
                } else if key == "cache_root" {
                    cache_root = value;
                }
            }
            "repos.lez" | "repos.lssa" => {
                if key == "url" {
                    lez_url = value;
                } else if key == "source" {
                    lez_source = value;
                } else if key == "path" {
                    lez_path = value;
                } else if key == "pin" {
                    lez_pin = value;
                }
            }
            "framework" => {
                if key == "kind" {
                    framework_kind = value;
                } else if key == "version" {
                    framework_version = value;
                }
            }
            "framework.idl" => {
                if key == "spec" {
                    framework_idl_spec = value;
                } else if key == "path" {
                    framework_idl_path = value;
                }
            }
            "wallet" => {
                if key == "home_dir" {
                    wallet_home_dir = value;
                }
            }
            "localnet" => {
                if key == "port" {
                    if !value.is_empty() {
                        localnet_port = match value.parse::<u16>() {
                            Ok(p) => p,
                            Err(_) => bail!(
                                "invalid scaffold.toml: [localnet] port `{value}` is not a valid u16 (expected 0-65535)"
                            ),
                        };
                    }
                } else if key == "risc0_dev_mode" {
                    localnet_risc0_dev_mode = value != "false" && value != "0";
                }
            }
            "run" => {
                if key == "restart_localnet" {
                    run_restart_localnet = Some(value != "false" && value != "0");
                } else if key == "post_deploy" {
                    if raw_value.starts_with('[') {
                        run_post_deploy = parse_inline_string_array(&raw_value)?;
                    } else if !value.is_empty() {
                        run_post_deploy = vec![value];
                    }
                }
            }
            _ => {}
        }
    }

    if version.is_empty() || cache_root.is_empty() {
        bail!("invalid scaffold.toml: missing [scaffold] keys");
    }

    if lez_url.is_empty() {
        lez_url = LEZ_URL.to_string();
    }

    if lez_source.is_empty() || lez_path.is_empty() || lez_pin.is_empty() {
        bail!("invalid scaffold.toml: missing required repos.lez keys (also accepts legacy repos.lssa)");
    }

    if wallet_home_dir.is_empty() {
        wallet_home_dir = ".scaffold/wallet".to_string();
    }

    if framework_kind.is_empty() {
        framework_kind = FRAMEWORK_KIND_DEFAULT.to_string();
    }
    if framework_version.is_empty() {
        framework_version = DEFAULT_FRAMEWORK_VERSION.to_string();
    }
    if framework_idl_spec.is_empty() {
        framework_idl_spec = DEFAULT_FRAMEWORK_IDL_SPEC.to_string();
    }
    if framework_idl_path.is_empty() {
        framework_idl_path = DEFAULT_FRAMEWORK_IDL_PATH.to_string();
    }

    Ok(Config {
        version,
        cache_root,
        lez: RepoRef {
            url: lez_url,
            source: lez_source,
            path: lez_path,
            pin: lez_pin,
        },
        wallet_home_dir,
        localnet: LocalnetConfig {
            port: localnet_port,
            risc0_dev_mode: localnet_risc0_dev_mode,
        },
        framework: FrameworkConfig {
            kind: framework_kind,
            version: framework_version,
            idl: FrameworkIdlConfig {
                spec: framework_idl_spec,
                path: framework_idl_path,
            },
        },
        run: RunConfig {
            restart_localnet: run_restart_localnet.unwrap_or(false),
            post_deploy: run_post_deploy,
        },
    })
}

pub(crate) fn serialize_config(cfg: &Config) -> String {
    let mut out = format!(
        "[scaffold]\nversion = \"{}\"\ncache_root = \"{}\"\n\n[repos.lez]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n\n[wallet]\nhome_dir = \"{}\"\n\n[framework]\nkind = \"{}\"\nversion = \"{}\"\n\n[framework.idl]\nspec = \"{}\"\npath = \"{}\"\n\n[localnet]\nport = {}\nrisc0_dev_mode = {}\n",
        escape_toml_string(&cfg.version),
        escape_toml_string(&cfg.cache_root),
        escape_toml_string(&cfg.lez.url),
        escape_toml_string(&cfg.lez.source),
        escape_toml_string(&cfg.lez.path),
        escape_toml_string(&cfg.lez.pin),
        escape_toml_string(&cfg.wallet_home_dir),
        escape_toml_string(&cfg.framework.kind),
        escape_toml_string(&cfg.framework.version),
        escape_toml_string(&cfg.framework.idl.spec),
        escape_toml_string(&cfg.framework.idl.path),
        cfg.localnet.port,
        cfg.localnet.risc0_dev_mode,
    );

    if cfg.run.restart_localnet || !cfg.run.post_deploy.is_empty() {
        out.push_str(&format!(
            "\n[run]\nrestart_localnet = {}\n",
            cfg.run.restart_localnet,
        ));
        let quoted: Vec<String> = cfg
            .run
            .post_deploy
            .iter()
            .map(|h| format!("\"{}\"", escape_toml_string(h)))
            .collect();
        out.push_str(&format!("post_deploy = [{}]\n", quoted.join(", ")));
    }

    out
}

fn parse_inline_string_array(raw: &str) -> DynResult<Vec<String>> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| anyhow::anyhow!("malformed array (expected `[\"...\", ...]`): {trimmed}"))?;

    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut item_open = false;
    let mut chars = inner.chars();

    while let Some(c) = chars.next() {
        if in_string {
            match c {
                '\\' => match chars.next() {
                    Some('n') => current.push('\n'),
                    Some('t') => current.push('\t'),
                    Some('\\') => current.push('\\'),
                    Some('"') => current.push('"'),
                    Some(other) => current.push(other),
                    None => bail!("trailing backslash in array string: {trimmed}"),
                },
                '"' => {
                    items.push(std::mem::take(&mut current));
                    in_string = false;
                    item_open = false;
                }
                _ => current.push(c),
            }
        } else {
            match c {
                '"' => {
                    if item_open {
                        bail!("expected `,` between array items: {trimmed}");
                    }
                    in_string = true;
                    item_open = true;
                }
                ',' => {
                    item_open = false;
                }
                ' ' | '\t' | '\n' | '\r' => {}
                _ => bail!("unexpected character `{c}` in array: {trimmed}"),
            }
        }
    }

    if in_string {
        bail!("unterminated string in array: {trimmed}");
    }

    Ok(items)
}

pub(crate) fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{parse_config, serialize_config};

    fn minimal_scaffold_toml() -> String {
        r#"[scaffold]
version = "0.1.0"
cache_root = "/tmp/cache"

[repos.lssa]
url = "https://example.com/lssa.git"
source = "https://example.com/lssa.git"
path = "/tmp/lssa"
pin = "deadbeef"

"#
        .to_string()
    }

    #[test]
    fn rejects_invalid_localnet_port() {
        let toml = minimal_scaffold_toml() + "[localnet]\nport = not_a_port\n";
        let err = parse_config(&toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not_a_port") && msg.contains("port"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn rejects_localnet_port_out_of_range() {
        let toml = minimal_scaffold_toml() + "[localnet]\nport = 70000\n";
        let err = parse_config(&toml).unwrap_err();
        assert!(err.to_string().contains("70000"), "{err}");
    }

    #[test]
    fn parses_valid_custom_localnet_port() {
        let toml = minimal_scaffold_toml() + "[localnet]\nport = 3050\n";
        let cfg = parse_config(&toml).unwrap();
        assert_eq!(cfg.localnet.port, 3050);
    }

    #[test]
    fn default_localnet_port_when_section_omitted() {
        let cfg = parse_config(&minimal_scaffold_toml()).unwrap();
        assert_eq!(cfg.localnet.port, 3040);
    }

    #[test]
    fn parse_config_without_run_section_uses_defaults() {
        let cfg = parse_config(&minimal_scaffold_toml()).expect("parse");
        assert!(!cfg.run.restart_localnet);
        assert!(cfg.run.post_deploy.is_empty());
    }

    #[test]
    fn parse_config_with_run_section_legacy_string() {
        let toml = minimal_scaffold_toml()
            + "[run]\nrestart_localnet = true\npost_deploy = \"echo hello\"\n";
        let cfg = parse_config(&toml).expect("parse");
        assert!(cfg.run.restart_localnet);
        assert_eq!(cfg.run.post_deploy, vec!["echo hello".to_string()]);
    }

    #[test]
    fn parse_config_with_run_section_array() {
        let toml = minimal_scaffold_toml()
            + "[run]\npost_deploy = [\"echo one\", \"echo two\", \"echo three\"]\n";
        let cfg = parse_config(&toml).expect("parse");
        assert_eq!(
            cfg.run.post_deploy,
            vec![
                "echo one".to_string(),
                "echo two".to_string(),
                "echo three".to_string(),
            ]
        );
    }

    #[test]
    fn parse_config_with_multiline_array() {
        let toml =
            minimal_scaffold_toml() + "[run]\npost_deploy = [\n  \"echo a\",\n  \"echo b\",\n]\n";
        let cfg = parse_config(&toml).expect("parse");
        assert_eq!(
            cfg.run.post_deploy,
            vec!["echo a".to_string(), "echo b".to_string()]
        );
    }

    #[test]
    fn parse_config_run_section_empty_array() {
        let toml = minimal_scaffold_toml() + "[run]\npost_deploy = []\n";
        let cfg = parse_config(&toml).expect("parse");
        assert!(cfg.run.post_deploy.is_empty());
    }

    #[test]
    fn parse_config_rejects_malformed_array() {
        let toml = minimal_scaffold_toml() + "[run]\npost_deploy = [\"unterminated\n";
        assert!(parse_config(&toml).is_err());
    }

    #[test]
    fn serialize_config_omits_run_section_when_defaults() {
        let cfg = parse_config(&minimal_scaffold_toml()).expect("parse");
        let serialized = serialize_config(&cfg);
        assert!(
            !serialized.contains("[run]"),
            "should not contain [run] section when defaults"
        );
    }

    #[test]
    fn serialize_config_includes_run_section_when_non_default() {
        let toml = minimal_scaffold_toml()
            + "[run]\nrestart_localnet = true\npost_deploy = [\"spel --idl foo.json\"]\n";
        let cfg = parse_config(&toml).expect("parse");
        let serialized = serialize_config(&cfg);
        assert!(serialized.contains("[run]"));
        assert!(serialized.contains("restart_localnet = true"));
        assert!(serialized.contains("spel --idl foo.json"));
    }

    #[test]
    fn run_config_round_trips_through_parse_serialize() {
        let toml = minimal_scaffold_toml()
            + "[run]\nrestart_localnet = true\npost_deploy = [\"echo a\", \"echo b\"]\n";
        let cfg1 = parse_config(&toml).expect("parse");
        let serialized = serialize_config(&cfg1);
        let cfg2 = parse_config(&serialized).expect("re-parse");
        assert_eq!(cfg1.run.restart_localnet, cfg2.run.restart_localnet);
        assert_eq!(cfg1.run.post_deploy, cfg2.run.post_deploy);
        assert_eq!(cfg2.run.post_deploy.len(), 2);
    }
}
