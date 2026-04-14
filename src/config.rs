use anyhow::bail;

use crate::constants::{
    BASECAMP_RUNTIME_DEV, DEFAULT_BASECAMP_DATA_ROOT, DEFAULT_FRAMEWORK_IDL_PATH,
    DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION, FRAMEWORK_KIND_DEFAULT, LEZ_URL,
    PROJECT_KIND_LEZ,
};
use crate::model::{
    BasecampConfig, Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, ProjectConfig,
    RepoRef,
};
use crate::DynResult;

pub(crate) fn parse_config(text: &str) -> DynResult<Config> {
    let mut section = String::new();

    let mut version = String::new();
    let mut cache_root = String::new();
    let mut project_kind = String::new();

    let mut lez_url = String::new();
    let mut lez_source = String::new();
    let mut lez_path = String::new();
    let mut lez_pin = String::new();

    let mut module_builder_url = String::new();
    let mut module_builder_source = String::new();
    let mut module_builder_path = String::new();
    let mut module_builder_pin = String::new();

    let mut wallet_home_dir = String::new();

    let mut localnet_port: u16 = 3040;
    let mut localnet_risc0_dev_mode: bool = true;

    let mut framework_kind = String::new();
    let mut framework_version = String::new();
    let mut framework_idl_spec = String::new();
    let mut framework_idl_path = String::new();

    let mut basecamp_data_root = String::new();
    let mut basecamp_runtime_variant = String::new();

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
            "project" => {
                if key == "kind" {
                    project_kind = value;
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
            "repos.logos_module_builder" => {
                if key == "url" {
                    module_builder_url = value;
                } else if key == "source" {
                    module_builder_source = value;
                } else if key == "path" {
                    module_builder_path = value;
                } else if key == "pin" {
                    module_builder_pin = value;
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
            "basecamp" => {
                if key == "data_root" {
                    basecamp_data_root = value;
                } else if key == "runtime_variant" {
                    basecamp_runtime_variant = value;
                }
            }
            _ => {}
        }
    }

    if version.is_empty() || cache_root.is_empty() {
        bail!("invalid scaffold.toml: missing [scaffold] keys");
    }

    if project_kind.is_empty() {
        project_kind = PROJECT_KIND_LEZ.to_string();
    }

    if lez_url.is_empty() {
        lez_url = LEZ_URL.to_string();
    }

    let lez = if lez_source.is_empty() && lez_path.is_empty() && lez_pin.is_empty() {
        None
    } else {
        if lez_source.is_empty() || lez_path.is_empty() || lez_pin.is_empty() {
            bail!("invalid scaffold.toml: missing required repos.lez keys (also accepts legacy repos.lssa)");
        }
        Some(RepoRef {
            url: lez_url,
            source: lez_source,
            path: lez_path,
            pin: lez_pin,
        })
    };

    if project_kind == PROJECT_KIND_LEZ && lez.is_none() {
        bail!("invalid scaffold.toml: missing required repos.lez keys (also accepts legacy repos.lssa)");
    }

    let logos_module_builder =
        if module_builder_source.is_empty() && module_builder_path.is_empty() && module_builder_pin.is_empty() {
            None
        } else {
            if module_builder_source.is_empty()
                || module_builder_path.is_empty()
                || module_builder_pin.is_empty()
            {
                bail!("invalid scaffold.toml: missing required repos.logos_module_builder keys");
            }
            Some(RepoRef {
                url: module_builder_url,
                source: module_builder_source,
                path: module_builder_path,
                pin: module_builder_pin,
            })
        };

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

    if basecamp_data_root.is_empty() {
        basecamp_data_root = DEFAULT_BASECAMP_DATA_ROOT.to_string();
    }
    if basecamp_runtime_variant.is_empty() {
        basecamp_runtime_variant = BASECAMP_RUNTIME_DEV.to_string();
    }

    Ok(Config {
        version,
        cache_root,
        project: ProjectConfig { kind: project_kind },
        lez,
        logos_module_builder,
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
        basecamp: BasecampConfig {
            data_root: basecamp_data_root,
            runtime_variant: basecamp_runtime_variant,
        },
    })
}

pub(crate) fn serialize_config(cfg: &Config) -> String {
    let mut out = format!(
        "[scaffold]\nversion = \"{}\"\ncache_root = \"{}\"\n\n[project]\nkind = \"{}\"\n",
        escape_toml_string(&cfg.version),
        escape_toml_string(&cfg.cache_root),
        escape_toml_string(&cfg.project.kind),
    );

    if let Some(lez) = &cfg.lez {
        out.push_str(&format!(
            "\n[repos.lez]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n",
            escape_toml_string(&lez.url),
            escape_toml_string(&lez.source),
            escape_toml_string(&lez.path),
            escape_toml_string(&lez.pin),
        ));
    }

    if let Some(module_builder) = &cfg.logos_module_builder {
        out.push_str(&format!(
            "\n[repos.logos_module_builder]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n",
            escape_toml_string(&module_builder.url),
            escape_toml_string(&module_builder.source),
            escape_toml_string(&module_builder.path),
            escape_toml_string(&module_builder.pin),
        ));
    }

    out.push_str(&format!(
        "\n[wallet]\nhome_dir = \"{}\"\n\n[framework]\nkind = \"{}\"\nversion = \"{}\"\n\n[framework.idl]\nspec = \"{}\"\npath = \"{}\"\n\n[localnet]\nport = {}\nrisc0_dev_mode = {}\n\n[basecamp]\ndata_root = \"{}\"\nruntime_variant = \"{}\"\n",
        escape_toml_string(&cfg.wallet_home_dir),
        escape_toml_string(&cfg.framework.kind),
        escape_toml_string(&cfg.framework.version),
        escape_toml_string(&cfg.framework.idl.spec),
        escape_toml_string(&cfg.framework.idl.path),
        cfg.localnet.port,
        cfg.localnet.risc0_dev_mode,
        escape_toml_string(&cfg.basecamp.data_root),
        escape_toml_string(&cfg.basecamp.runtime_variant),
    ));

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
    use super::{parse_config, serialize_config};
    use crate::constants::{BASECAMP_RUNTIME_DEV, PROJECT_KIND_BASECAMP_QML, PROJECT_KIND_LEZ};

    #[test]
    fn legacy_lez_config_defaults_project_kind() {
        let text = r#"
[scaffold]
version = "0.1.0"
cache_root = "/tmp/cache"

[repos.lez]
url = "https://example.com/lez.git"
source = "https://example.com/lez.git"
path = "/tmp/lez"
pin = "abc123"
"#;

        let cfg = parse_config(text).expect("parse legacy lez config");
        assert_eq!(cfg.project.kind, PROJECT_KIND_LEZ);
        assert!(cfg.lez.is_some());
        assert!(cfg.logos_module_builder.is_none());
    }

    #[test]
    fn basecamp_config_parses_and_round_trips() {
        let text = r#"
[scaffold]
version = "0.1.0"
cache_root = "/tmp/cache"

[project]
kind = "basecamp-qml"

[repos.logos_module_builder]
url = "https://github.com/logos-co/logos-module-builder.git"
source = "/tmp/module-builder"
path = "/tmp/cache/repos/logos-module-builder/pin"
pin = "deadbeef"

[basecamp]
data_root = "/tmp/runtime/LogosBasecamp"
runtime_variant = "dev"
"#;

        let cfg = parse_config(text).expect("parse basecamp config");
        assert_eq!(cfg.project.kind, PROJECT_KIND_BASECAMP_QML);
        assert!(cfg.lez.is_none());
        assert_eq!(
            cfg.logos_module_builder
                .as_ref()
                .expect("module builder")
                .pin,
            "deadbeef"
        );
        assert_eq!(cfg.basecamp.runtime_variant, BASECAMP_RUNTIME_DEV);

        let serialized = serialize_config(&cfg);
        let reparsed = parse_config(&serialized).expect("reparse serialized config");
        assert_eq!(reparsed.project.kind, PROJECT_KIND_BASECAMP_QML);
        assert_eq!(reparsed.basecamp.data_root, "/tmp/runtime/LogosBasecamp");
        assert_eq!(reparsed.basecamp.runtime_variant, BASECAMP_RUNTIME_DEV);
    }
}
