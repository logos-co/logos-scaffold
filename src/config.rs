use anyhow::bail;

use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    FRAMEWORK_KIND_DEFAULT, LEZ_URL,
};
use crate::model::{
    BasecampConfig, Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, ModuleEntry,
    ModuleRole, RepoRef, RunConfig,
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
    let mut run_reset_localnet: Option<bool> = None;
    let mut run_post_deploy: Vec<String> = Vec::new();

    let mut basecamp_seen = false;
    let mut basecamp_pin = String::new();
    let mut basecamp_source = String::new();
    let mut basecamp_lgpm_flake = String::new();
    let mut basecamp_port_base: u16 = 60000;
    let mut basecamp_port_stride: u16 = 10;
    // Keyed by module_name. Values are partial — we fill in fields as we see
    // them in `[basecamp.modules.<name>]` sub-sections, then validate below.
    let mut basecamp_modules_partial: std::collections::BTreeMap<
        String,
        (Option<String>, Option<String>),
    > = std::collections::BTreeMap::new();

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
            "basecamp" => {
                basecamp_seen = true;
                if key == "pin" {
                    basecamp_pin = value;
                } else if key == "source" {
                    basecamp_source = value;
                } else if key == "lgpm_flake" {
                    basecamp_lgpm_flake = value;
                } else if key == "port_base" {
                    basecamp_port_base = value.parse::<u16>().map_err(|e| {
                        anyhow::anyhow!(
                            "invalid scaffold.toml: [basecamp].port_base = {value:?}: {e}"
                        )
                    })?;
                } else if key == "port_stride" {
                    basecamp_port_stride = value.parse::<u16>().map_err(|e| {
                        anyhow::anyhow!(
                            "invalid scaffold.toml: [basecamp].port_stride = {value:?}: {e}"
                        )
                    })?;
                }
            }
            s if s.starts_with("basecamp.modules.") => {
                basecamp_seen = true;
                let name = s.trim_start_matches("basecamp.modules.").to_string();
                if name.is_empty() {
                    continue;
                }
                let entry = basecamp_modules_partial.entry(name).or_default();
                if key == "flake" {
                    entry.0 = Some(value);
                } else if key == "role" {
                    entry.1 = Some(value);
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
                } else if key == "reset_localnet" {
                    run_reset_localnet = Some(value != "false" && value != "0");
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

    if version.is_empty() {
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

    let mut basecamp_modules: std::collections::BTreeMap<String, ModuleEntry> =
        std::collections::BTreeMap::new();
    for (name, (flake, role)) in basecamp_modules_partial {
        let flake = flake.ok_or_else(|| {
            anyhow::anyhow!(
                "invalid scaffold.toml: [basecamp.modules.{name}] missing required field `flake`"
            )
        })?;
        let role_str = role.unwrap_or_default();
        let role = match role_str.as_str() {
            "project" => ModuleRole::Project,
            "dependency" => ModuleRole::Dependency,
            other => bail!(
                "invalid scaffold.toml: [basecamp.modules.{name}] `role` = {other:?}; expected `project` or `dependency`"
            ),
        };
        basecamp_modules.insert(name, ModuleEntry { flake, role });
    }

    let basecamp = if basecamp_seen {
        Some(BasecampConfig {
            pin: basecamp_pin,
            source: basecamp_source,
            lgpm_flake: basecamp_lgpm_flake,
            port_base: basecamp_port_base,
            port_stride: basecamp_port_stride,
            modules: basecamp_modules,
        })
    } else {
        None
    };

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
            reset_localnet: run_reset_localnet.unwrap_or(false),
            post_deploy: run_post_deploy,
        },
        basecamp,
    })
}

pub(crate) fn serialize_config(cfg: &Config) -> DynResult<String> {
    check_toml_value("version", &cfg.version)?;
    check_toml_value("cache_root", &cfg.cache_root)?;
    check_toml_value("repos.lez.url", &cfg.lez.url)?;
    check_toml_value("repos.lez.source", &cfg.lez.source)?;
    check_toml_value("repos.lez.path", &cfg.lez.path)?;
    check_toml_value("repos.lez.pin", &cfg.lez.pin)?;
    check_toml_value("wallet.home_dir", &cfg.wallet_home_dir)?;
    check_toml_value("framework.kind", &cfg.framework.kind)?;
    check_toml_value("framework.version", &cfg.framework.version)?;
    check_toml_value("framework.idl.spec", &cfg.framework.idl.spec)?;
    check_toml_value("framework.idl.path", &cfg.framework.idl.path)?;

    let cache_root_line = if cfg.cache_root.is_empty() {
        // Documentation block for the default (unset) case. Keeping it in
        // scaffold.toml means devs discover the override without reading docs.
        "# cache_root: directory for scaffold's build/repo caches.\n\
         # Resolution order when resolving at runtime:\n\
         #   1. LOGOS_SCAFFOLD_CACHE_ROOT env var\n\
         #   2. cache_root below (uncomment to pin)\n\
         #   3. $XDG_CACHE_HOME/logos-scaffold\n\
         #   4. $HOME/.cache/logos-scaffold\n\
         # Relative values resolve against this file's directory.\n\
         # cache_root = \".scaffold/cache\"\n"
            .to_string()
    } else {
        format!("cache_root = \"{}\"\n", escape_toml_string(&cfg.cache_root))
    };
    let mut out = format!(
        "[scaffold]\nversion = \"{}\"\n{}\n[repos.lez]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n\n[wallet]\nhome_dir = \"{}\"\n\n[framework]\nkind = \"{}\"\nversion = \"{}\"\n\n[framework.idl]\nspec = \"{}\"\npath = \"{}\"\n\n[localnet]\nport = {}\nrisc0_dev_mode = {}\n",
        escape_toml_string(&cfg.version),
        cache_root_line,
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

    if cfg.run.restart_localnet || cfg.run.reset_localnet || !cfg.run.post_deploy.is_empty() {
        for (i, hook) in cfg.run.post_deploy.iter().enumerate() {
            check_toml_value(&format!("run.post_deploy[{i}]"), hook)?;
        }
        out.push_str("\n[run]\n");
        if cfg.run.restart_localnet {
            out.push_str(&format!(
                "restart_localnet = {}\n",
                cfg.run.restart_localnet
            ));
        }
        if cfg.run.reset_localnet {
            out.push_str(&format!("reset_localnet = {}\n", cfg.run.reset_localnet));
        }
        if !cfg.run.post_deploy.is_empty() {
            let quoted: Vec<String> = cfg
                .run
                .post_deploy
                .iter()
                .map(|h| format!("\"{}\"", escape_toml_string(h)))
                .collect();
            out.push_str(&format!("post_deploy = [{}]\n", quoted.join(", ")));
        }
    }

    if let Some(bc) = &cfg.basecamp {
        check_toml_value("basecamp.pin", &bc.pin)?;
        check_toml_value("basecamp.source", &bc.source)?;
        check_toml_value("basecamp.lgpm_flake", &bc.lgpm_flake)?;
        out.push_str(&format!(
            "\n[basecamp]\npin = \"{}\"\nsource = \"{}\"\nlgpm_flake = \"{}\"\nport_base = {}\nport_stride = {}\n",
            escape_toml_string(&bc.pin),
            escape_toml_string(&bc.source),
            escape_toml_string(&bc.lgpm_flake),
            bc.port_base,
            bc.port_stride,
        ));
        for (name, entry) in &bc.modules {
            check_toml_value(&format!("basecamp.modules.{name}"), name)?;
            check_toml_value(&format!("basecamp.modules.{name}.flake"), &entry.flake)?;
            let role_str = match entry.role {
                ModuleRole::Project => "project",
                ModuleRole::Dependency => "dependency",
            };
            out.push_str(&format!(
                "\n[basecamp.modules.{}]\nflake = \"{}\"\nrole = \"{}\"\n",
                escape_toml_string(name),
                escape_toml_string(&entry.flake),
                role_str,
            ));
        }
    }

    Ok(out)
}

/// Reject any value containing a newline, CR, tab, or other C0 control
/// character — the line-oriented parser in `parse_config` treats newlines as
/// record separators, so an embedded one would forge a new key/section. Used
/// as defense-in-depth alongside `normalize_and_validate_module_name`; the
/// only remaining attacker surface after module-name validation is
/// `--flake`-derived or config-sourced values.
fn check_toml_value(key: &str, value: &str) -> DynResult<()> {
    if let Some(bad) = value
        .chars()
        .find(|c| *c == '\n' || *c == '\r' || *c == '\t' || (*c as u32) < 0x20)
    {
        bail!(
            "scaffold.toml `{key}` contains control character {:?} which would \
             corrupt the line-oriented serializer: {value:?}",
            bad
        );
    }
    Ok(())
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
    use super::*;

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
        let serialized = serialize_config(&cfg).expect("serialize");
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
        let serialized = serialize_config(&cfg).expect("serialize");
        assert!(serialized.contains("[run]"));
        assert!(serialized.contains("restart_localnet = true"));
        assert!(serialized.contains("spel --idl foo.json"));
    }

    #[test]
    fn parse_config_with_reset_localnet_true() {
        let toml = minimal_scaffold_toml() + "[run]\nreset_localnet = true\n";
        let cfg = parse_config(&toml).expect("parse");
        assert!(cfg.run.reset_localnet);
    }

    #[test]
    fn parse_config_reset_localnet_defaults_to_false() {
        let cfg = parse_config(&minimal_scaffold_toml()).expect("parse");
        assert!(!cfg.run.reset_localnet);
    }

    #[test]
    fn serialize_config_emits_reset_localnet_when_set() {
        let toml = minimal_scaffold_toml() + "[run]\nreset_localnet = true\n";
        let cfg = parse_config(&toml).expect("parse");
        let out = serialize_config(&cfg).expect("serialize");
        assert!(out.contains("reset_localnet = true"), "got: {out}");
    }

    #[test]
    fn serialize_config_omits_reset_localnet_when_default() {
        let cfg = parse_config(&minimal_scaffold_toml()).expect("parse");
        let out = serialize_config(&cfg).expect("serialize");
        assert!(
            !out.contains("reset_localnet"),
            "should omit reset_localnet at default, got: {out}"
        );
    }

    #[test]
    fn reset_localnet_round_trips_through_parse_serialize() {
        let toml = minimal_scaffold_toml()
            + "[run]\nrestart_localnet = false\nreset_localnet = true\npost_deploy = [\"echo a\"]\n";
        let cfg1 = parse_config(&toml).expect("parse");
        let serialized = serialize_config(&cfg1).expect("serialize");
        let cfg2 = parse_config(&serialized).expect("re-parse");
        assert_eq!(cfg1.run.reset_localnet, cfg2.run.reset_localnet);
        assert!(cfg2.run.reset_localnet);
    }

    #[test]
    fn run_config_round_trips_through_parse_serialize() {
        let toml = minimal_scaffold_toml()
            + "[run]\nrestart_localnet = true\npost_deploy = [\"echo a\", \"echo b\"]\n";
        let cfg1 = parse_config(&toml).expect("parse");
        let serialized = serialize_config(&cfg1).expect("serialize");
        let cfg2 = parse_config(&serialized).expect("re-parse");
        assert_eq!(cfg1.run.restart_localnet, cfg2.run.restart_localnet);
        assert_eq!(cfg1.run.post_deploy, cfg2.run.post_deploy);
        assert_eq!(cfg2.run.post_deploy.len(), 2);
    }

    fn base_config() -> Config {
        Config {
            version: "0.1.0".to_string(),
            cache_root: "cache".to_string(),
            lez: RepoRef {
                url: LEZ_URL.to_string(),
                source: LEZ_URL.to_string(),
                path: "lez".to_string(),
                pin: "abc123".to_string(),
            },
            wallet_home_dir: ".scaffold/wallet".to_string(),
            localnet: LocalnetConfig::default(),
            framework: FrameworkConfig {
                kind: FRAMEWORK_KIND_DEFAULT.to_string(),
                version: DEFAULT_FRAMEWORK_VERSION.to_string(),
                idl: FrameworkIdlConfig {
                    spec: DEFAULT_FRAMEWORK_IDL_SPEC.to_string(),
                    path: DEFAULT_FRAMEWORK_IDL_PATH.to_string(),
                },
            },
            run: RunConfig::default(),
            basecamp: None,
        }
    }

    #[test]
    fn basecamp_absent_roundtrips_as_none() {
        let cfg = base_config();
        let serialized = serialize_config(&cfg).expect("serialize");
        assert!(
            !serialized.contains("[basecamp]"),
            "basecamp section should be omitted when None, got:\n{serialized}"
        );

        let parsed = parse_config(&serialized).expect("parse");
        assert!(parsed.basecamp.is_none());
    }

    #[test]
    fn basecamp_section_roundtrips_preserving_fields() {
        let mut cfg = base_config();
        cfg.basecamp = Some(BasecampConfig {
            pin: "deadbeef".to_string(),
            source: "https://github.com/logos-co/logos-basecamp".to_string(),
            lgpm_flake: "github:logos-co/lgpm#lgpm".to_string(),
            port_base: 61000,
            port_stride: 20,
            modules: std::collections::BTreeMap::new(),
        });

        let serialized = serialize_config(&cfg).expect("serialize");
        assert!(serialized.contains("[basecamp]"));

        let parsed = parse_config(&serialized).expect("parse");
        let bc = parsed.basecamp.expect("basecamp present");
        assert_eq!(bc.pin, "deadbeef");
        assert_eq!(bc.source, "https://github.com/logos-co/logos-basecamp");
        assert_eq!(bc.lgpm_flake, "github:logos-co/lgpm#lgpm");
        assert_eq!(bc.port_base, 61000);
        assert_eq!(bc.port_stride, 20);
        assert!(bc.modules.is_empty());
    }

    #[test]
    fn basecamp_modules_empty_map_omits_section() {
        let mut cfg = base_config();
        cfg.basecamp = Some(BasecampConfig {
            pin: "deadbeef".to_string(),
            source: "https://example/basecamp".to_string(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            modules: std::collections::BTreeMap::new(),
        });

        let serialized = serialize_config(&cfg).expect("serialize");
        assert!(
            !serialized.contains("[basecamp.modules"),
            "empty modules map should omit sub-sections, got:\n{serialized}"
        );
    }

    #[test]
    fn basecamp_modules_subsection_roundtrips_preserving_entries() {
        let mut cfg = base_config();
        let mut modules = std::collections::BTreeMap::new();
        modules.insert(
            "tictactoe".to_string(),
            ModuleEntry {
                flake: "path:/abs/tictactoe#lgx".to_string(),
                role: ModuleRole::Project,
            },
        );
        modules.insert(
            "delivery_module".to_string(),
            ModuleEntry {
                flake: "github:logos-co/logos-delivery-module/1fde1566#lgx".to_string(),
                role: ModuleRole::Dependency,
            },
        );
        cfg.basecamp = Some(BasecampConfig {
            pin: "deadbeef".to_string(),
            source: "https://example/basecamp".to_string(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            modules: modules.clone(),
        });

        let serialized = serialize_config(&cfg).expect("serialize");
        assert!(
            serialized.contains("[basecamp.modules.tictactoe]"),
            "expected [basecamp.modules.tictactoe] in:\n{serialized}"
        );
        assert!(
            serialized.contains("[basecamp.modules.delivery_module]"),
            "expected [basecamp.modules.delivery_module] in:\n{serialized}"
        );

        let parsed = parse_config(&serialized).expect("parse");
        let bc = parsed.basecamp.expect("basecamp present");
        assert_eq!(bc.modules, modules);
    }

    #[test]
    fn basecamp_modules_alone_implies_basecamp_seen() {
        let text = r#"[scaffold]
version = "0.1.0"
cache_root = "cache"

[repos.lez]
url = "u"
source = "s"
path = "p"
pin = "q"

[basecamp.modules.tictactoe]
flake = "path:/abs/tictactoe#lgx"
role = "project"
"#;
        let parsed = parse_config(text).expect("parse");
        let bc = parsed.basecamp.expect("basecamp synthesized");
        let entry = bc.modules.get("tictactoe").expect("tictactoe captured");
        assert_eq!(entry.flake, "path:/abs/tictactoe#lgx");
        assert_eq!(entry.role, ModuleRole::Project);
    }

    #[test]
    fn basecamp_modules_rejects_unknown_role() {
        let text = r#"[scaffold]
version = "0.1.0"
cache_root = "cache"

[repos.lez]
url = "u"
source = "s"
path = "p"
pin = "q"

[basecamp.modules.tictactoe]
flake = "path:/abs/tictactoe#lgx"
role = "weird"
"#;
        let err = parse_config(text).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("weird") && msg.contains("role"),
            "expected role/weird in error, got: {msg}"
        );
    }

    #[test]
    fn basecamp_modules_rejects_missing_flake() {
        let text = r#"[scaffold]
version = "0.1.0"
cache_root = "cache"

[repos.lez]
url = "u"
source = "s"
path = "p"
pin = "q"

[basecamp.modules.tictactoe]
role = "project"
"#;
        let err = parse_config(text).unwrap_err();
        assert!(
            err.to_string().contains("flake"),
            "expected flake-missing error, got: {err}"
        );
    }

    #[test]
    fn serialize_rejects_newline_in_bc_source() {
        let mut cfg = base_config();
        cfg.basecamp = Some(BasecampConfig {
            pin: "abc".to_string(),
            source: "https://example\n[basecamp.modules.evil]\nflake = \"evil\"".to_string(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            modules: std::collections::BTreeMap::new(),
        });
        let err = serialize_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("newline") || msg.contains("control") || msg.contains("\\n"),
            "expected control-char error, got: {msg}"
        );
    }

    #[test]
    fn serialize_rejects_carriage_return_in_pin() {
        let mut cfg = base_config();
        cfg.basecamp = Some(BasecampConfig {
            pin: "a\rbad".to_string(),
            source: String::new(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            modules: std::collections::BTreeMap::new(),
        });
        let err = serialize_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("pin"), "{err}");
    }

    #[test]
    fn serialize_rejects_newline_in_module_entry_flake() {
        let mut cfg = base_config();
        let mut modules = std::collections::BTreeMap::new();
        modules.insert(
            "legit".to_string(),
            ModuleEntry {
                flake: "path:/p#lgx\n[basecamp.modules.attacker]\nflake = evil".to_string(),
                role: ModuleRole::Project,
            },
        );
        cfg.basecamp = Some(BasecampConfig {
            pin: "abc".to_string(),
            source: String::new(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            modules,
        });
        let err = serialize_config(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("flake") && err.to_string().contains("legit"),
            "{err}"
        );
    }

    #[test]
    fn serialize_rejects_tab_in_lez_url() {
        let mut cfg = base_config();
        cfg.lez.url = "https://example\tevil".to_string();
        let err = serialize_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("url"), "{err}");
    }

    #[test]
    fn basecamp_section_with_only_pin_applies_defaults() {
        let text = "[scaffold]\nversion = \"0.1.0\"\ncache_root = \"cache\"\n\n[repos.lez]\nurl = \"u\"\nsource = \"s\"\npath = \"p\"\npin = \"q\"\n\n[basecamp]\npin = \"sha1\"\n";
        let parsed = parse_config(text).expect("parse");
        let bc = parsed.basecamp.expect("basecamp present");
        assert_eq!(bc.pin, "sha1");
        assert_eq!(bc.port_base, 60000);
        assert_eq!(bc.port_stride, 10);
    }
}
