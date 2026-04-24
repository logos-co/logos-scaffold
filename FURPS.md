# Scaffold — FURPS+

## FURPS+ (v0.1)

[v0.1 milestone](https://github.com/logos-co/ecosystem/milestone/9)

### Functionality

1. One public DevNet vertical slice: generate wallet, fund wallet, deploy contract, execute one transaction type, verify result.
2. Integrate wallet generation as part of the scaffold workflow for bootstrap and interaction flows.
3. Support native token topup for wallet operations on local and DevNet environments.

### Usability

1. Single command bootstrap with no manual project wiring required.
2. Generated layout clearly separates contract code, client code, config, and deploy scripts.
3. Deterministic wallet generation and .env handling for repeatability.
4. Clear happy-path docs, reproducible setup, discoverable commands.
5. CLI prints underlying commands for each step so users can drop down to lower-level tooling.

### Reliability

1. The vertical slice must succeed 3 times in a row on a clean machine with deterministic wallets.
2. Local network can be started and torn down in isolation without modifying host-global blockchain state.

### Performance

1. Each workshop step must complete within a demo-tolerable threshold (a few minutes).

### Supportability

1. Scaffold version and toolchain versions are explicit in generated output so projects remain buildable over time.
2. Network configuration for local and DevNet deployment is .env based config.
3. The scaffolded project includes command references for build, deploy, and interaction steps.

### + (Privacy, Anonymity, Censorship-Resistance)

- Local workflow does not require uploading source code, artifacts, or private keys to third-party services.
- CLI interaction flow works with locally controlled wallet keys and does not require custodial key management.
- Local development and testing can run fully offline from public networks.
- DevNet interaction uses explicit wallet and RPC configuration so developers can avoid accidental cross-network key reuse.

### Dependencies

#### Internal Dependencies

- Logos Core DevEx for overall developer journey alignment and terminology.
- Logos Blockchain and Logos Execution Environment for functionality.
- Wallet Module for interactions with Logos Execution Environment.

#### Runtime Dependencies

- Local network runtime availability for local deploy and interaction workflows.
- DevNet RPC endpoint availability and stable chain configuration.
- Deterministic local/DevNet account and chain configuration via environment files.

#### Wallet Dependencies

- Wallet available for signing transactions initiated by CLI interaction commands.
- Network-aware wallet configuration to prevent cross-network key misuse.

## FURPS+ (v0.2 — Basecamp Profiles)

### Functionality

1. Fetch and build a pinned basecamp (`nix build '.#app'`) and pinned `lgpm` as project-local artifacts, in the same pin-isolated cache layout used for LEZ.
2. Pre-seed two isolated basecamp profiles (`alice`, `bob`) per project for p2p dogfooding.
3. Build and install the project's `.lgx` module(s) into one or both profiles via `lgpm`, with source resolution that follows the `.#lgx` flake-output convention used by existing modules.
4. Launch basecamp for a named profile with clean-slate semantics: kill any prior process tree for that profile, scrub the profile directory, reinstall recorded `.lgx` sources, and `exec` basecamp with profile-scoped `XDG_*` environment.
5. Set per-profile values for each module's documented port-override env vars on `launch` (names owned by each module), so multiple profiles can coexist without port collisions on the same machine.

### Usability

1. `basecamp setup` is opt-in — it is never triggered implicitly by `new`, the top-level `setup`, or `build`.
2. When `install` or `launch` run without prior `basecamp setup`, the CLI prints a single one-line hint pointing at the required command instead of erroring with a raw subprocess trace.
3. When only `.#lgx-portable` is found on a project, the CLI fails explicitly, names the missing `.#lgx` output, and suggests `--flake <ref>#lgx-portable` for explicit opt-in.
4. Commands follow the existing `logos-scaffold` CLI idioms (subcommand groups, `--help` output, project-context errors).

### Reliability

1. Two `basecamp launch` invocations for different profiles on the same machine run concurrently without colliding on XDG paths, p2p identity keys, or module ports (subject to modules honoring the external port-override contract).
2. `basecamp setup` is idempotent when the pinned commit is unchanged: no rebuild, no reseeding, no state mutation.
3. `basecamp launch <profile>` produces a reproducible profile state — clean-slate on every invocation.
4. `rm -rf` during scrub targets only paths under `<project>/.scaffold/basecamp/profiles/<name>/`.

### Performance

1. `basecamp install` completes in the low-seconds range with a warm Nix cache; cold first-run wall-clock is bounded by upstream `nix build '.#lgx'` time.
2. `basecamp setup` first-run wall-clock is bounded by upstream basecamp + `lgpm` build time; re-runs on unchanged pin are effectively instant.

### Supportability

1. `logos-scaffold doctor` gains a basecamp section when `.scaffold/basecamp/` exists, covering binary presence, profile integrity, and installed-module state.
2. Basecamp and `lgpm` pinned commits are explicit in `scaffold.toml`.
3. `.scaffold/state/basecamp.state` is plain-text and line-oriented, matching existing scaffold state conventions.
4. Dogfooding scenarios (`B1`–`B4` in `DOGFOODING.md`) cover setup, single-instance, multi-instance p2p, and clean-slate behaviors.

### + (Privacy, Anonymity, Censorship-Resistance)

- Per-profile isolation of p2p identity keys: fresh profile directory produces a fresh libp2p / Waku identity with no cross-profile leakage.
- No state mutation outside the project's `.scaffold/` directory — the user's global Logos state is never touched.
- No telemetry, no upload of module artifacts, identities, or profile state to third-party services.

### Dependencies

#### Internal Dependencies

- Logos Basecamp (dev variant only in v0.2).
- Logos Package Manager (`lgpm`).
- Module repositories (delivery, storage, etc.) exposing env-var overrides for every listening port, with env var names chosen and documented by each module; tracked via upstream issues (e.g., [logos-delivery-module#18](https://github.com/logos-co/logos-delivery-module/issues/18)).

#### Runtime Dependencies

- Nix with flakes enabled on the developer machine.
- Qt build toolchain (supplied via the basecamp flake dev shell).
- Unix-like OS (Linux, macOS). Windows is out of scope.

#### Module Dependencies

- `.#lgx` flake output on the project (or sub-flakes) — `.#lgx-portable`-only projects fail explicitly until they expose `.#lgx`.
- Modules that bind sockets must honor external port override via env var (names chosen by each module) for multi-instance launch to be fully useful.

## FURPS+ (v0.3 — Build Portable)

Extends v0.2 with a release-style `.lgx-portable` build path for AppImage testing.

### Functionality

1. `basecamp build-portable` builds the project's `.#lgx-portable` flake outputs (the variant that loads cleanly into a release basecamp AppImage), orders them topologically by `metadata.json` dependencies so leaves load first, symlinks the results into `<project>/.scaffold/basecamp/portable/` with names carrying the load order, and prints those symlink paths. The wipe-and-recreate on every run keeps the staging dir idempotent.
2. Source resolution for `build-portable` reuses the same auto-discovery + `--path` / `--flake` escape hatches as `install`, but targets `#lgx-portable` instead of `#lgx`.

### Usability

1. Projects exposing only `.#lgx` (no `.#lgx-portable`) receive a targeted hint naming the missing attribute and suggesting `--flake <ref>#lgx-portable` for explicit opt-in — mirror of the v0.2 failure mode, in reverse.
2. `build-portable` stages a user-facing mirror of every built artefact as a symlink under `<project>/.scaffold/basecamp/portable/<NN>-<module_name>.lgx`. The two-digit `NN` is the load-order index so a file-browser lists the artefacts in the exact order basecamp needs to load them — the AppImage's "install lgx" picker sees human-named files in the right order rather than opaque `/nix/store/…-source/…` paths. Nix's own `./result-lgx-portable` symlinks still land next to each flake; the scaffold-owned dir is a separate concern layered on top.

### Reliability

1. `build-portable` writes only under `<project>/.scaffold/basecamp/portable/` (a wiped-and-recreated staging dir of symlinks into the nix store), never invokes `lgpm`, and never touches `basecamp.state` or the `alice`/`bob` profile trees — so a failed portable build cannot corrupt install/launch state.

### Supportability

1. `build-portable`'s manual load-into-AppImage step is explicit: scaffold stages browsable symlinks under `.scaffold/basecamp/portable/` but does not know or auto-feed the AppImage's install dialog. The AppImage lifecycle is intentionally outside scaffold's scope — see ADR "AppImage Path is Outside Scaffold's Scope".
2. Known limitation: multi-sub-flake projects must unify transitive `logos-module-builder` references via `inputs.<dep>.inputs.logos-module-builder.follows = "logos-module-builder"`. Without it, `install` can fail via the overridden sibling's lock even when a direct `nix build` succeeds. Documented fully in `docs/basecamp-module-requirements.md`; expected to become obsolete once upstream `logos-module-builder` scaffolding emits this `follows` automatically.

### Dependencies

#### Module Dependencies

- `.#lgx-portable` flake output for any module the developer wants to test against a basecamp AppImage. Projects without it get a clear error from `build-portable`, not a silent miss.

## FURPS+ (v0.4 — Module Identity in scaffold.toml)

Every captured module — whether a project source or a runtime dependency — carries a `module_name` in `scaffold.toml` that matches the identifier used in other sources' `metadata.json` `dependencies` array. Dep resolution is a key lookup against that table, and the captured module set is reviewable in version control.

### Functionality

1. `scaffold.toml` gains one `[basecamp.modules.<module_name>]` sub-section per captured module, with `flake` and `role` (`project` | `dependency`) fields. The collection of these sub-sections is the sole source of truth for the captured module set; `basecamp.state` holds only derived artefacts (pin outputs, binaries). Sub-section form fits scaffold's existing line-oriented TOML parser — no inline tables.
2. `basecamp modules` writes `[basecamp.modules]` during capture. For each captured source, the command derives `module_name` as follows:
   - `path:` flake ref → read `<flake-path>/metadata.json`, use `.name`. Deterministic.
   - `.lgx` file path → read `metadata.json` from the sibling directory if present; otherwise fall back to the filename stem.
   - `github:` flake ref → heuristic: strip `logos-` prefix from the repo stem, replace `-` with `_`. Printed at capture time with an assumption note (see Usability 1).
3. Dep resolution walks each project source's `metadata.json` `dependencies` array and, for each declared name:
   - Already keyed in `[basecamp.modules]` → no-op (already covered, irrespective of role).
   - In `BASECAMP_PREINSTALLED_MODULES` → no-op (basecamp ships it).
   - Not covered → resolve a flake ref via the declaring source's `flake.lock`, then the scaffold-default pin table. On success, insert into `[basecamp.modules]` with `role = "dependency"`.
   - Unresolved after all fallbacks → fail with a targeted error naming the two user-side fixes (capture as project source, or add an explicit dependency entry). No silent skip.
4. `[basecamp.dependencies]` (the legacy override table) is removed. Its role is subsumed by explicit `role = "dependency"` entries in `[basecamp.modules]`.

### Usability

1. For each `github:` flake where scaffold derives `module_name` from the repo slug, `basecamp modules` prints exactly one assumption note at capture time: the flake ref and the inferred `module_name`, with "edit `[basecamp.modules]` in scaffold.toml if wrong." One-time UX cost, never repeats.
2. `scaffold.toml` is human-editable at all times. `basecamp modules` is idempotent: if a key already exists in `[basecamp.modules]`, its `module_name` and `role` are preserved (user intent wins over auto-derivation).
3. Unresolved dep diagnostics are a fail-fast error at `basecamp modules` time — the dep name must resolve to an entry in `[basecamp.modules]`, a `metadata.json` source flake-input pin, the scaffold default pin table, or the basecamp preinstall list, otherwise the command exits non-zero before writing any state. No warn-and-skip path.
4. No migration path: the whole `basecamp` subcommand is unreleased. Users on earlier iterations re-run `basecamp modules` against a fresh scaffold.toml.

### Reliability

1. Dep resolution is deterministic given the same `scaffold.toml` and source `metadata.json` files — no reliance on github repo naming conventions, no string substring matches, no ordering dependencies.
2. `basecamp modules` writes to `scaffold.toml` atomically (write-temp-then-rename) so a crash mid-write cannot corrupt an otherwise-valid scaffold.toml.
3. Re-running `basecamp modules` with an unchanged project set is a no-op against `scaffold.toml` contents; hashes of the serialized section match byte-for-byte on re-entry.

### Supportability

1. Assumption notes from Usability 1 are printed to stderr (not the captured log), so pasting them into a bug report is straightforward.
2. `scaffold.toml` diffs in version control surface module-identity changes as explicit, reviewable edits — same footing as any other project config change.

### Dependencies

#### Internal Dependencies

- Module `metadata.json` schema: `name` (string), `dependencies` (array of strings). Already documented in `docs/basecamp-module-requirements.md`.
