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
/// Basecamp commit pin. Empty string signals that no default is shipped;
/// the user must set `[basecamp].pin` in `scaffold.toml` before running
/// `basecamp setup`. Projects can override locally.
pub(crate) const DEFAULT_BASECAMP_PIN: &str = "";
pub(crate) const BASECAMP_PROFILE_ALICE: &str = "alice";
pub(crate) const BASECAMP_PROFILE_BOB: &str = "bob";
pub(crate) const BASECAMP_XDG_APP_SUBPATH: &str = "Logos/LogosBasecamp";
