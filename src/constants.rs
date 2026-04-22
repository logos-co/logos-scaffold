pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const LEZ_URL: &str = "https://github.com/logos-blockchain/logos-execution-zone.git";
pub(crate) const DEFAULT_LEZ_PIN: &str = "35d8df0d031315219f94d1546ceb862b0e5b208f";
pub(crate) const DEFAULT_HELLO_WORLD_IMAGE_ID_HEX: &str =
    "4880b298f59699c1e4263c5c2245c80123632d608b9116f4b253c63e6c340771";
pub(crate) const DEFAULT_WALLET_PASSWORD: &str = "logos-scaffold-v0";
pub(crate) const WALLET_BIN_REL_PATH: &str = "target/release/wallet";
pub(crate) const FRAMEWORK_KIND_DEFAULT: &str = "default";
pub(crate) const FRAMEWORK_KIND_LEZ_FRAMEWORK: &str = "lez-framework";
pub(crate) const DEFAULT_FRAMEWORK_VERSION: &str = "0.1.0";
pub(crate) const DEFAULT_FRAMEWORK_IDL_SPEC: &str = "lssa-idl/0.1.0";
pub(crate) const DEFAULT_FRAMEWORK_IDL_PATH: &str = "idl";
pub(crate) const SEQUENCER_BIN_REL_PATH: &str = "target/release/sequencer_service";
pub(crate) const SEQUENCER_CONFIG_REL_PATH: &str =
    "sequencer/service/configs/debug/sequencer_config.json";
pub(crate) const BASECAMP_URL: &str = "https://github.com/logos-co/logos-basecamp.git";
/// Basecamp commit pin — `logos-basecamp` tag `v0.1.1`.
/// Projects can override via `[basecamp].pin` in `scaffold.toml`.
pub(crate) const DEFAULT_BASECAMP_PIN: &str = "a746cdbc521f72ee22c5a4856fd17a9802bb9d69";
pub(crate) const BASECAMP_PROFILE_ALICE: &str = "alice";
pub(crate) const BASECAMP_PROFILE_BOB: &str = "bob";
/// Relative path (under the project root) to the per-profile XDG tree root.
pub(crate) const BASECAMP_PROFILES_REL: &str = ".scaffold/basecamp/profiles";
/// Subdirectories of the project root that `basecamp install` auto-discovery
/// never descends into when probing for `.lgx`-producing flakes. Hidden dirs
/// (those starting with `.`) are skipped separately and are not listed here.
/// The configured `cache_root` is prepended at call sites — it's dynamic.
pub(crate) const BASECAMP_AUTODISCOVER_SKIP_SUBDIRS: &[&str] =
    &["target", "node_modules", "result"];
/// Path under `XDG_DATA_HOME` (and `XDG_CACHE_HOME`) where basecamp reads and
/// writes its user data — modules, plugins, preinstall seed. Must match the
/// Qt `QApplication::applicationName()` the pinned basecamp binary is built
/// with: dev builds use `LogosBasecampDev`; release builds would use
/// `LogosBasecamp`. The current pin (`DEFAULT_BASECAMP_PIN`) is a dev build,
/// so lgpm must install under `LogosBasecampDev` for basecamp to discover
/// the installed modules at launch.
pub(crate) const BASECAMP_XDG_APP_SUBPATH: &str = "Logos/LogosBasecampDev";
/// Default flake ref for the `lgpm` CLI. The basecamp flake does not expose `lgpm`;
/// it lives in a separate repo (`logos-package-manager`). Pin alongside basecamp
/// so dogfooding is reproducible. Override via `[basecamp].lgpm_flake` in scaffold.toml.
pub(crate) const DEFAULT_LGPM_FLAKE: &str =
    "github:logos-co/logos-package-manager/9101875bc103214855bc6217834e22e66802ed86#cli";
