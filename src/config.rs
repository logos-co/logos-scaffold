use crate::constants::{DEFAULT_WALLET_BINARY, LSSA_URL};
use crate::model::{Config, RepoRef};
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
            _ => {}
        }
    }

    if version.is_empty() || cache_root.is_empty() {
        return Err("invalid scaffold.toml: missing [scaffold] keys".into());
    }

    if lssa_url.is_empty() {
        lssa_url = LSSA_URL.to_string();
    }

    if lssa_source.is_empty() || lssa_path.is_empty() || lssa_pin.is_empty() {
        return Err("invalid scaffold.toml: missing required repos.lssa keys".into());
    }

    if wallet_binary.is_empty() {
        wallet_binary = DEFAULT_WALLET_BINARY.to_string();
    }
    if wallet_home_dir.is_empty() {
        wallet_home_dir = ".scaffold/wallet".to_string();
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
    })
}

pub(crate) fn serialize_config(cfg: &Config) -> String {
    format!(
        "[scaffold]\nversion = \"{}\"\ncache_root = \"{}\"\n\n[repos.lssa]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n\n[wallet]\nbinary = \"{}\"\nhome_dir = \"{}\"\n",
        escape_toml_string(&cfg.version),
        escape_toml_string(&cfg.cache_root),
        escape_toml_string(&cfg.lssa.url),
        escape_toml_string(&cfg.lssa.source),
        escape_toml_string(&cfg.lssa.path),
        escape_toml_string(&cfg.lssa.pin),
        escape_toml_string(&cfg.wallet_binary),
        escape_toml_string(&cfg.wallet_home_dir),
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
