# Scaffold — Architecture Decision Records

## Project Structure (Single-Repo Template)

Developers need one bootstrap target that is immediately runnable and easy to modify.
Use a single generated project containing contract, CLI client, configuration, and deployment scripts.
Single-template onboarding is very simple.

## CLI

The workflow should be discoverable for new developers.
Expose one CLI surface with subcommands for init, build, deploy, and interact.
One CLI improves onboarding but makes it hard to maintain backward-compatibility.

## Local Runtime

Local development should work without requiring manually managed external node setup.
Provide embedded localnet lifecycle commands as part of scaffold workflow.
The scaffolded toolchain can start, stop, and reset a localnet environment
that supports deploy and wallet-based interaction for the generated example contract.

## Build Pipeline

Contract compilation should align with Rust ecosystem standards
and avoid unnecessary abstraction.
Use native Cargo-based build flow as the primary compilation path.

## Network Configuration

Developers need explicit, editable environment targeting for local and DevNet workflows.
Use environment-file based network configuration as the default model.
Generated projects include env files for local and DevNet,
wallet interaction settings used by deploy and wallet-based interaction commands.
Env files are familiar and automation-friendly,
but require strict handling to avoid credential leakage.

## Portable Artefact Build is Separate from Install

The `.#lgx-portable` output exists to load into a released basecamp AppImage — a
different delivery path with different XDG paths and a different install mechanism
than the scaffold-managed `alice`/`bob` profiles. `basecamp build-portable` targets
that output and stops once the artefacts exist; it never invokes `lgpm`, never
mutates profile state, and never touches `basecamp.state`. This keeps the two
delivery flows strictly separate, trading a slight command-surface duplication for
clean boundaries.

## Build-Portable Stages Symlinks in a Scaffold-Owned Directory

`basecamp build-portable` produces `.lgx` artefacts in the nix store — paths
like `/nix/store/xxx-source/foo.lgx`. Operators then load those into a
basecamp AppImage via its "install lgx" file picker, which opens in the
filesystem. Browsing to `/nix/store/` by hand through a file dialog is
painful: the hashes are opaque, there's no ordering, and the user has to
mentally reconstruct which artefact belongs to which module.

`build-portable` now writes a mirror of each built artefact as a symlink
under `<project>/.scaffold/basecamp/portable/<NN>-<module_name>.lgx`
(or `<NN>-<module_name>-<stem>.lgx` when a source emits multiple outputs).
The `NN` prefix is a two-digit zero-padded load-order index, so the
directory lists the artefacts in the exact order basecamp needs to load
them — modules with no project-internal deps first, modules that depend
on them afterwards. Ordering is derived from each source's `metadata.json`
`dependencies` array via a topological sort among the captured
`role = "project"` modules; non-project deps are ignored because they're
resolved at runtime via the basecamp preinstall or package-manager
catalog, not at hand-load time.

The symlink directory is wiped and recreated on every `build-portable`
run. That keeps re-runs idempotent: removing a module via `basecamp
modules` and re-running `build-portable` leaves no stale symlinks. The
symlinks point at live nix-store paths; the store entries themselves are
retained by their `result-lgx-portable` GC root at each flake root, so
scaffold's symlink does not itself pin the artefacts.

Tradeoffs: scaffold writes under `.scaffold/basecamp/portable/` at
`build-portable` time, relaxing an earlier design goal that
`build-portable` never touch `.scaffold/`. The upside is operator
ergonomics for the one actual friction point in the AppImage test
flow — no more hunting through the nix store in a file picker.

## AppImage Path is Outside Scaffold's Scope

`build-portable` could have tried to auto-locate a basecamp AppImage and copy
artefacts into its module directory. It does not. The scaffold does not know the
AppImage's install location, and probing filesystem heuristics would be unreliable
across Linux distributions and macOS bundles. Instead, scaffold produces the `.lgx`
artefacts and prints their absolute store paths; the developer copies them into
whatever AppImage they are testing against. The tradeoff is an extra manual step;
the upside is no fragile heuristics and no surprise writes into user-managed
locations.

## Module Identity Lives in scaffold.toml

Runtime IPC composition means a module declares dependencies on other modules by
*name* (in `metadata.json`'s `dependencies` array). Scaffold needs a mapping from
those names to concrete flake refs in the captured module set — otherwise
`basecamp install` cannot know which flake provides `delivery_module` when a
project source declares that dep.

`scaffold.toml` gains a `[basecamp.modules]` table keyed by `module_name` with
explicit `flake` and `role` (`project` | `dependency`) fields. `basecamp modules`
writes the table at capture time; dep resolution at build time is a key lookup
against that table. `scaffold.toml` is the sole human-readable source of truth
for the captured module set; `basecamp.state` is reduced to pin-artefact
metadata only.

Populating `module_name` on capture:
- `path:` flake sources — read directly from the source's `metadata.json.name`
  (the source tree is on the local filesystem).
- `github:` flake sources — `metadata.json` is only available after building,
  so scaffold derives a best-guess name from the repo slug (strip `logos-`
  prefix, `-` → `_`) and prints a one-line assumption note at capture time
  inviting the user to edit `scaffold.toml` if wrong.

The tradeoff: `basecamp modules` now writes to `scaffold.toml`, widening its
write surface beyond the derived `basecamp.state`. The upside is that the
captured module set becomes reviewable in version control and diff tooling
(a single TOML section, not a line-oriented state file), and dep-resolution
lookups become deterministic key matches — the "is this dep covered by
something I already captured?" question has an unambiguous answer.

## Sibling `--override-input` Resolves By Declared Input Name

Multi-sub-flake projects rely on `--override-input <input> path:<sibling-abs>` so a sub-flake's `path:../<sibling>` inputs resolve to the developer's working tree instead of whatever `github:` ref is in its lock. The first implementation keyed overrides by the **sibling directory name on disk** — a convention where input names are expected to match directory names. Two problems:

- Projects with snake_case input names and kebab-case directory names (e.g. `inputs.tictactoe_solo_ai.url = "path:../logos-tictactoe-solo-ai"`) fail: nix emits `input has an override for a non-existent input <dirname>`, drops the override, falls through to the original `path:..` URL, and errors out under pure-eval.
- The convention isn't visible at call sites: there's no error telling the developer to rename either the input or the directory; the break appears as a nix lock-resolution error from inside the store copy.

Scaffold now reads the target sub-flake's `flake.nix` at both probe time (`basecamp modules`) and build time (`basecamp install` / `build-portable`), looking for `<name>.url = "path:../<sibling>"` patterns, and emits `--override-input <name> path:<sibling-abs>` — keyed by the **declared input name**, not the directory name. Directory and input names no longer need to match.

The parser is line-level and recognizes the common single-line declarative forms (`<name>.url = "…"` and `inputs.<name>.url = "…"`). Multi-line nested-attrset forms (`inputs.x = { url = "…"; flake = false; };` split across lines) are out of scope — they fall through silently and the probe or build may fail with the raw pure-eval error. Widening the parser is cheaper than adding a nix-based input enumerator, since the latter also hits pure-eval constraints on the very inputs we're trying to unstick.

The tradeoff: adding a minimal flake.nix text parser to scaffold. The upside: one less convention the user has to learn to get their project building, and the error mode goes away for legitimate non-convention projects. Build-time filtering via `flake_declared_inputs` (nix flake metadata) remains as defense-in-depth; after this change, the parser's output is already constrained to declared names, so the filter is redundant on the common path and only fires when the metadata fetch itself succeeded against some nix setup the parser didn't.

## Flake Attribute Selection is a Resolver Parameter

`install` and `build-portable` share a single source-resolution routine that is
parameterized on the flake attribute name (`lgx` vs `lgx-portable`). Hardcoding two
parallel resolvers would duplicate the precedence rules (explicit → root flake →
sub-flakes → targeted failure) and bug-surface the sibling path-input override
logic. The tradeoff is one extra argument on an internal helper; the upside is
that any resolver fix automatically applies to both commands, and the "only
`.#lgx-portable` found" and "only `.#lgx` found" error paths are symmetric by
construction.

## Build Output Discovery

The deploy command must work for any scaffolded project regardless of its name.
Binary discovery should derive paths from the actual project structure, not assume the template's naming.
The implementation walks both `target/riscv-guest/` (the canonical risc0 layout used by the
scaffold template) and `methods/target/` (the sub-crate workspace layout), matching `<program>.bin`
files whose path components include both a `riscv32im*` target triple and a `release` directory.
Release builds are preferred; if only a debug build exists, that is used as a fallback. When
multiple matches exist, the shallowest path wins. This keeps the scaffold general and avoids
coupling to a specific project name or workspace layout.
