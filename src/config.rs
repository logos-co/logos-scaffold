use anyhow::bail;

use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    FRAMEWORK_KIND_DEFAULT, LEZ_URL,
};
use crate::model::{
    BasecampConfig, Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, ModuleEntry,
    ModuleRole, RepoRef,
};
use crate::DynResult;

pub(crate) fn parse_config(text: &str) -> DynResult<Config> {
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

    let mut basecamp_seen = false;
    let mut basecamp_pin = String::new();
    let mut basecamp_source = String::new();
    let mut basecamp_lgpm_flake = String::new();
    let mut basecamp_port_base: u16 = 60000;
    let mut basecamp_port_stride: u16 = 10;
    let mut basecamp_dependencies: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
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
        let value = unquote(parts.next().unwrap_or("").trim());

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
            "basecamp.dependencies" => {
                basecamp_seen = true;
                if !key.is_empty() {
                    basecamp_dependencies.insert(key.to_string(), value);
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
            dependencies: basecamp_dependencies,
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
        basecamp,
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

    if let Some(bc) = &cfg.basecamp {
        out.push_str(&format!(
            "\n[basecamp]\npin = \"{}\"\nsource = \"{}\"\nlgpm_flake = \"{}\"\nport_base = {}\nport_stride = {}\n",
            escape_toml_string(&bc.pin),
            escape_toml_string(&bc.source),
            escape_toml_string(&bc.lgpm_flake),
            bc.port_base,
            bc.port_stride,
        ));
        if !bc.dependencies.is_empty() {
            out.push_str("\n[basecamp.dependencies]\n");
            for (name, flake_ref) in &bc.dependencies {
                out.push_str(&format!(
                    "{} = \"{}\"\n",
                    escape_toml_string(name),
                    escape_toml_string(flake_ref),
                ));
            }
        }
        for (name, entry) in &bc.modules {
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

    out
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
            basecamp: None,
        }
    }

    #[test]
    fn basecamp_absent_roundtrips_as_none() {
        let cfg = base_config();
        let serialized = serialize_config(&cfg);
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
            dependencies: std::collections::BTreeMap::new(),
            modules: std::collections::BTreeMap::new(),
        });

        let serialized = serialize_config(&cfg);
        assert!(serialized.contains("[basecamp]"));

        let parsed = parse_config(&serialized).expect("parse");
        let bc = parsed.basecamp.expect("basecamp present");
        assert_eq!(bc.pin, "deadbeef");
        assert_eq!(bc.source, "https://github.com/logos-co/logos-basecamp");
        assert_eq!(bc.lgpm_flake, "github:logos-co/lgpm#lgpm");
        assert_eq!(bc.port_base, 61000);
        assert_eq!(bc.port_stride, 20);
        assert!(bc.dependencies.is_empty());
    }

    #[test]
    fn basecamp_dependencies_section_roundtrips() {
        let mut cfg = base_config();
        let mut deps = std::collections::BTreeMap::new();
        deps.insert(
            "delivery_module".to_string(),
            "github:logos-co/logos-delivery-module/1.1.0#lgx".to_string(),
        );
        deps.insert(
            "storage_module".to_string(),
            "github:logos-co/logos-storage-module/abcdef#lgx".to_string(),
        );
        cfg.basecamp = Some(BasecampConfig {
            pin: "deadbeef".to_string(),
            source: "https://example/basecamp".to_string(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            dependencies: deps.clone(),
            modules: std::collections::BTreeMap::new(),
        });

        let serialized = serialize_config(&cfg);
        assert!(
            serialized.contains("[basecamp.dependencies]"),
            "expected [basecamp.dependencies] section in:\n{serialized}"
        );
        assert!(serialized.contains("delivery_module ="));
        assert!(serialized.contains("storage_module ="));

        let parsed = parse_config(&serialized).expect("parse");
        let bc = parsed.basecamp.expect("basecamp present");
        assert_eq!(bc.dependencies, deps);
    }

    #[test]
    fn basecamp_dependencies_section_alone_implies_basecamp_seen() {
        // A project that only sets [basecamp.dependencies] (no [basecamp] pins)
        // should still round-trip with a BasecampConfig containing the deps.
        let text = r#"[scaffold]
version = "0.1.0"
cache_root = "cache"

[repos.lez]
url = "u"
source = "s"
path = "p"
pin = "q"

[basecamp.dependencies]
delivery_module = "github:logos-co/logos-delivery-module/1.0.0#lgx"
"#;
        let parsed = parse_config(text).expect("parse");
        let bc = parsed
            .basecamp
            .expect("basecamp section should be synthesized");
        assert_eq!(
            bc.dependencies.get("delivery_module").map(|s| s.as_str()),
            Some("github:logos-co/logos-delivery-module/1.0.0#lgx")
        );
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
            dependencies: std::collections::BTreeMap::new(),
            modules: std::collections::BTreeMap::new(),
        });

        let serialized = serialize_config(&cfg);
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
            dependencies: std::collections::BTreeMap::new(),
            modules: modules.clone(),
        });

        let serialized = serialize_config(&cfg);
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
    fn basecamp_section_with_only_pin_applies_defaults() {
        let text = "[scaffold]\nversion = \"0.1.0\"\ncache_root = \"cache\"\n\n[repos.lez]\nurl = \"u\"\nsource = \"s\"\npath = \"p\"\npin = \"q\"\n\n[basecamp]\npin = \"sha1\"\n";
        let parsed = parse_config(text).expect("parse");
        let bc = parsed.basecamp.expect("basecamp present");
        assert_eq!(bc.pin, "sha1");
        assert_eq!(bc.port_base, 60000);
        assert_eq!(bc.port_stride, 10);
    }
}
