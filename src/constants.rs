pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const LEZ_URL: &str = "https://github.com/logos-blockchain/logos-execution-zone.git";
pub(crate) const SPEL_URL: &str = "https://github.com/logos-co/spel.git";

/// Two-form git pin: SHA (used in scaffold.toml `[repos.*].pin` and in
/// `check_repo` git-head comparisons) plus tag (used by `check_spel_lez_alignment`
/// and by user-project Cargo.toml git-dep substitution).
pub(crate) struct GitRef {
    pub(crate) sha: &'static str,
    pub(crate) tag: &'static str,
}

// IMPORTANT — single source of truth for framework pins. When bumping,
// change the literal in this file ONLY. Every site that needs the SHA
// reads `.sha`; every site that needs the tag reads `.tag`. Active
// downstream consumers:
//
//   - `commands/init.rs` / `commands/new.rs` — write `[repos.*].pin = <sha>`
//     into the user's scaffold.toml.
//   - `commands/doctor.rs` — drift checks against the user's scaffold.toml
//     pin and the `check_spel_lez_alignment` string match against spel's
//     vendored LEZ ref (tag form takes priority, SHA fallback).
//   - `template/project.rs` — substitutes `{{lez_pin}}` (SHA) and
//     `{{spel_tag}}` (tag) into rendered Cargo.toml files. After PR 19
//     (`logos-co/logos-scaffold#19`) merges, the lez-framework template
//     gains three `tag = "{{spel_tag}}"` lines that read DEFAULT_SPEL.tag.
//
// Special note on DEFAULT_SPEL: the unsuffixed `v0.2.0` tag (commit
// `72fc22…`) is *older* than `v0.2.0-rc.5` (commit `ed3bbe…`). We pick
// rc.5 because its `spel-cli/Cargo.toml` pins LEZ via `tag = "v0.2.0-rc1"`
// — i.e. the same LEZ commit `DEFAULT_LEZ.sha` resolves to. Picking
// v0.2.0 would have spel build against an older LEZ (`ffcbc15`, 119
// commits behind scaffold's pin); spel's sequencer-RPC client would
// then speak a different protocol than the sequencer scaffold itself
// builds. `check_spel_lez_alignment` in `commands/doctor.rs` enforces
// this invariant at runtime.
pub(crate) const DEFAULT_LEZ: GitRef = GitRef {
    sha: "35d8df0d031315219f94d1546ceb862b0e5b208f",
    tag: "v0.2.0-rc1",
};
pub(crate) const DEFAULT_SPEL: GitRef = GitRef {
    sha: "ed3bbedb4b684645da05455d30a4a0be7cc4dfe0",
    tag: "v0.2.0-rc.5",
};

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
/// Project-relative directory holding the Risc0 guest crate (`methods/Cargo.toml`,
/// `methods/guest/...`). Shared between the build side (`build_methods_guests`),
/// which compiles the manifest, and the deploy side, which discovers the resulting
/// `.bin` artefacts under `methods/target/...`.
pub(crate) const METHODS_DIR: &str = "methods";
pub(crate) const SEQUENCER_CONFIG_REL_PATH: &str =
    "sequencer/service/configs/debug/sequencer_config.json";
pub(crate) const SPEL_BIN_REL_PATH: &str = "target/release/spel";
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
///
/// Pinned to `logos-package-manager` tag `tutorial-v1` (the last pre-validation
/// commit). PR #8 introduced content-hash validation in the manifest; later
/// lgpm commits tightened it further. Neither is compatible today with the
/// `.lgx` files emitted by `logos-module-builder` tag `tutorial-v1`, which
/// does not populate content hashes. Revisit when module-builder starts
/// emitting hashes (or lgpm gains a compatibility mode).
pub(crate) const DEFAULT_LGPM_FLAKE: &str =
    "github:logos-co/logos-package-manager/e5c25989861f4487c3dc8c7b3bc0062bcbc3221f#cli";

/// Scaffold-level default pins for runtime companion modules that basecamp
/// v0.1.1 does NOT preinstall (listed in the Package Manager UI catalog but
/// shipped as portable-only, so dev basecamp can't load them). When
/// `basecamp modules` auto-discovery walks a project's `metadata.json` and
/// finds a dep in this table, it captures the pinned flake ref into
/// `[basecamp.modules]` so `install` builds and installs the dev variant.
///
/// Keyed by the module name as it appears in `metadata.json` `dependencies`.
/// Paired conceptually with `DEFAULT_BASECAMP_PIN` — when basecamp bumps, revisit
/// these pins to stay ABI-compatible. Per-project overrides go in
/// `[basecamp.dependencies]` in `scaffold.toml`.
///
/// See the upstream issue tracking a proper `logos-modules` release pin:
/// <https://github.com/logos-co/logos-basecamp/issues/167>. Once that lands
/// scaffold can derive this table from basecamp's own manifest rather than
/// carrying an opinion.
pub(crate) const BASECAMP_DEPENDENCIES: &[(&str, &str)] = &[
    // `logos-delivery-module/1.0.0` (tutorial-v1 era) predates the `#lgx`
    // flake-output convention and does NOT expose `packages.<sys>.lgx` — a
    // cold `basecamp install` against that pin fails at the resolver.
    //
    // Pin to the head of `tutorial-v1-compat` on logos-delivery-module
    // (commit `1fde1566…`, 2026-04-22) — the rev that both `tictactoe` and
    // `yolo-board-module` use in their own flakes. This is the known-good
    // default; per-project overrides in `[basecamp.dependencies]` in
    // `scaffold.toml` take precedence, and `basecamp modules` auto-discovery
    // prefers any matching input found in the project's own `flake.lock`
    // over this table (so a project's own pin always wins).
    (
        "delivery_module",
        "github:logos-co/logos-delivery-module/1fde1566291fe062b98255003b9166b0261c6081#lgx",
    ),
    // Additional companions (storage_module, etc.) added on demand as real
    // projects declare them. Keeping the starter set small avoids surprising
    // users with unnecessary companion builds.
];

/// Modules that basecamp v0.1.1 preinstalls on first launch (from its
/// `preinstall/` dir). These must NEVER be captured as dependencies by the
/// auto-discovery walk — basecamp provides them itself.
///
/// Kept in sync with `<basecamp>/preinstall/*.lgx` manually. Inspect the nix
/// build output to verify this list stays accurate when bumping the basecamp pin.
pub(crate) const BASECAMP_PREINSTALLED_MODULES: &[&str] = &[
    "capability_module",
    "package_manager",
    "package_manager_ui",
    "counter",
    "counter_qml",
    "webview_app",
    "basecamp_main_ui",
];
