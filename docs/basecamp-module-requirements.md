# Basecamp Module Requirements

This is the contract between a module project and `logos-scaffold basecamp {setup,install,launch,reset,build-portable}`. If your project satisfies the rules below, the commands will resolve, build, and install your `.lgx` artefacts into the pre-seeded `alice` and `bob` profiles automatically. `build-portable` additionally targets the `lgx-portable` flake output for hand-loading into a basecamp AppImage.

## Hard requirements

1. **`scaffold.toml` at the project root.** Basecamp commands refuse to run outside a scaffold project. Run `logos-scaffold init` once if you don't have one.

2. **`basecamp setup` must have been run once** in the project. It pins the basecamp repo, builds the `basecamp` + `lgpm` binaries, and seeds the `alice` / `bob` profile directories under `.scaffold/basecamp/profiles/`. `install` and `launch` will emit a targeted hint if you skip this.

3. **At least one `flake.nix`** that exposes a `.lgx` package:
   - Either at the project root, or
   - In one or more immediate sub-directories (one per sub-flake).

4. **Each such flake must expose `packages.<system>.lgx`** — this is the convention established by `logos-module-builder` (tag `tutorial-v1`).
   - If a flake only exposes `packages.<system>.lgx-portable`, the resolver fails explicitly with a hint — it will not silently fall back. Expose `lgx` or pass `--flake <ref>#lgx-portable` on the command line to opt in.
   - If no flake exposes any `.lgx` attribute, the resolver fails with a generic hint pointing at `--path` / `--flake`.

## Conventions that matter for local development

Multi-flake projects (e.g. a `tictactoe` core plus `tictactoe-ui-cpp` and `tictactoe-ui-qml` sibling flakes) rely on one convention:

**Sibling sub-flakes are auto-overridden by directory name.** When scaffold builds a sub-flake, it passes `--override-input <dir-name> path:<sibling-abs-path>` for every other path-flake that shares the same parent directory — but only if the target flake actually declares `<dir-name>` as one of its inputs.

Concretely:

- Directory layout `my-module/{core,ui-cpp,ui-qml}` with `flake.nix` in each.
- `ui-cpp/flake.nix` must declare `inputs.core.url = "path:../core";` (the input name must match the sibling directory name).
- When scaffold builds `ui-cpp#lgx`, it automatically passes `--override-input core path:/abs/path/to/my-module/core`, so you get a local-source build rather than the pinned `github:` ref.
- If `ui-cpp` does not declare `core` as an input, the override is silently dropped (no nix warning). If `ui-cpp` declares `core-lib` instead, rename the input to `core` to match the directory, or rename the directory to `core-lib`.

Non-path flake refs (e.g. `github:…`) and `.lgx` path sources are never auto-overridden.

## Explicit escape hatch

If the resolver doesn't do what you want, pass explicit sources to `basecamp install`:

```bash
# Pre-built .lgx files
logos-scaffold basecamp install --path ./dist/my-module.lgx

# Arbitrary flake refs (including portable variant, remote refs, non-standard attrs)
logos-scaffold basecamp install --flake .#lgx-portable
logos-scaffold basecamp install --flake github:me/my-module#lgx
```

Explicit sources win over auto-discovery entirely — no root or sub-flake probing happens when `--path` or `--flake` is present.

## AppImage testing via `build-portable`

`basecamp install` / `launch` load modules into the scaffold-managed alice/bob profiles. To instead test against a released basecamp **AppImage**, use `build-portable`:

```bash
logos-scaffold basecamp build-portable
# → builds .#lgx-portable for each auto-discovered source
# → prints the absolute store paths of the built .lgx artefacts
# → leaves `./result-lgx-portable` symlinks next to each flake
```

`build-portable` does not touch profiles, `basecamp.state`, or the AppImage itself — it only produces artefacts. Copy them into your AppImage's module directory manually; scaffold is intentionally unaware of that path.

If a flake exposes only `lgx` (not `lgx-portable`), `build-portable` fails with a targeted hint — mirror of the `install` portable-only failure, in reverse.

## Quick checklist

- [ ] `scaffold.toml` exists at the project root.
- [ ] `logos-scaffold basecamp setup` has been run.
- [ ] Each sub-flake exposes `packages.<system>.lgx`.
- [ ] Sibling input names match sibling directory names, if applicable.
- [ ] No project relies on `lgx-portable` as the only output without passing `--flake` explicitly.
