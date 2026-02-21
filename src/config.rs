use anyhow::bail;

use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    DEFAULT_WALLET_BINARY, FRAMEWORK_KIND_DEFAULT, LSSA_URL,
};
use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, RepoRef};
use crate::DynResult;

pub(crate) fn parse_config(text: &str) -> DynResult<Config> {
    let mut section = String::new();

    let mut version = String::new();
    let mut cache_root = String::new();

    let mut lssa_url = String::new();
    let mut lssa_source = String::new();
    let mut lssa_path = String::new();
    let mut lssa_pin = String::new();

    let mut wallet_binary = String::new();
    let mut wallet_home_dir = String::new();
    let mut framework_kind = String::new();
    let mut framework_version = String::new();
    let mut framework_idl_spec = String::new();
    let mut framework_idl_path = String::new();

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
            "repos.lssa" => {
                if key == "url" {
                    lssa_url = value;
                } else if key == "source" {
                    lssa_source = value;
                } else if key == "path" {
                    lssa_path = value;
                } else if key == "pin" {
                    lssa_pin = value;
                }
            }
            "wallet" => {
                if key == "binary" {
                    wallet_binary = value;
                } else if key == "home_dir" {
                    wallet_home_dir = value;
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
            _ => {}
        }
    }

    if version.is_empty() || cache_root.is_empty() {
        bail!("invalid scaffold.toml: missing [scaffold] keys");
    }

    if lssa_url.is_empty() {
        lssa_url = LSSA_URL.to_string();
    }

    if lssa_source.is_empty() || lssa_path.is_empty() || lssa_pin.is_empty() {
        bail!("invalid scaffold.toml: missing required repos.lssa keys");
    }

    if wallet_binary.is_empty() {
        wallet_binary = DEFAULT_WALLET_BINARY.to_string();
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
        lssa: RepoRef {
            url: lssa_url,
            source: lssa_source,
            path: lssa_path,
            pin: lssa_pin,
        },
        wallet_binary,
        wallet_home_dir,
        framework: FrameworkConfig {
            kind: framework_kind,
            version: framework_version,
            idl: FrameworkIdlConfig {
                spec: framework_idl_spec,
                path: framework_idl_path,
            },
        },
    })
}

pub(crate) fn serialize_config(cfg: &Config) -> String {
    format!(
        "[scaffold]\nversion = \"{}\"\ncache_root = \"{}\"\n\n[repos.lssa]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n\n[wallet]\nbinary = \"{}\"\nhome_dir = \"{}\"\n\n[framework]\nkind = \"{}\"\nversion = \"{}\"\n\n[framework.idl]\nspec = \"{}\"\npath = \"{}\"\n",
        escape_toml_string(&cfg.version),
        escape_toml_string(&cfg.cache_root),
        escape_toml_string(&cfg.lssa.url),
        escape_toml_string(&cfg.lssa.source),
        escape_toml_string(&cfg.lssa.path),
        escape_toml_string(&cfg.lssa.pin),
        escape_toml_string(&cfg.wallet_binary),
        escape_toml_string(&cfg.wallet_home_dir),
        escape_toml_string(&cfg.framework.kind),
        escape_toml_string(&cfg.framework.version),
        escape_toml_string(&cfg.framework.idl.spec),
        escape_toml_string(&cfg.framework.idl.path),
    )
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
    use crate::constants::{
        DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
        FRAMEWORK_KIND_DEFAULT,
    };
    use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, RepoRef};

    #[test]
    fn parse_old_config_defaults_framework_section() {
        let text = r#"
[scaffold]
version = "0.1.0"
cache_root = "/tmp/cache"

[repos.lssa]
url = "https://github.com/logos-blockchain/lssa.git"
source = "https://github.com/logos-blockchain/lssa.git"
path = "/tmp/lssa"
pin = "abc"

[wallet]
binary = "wallet"
home_dir = ".scaffold/wallet"
"#;

        let cfg = parse_config(text).expect("config should parse");
        assert_eq!(cfg.framework.kind, FRAMEWORK_KIND_DEFAULT);
        assert_eq!(cfg.framework.version, DEFAULT_FRAMEWORK_VERSION);
        assert_eq!(cfg.framework.idl.spec, DEFAULT_FRAMEWORK_IDL_SPEC);
        assert_eq!(cfg.framework.idl.path, DEFAULT_FRAMEWORK_IDL_PATH);
    }

    #[test]
    fn roundtrip_framework_fields() {
        let cfg = Config {
            version: "0.1.0".to_string(),
            cache_root: "/tmp/cache".to_string(),
            lssa: RepoRef {
                url: "url".to_string(),
                source: "source".to_string(),
                path: "path".to_string(),
                pin: "pin".to_string(),
            },
            wallet_binary: "wallet".to_string(),
            wallet_home_dir: ".scaffold/wallet".to_string(),
            framework: FrameworkConfig {
                kind: "lssa-lang".to_string(),
                version: "0.1.0".to_string(),
                idl: FrameworkIdlConfig {
                    spec: "lssa-idl/0.1.0".to_string(),
                    path: "idl".to_string(),
                },
            },
        };

        let text = serialize_config(&cfg);
        let parsed = parse_config(&text).expect("roundtrip parse should work");
        assert_eq!(parsed.framework.kind, "lssa-lang");
        assert_eq!(parsed.framework.idl.path, "idl");
        assert_eq!(parsed.framework.idl.spec, "lssa-idl/0.1.0");
    }
}
