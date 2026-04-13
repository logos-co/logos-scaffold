use anyhow::bail;

use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    FRAMEWORK_KIND_DEFAULT, LEZ_URL,
};
use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, RepoRef};
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
            "localnet" => {
                if key == "port" {
                    if let Ok(p) = value.parse::<u16>() {
                        localnet_port = p;
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
    })
}

pub(crate) fn serialize_config(cfg: &Config) -> String {
    format!(
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
