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
4. `basecamp profile list` exposes per-profile state in both human and `--json` forms.
5. Commands follow the existing `logos-scaffold` CLI idioms (subcommand groups, `--help` output, project-context errors).

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

## FURPS+ (v0.3 — Build Portable + Reset)

Extends v0.2 with two subcommands: a release-style `.lgx-portable` build path for AppImage testing, and a single-command teardown of installed-module profile state.

### Functionality

1. `basecamp build-portable` builds the project's `.#lgx-portable` flake outputs (the variant that loads cleanly into a release basecamp AppImage) and prints the absolute store paths of the resulting `.lgx` artefacts.
2. `basecamp reset` kills any live basecamp tracked in per-profile `launch.state`, wipes `.scaffold/basecamp/profiles/`, clears recorded `sources` in `basecamp.state` (preserving the pinned basecamp + lgpm binaries), and re-seeds empty `alice` / `bob` profiles in the same run.
3. Source resolution for `build-portable` reuses the same auto-discovery + `--path` / `--flake` escape hatches as `install`, but targets `#lgx-portable` instead of `#lgx`.
4. `basecamp reset --dry-run` prints the full action plan (PIDs to kill, paths to remove, source count to clear, profiles to re-seed) and exits 0 without side effects.

### Usability

1. Projects exposing only `.#lgx` (no `.#lgx-portable`) receive a targeted hint naming the missing attribute and suggesting `--flake <ref>#lgx-portable` for explicit opt-in — mirror of the v0.2 failure mode, in reverse.
2. `build-portable` leaves the symlink destination to `nix build`'s default — no `.scaffold/`-owned output directory, so each flake's `./result-lgx-portable` symlink lands next to its `flake.nix`. Multi-flake projects get per-flake-dir disambiguation for free.
3. `reset` prints the action plan before any destructive step, both for `--dry-run` and live invocations, so users see what's about to happen without surprises.
4. `reset` exits with a targeted hint (run `basecamp setup`) when invoked before the basecamp state exists.

### Reliability

1. `reset` re-verifies each recorded PID's `/proc/<pid>/comm` against the pinned basecamp binary before issuing TERM or KILL, avoiding PID-reuse hazards.
2. `reset` refuses to remove `.scaffold/basecamp/profiles/` when that path resolves (via symlink or otherwise) outside `.scaffold/basecamp/` — enforced by the same `canonicalize_under` guard used by `scrub_profile_data_and_cache`.
3. `build-portable` never writes under `.scaffold/`, never invokes `lgpm`, and never touches `basecamp.state`, so a failed portable build cannot corrupt the scaffold-managed profile trees.
4. `reset` is idempotent: a second invocation on a freshly-reset project prints an empty kill plan and completes cleanly.

### Supportability

1. `build-portable`'s manual copy-to-AppImage step is explicit: scaffold does not know or print the AppImage's destination path. The AppImage lifecycle is intentionally outside scaffold's scope — see ADR "AppImage Path is Outside Scaffold's Scope".
2. `reset`'s action plan doubles as a verification checklist when debugging stale profile state.

### Dependencies

#### Module Dependencies

- `.#lgx-portable` flake output for any module the developer wants to test against a basecamp AppImage. Projects without it get a clear error from `build-portable`, not a silent miss.
